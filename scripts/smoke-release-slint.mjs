import { access, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const NATIVE_HOST_NAME = 'com.myapp.download_manager';
export const DEFAULT_CHROMIUM_EXTENSION_ID = 'pkaojpfpjieklhinoibjibmjldohlmbb';
export const DEFAULT_FIREFOX_EXTENSION_ID = 'simple-download-manager@example.com';

export const CHROME_REGISTRY_PATH =
  'Software\\Google\\Chrome\\NativeMessagingHosts\\com.myapp.download_manager';
export const EDGE_REGISTRY_PATH =
  'Software\\Microsoft\\Edge\\NativeMessagingHosts\\com.myapp.download_manager';
export const FIREFOX_REGISTRY_PATH =
  'Software\\Mozilla\\NativeMessagingHosts\\com.myapp.download_manager';

export const CHROME_MANIFEST_FILE = 'com.myapp.download_manager.chrome.json';
export const EDGE_MANIFEST_FILE = 'com.myapp.download_manager.edge.json';
export const FIREFOX_MANIFEST_FILE = 'com.myapp.download_manager.firefox.json';
export const SLINT_APP_EXE = 'simple-download-manager.exe';
export const SLINT_SIDECAR_EXE = 'simple-download-manager-native-host.exe';

export function slintSmokeInstallLayout(installRoot) {
  const root = path.resolve(installRoot);
  const installDir = path.join(root, 'resources', 'install');
  const manifestRoot = path.join(root, 'native-messaging');

  return {
    installRoot: root,
    appExe: path.join(root, SLINT_APP_EXE),
    sidecar: path.join(root, SLINT_SIDECAR_EXE),
    uninstaller: path.join(root, 'uninstall.exe'),
    resources: {
      installDir,
      installDocs: path.join(installDir, 'install.md'),
      releaseJson: path.join(installDir, 'release.json'),
      registerScript: path.join(installDir, 'register-native-host.ps1'),
      unregisterScript: path.join(installDir, 'unregister-native-host.ps1'),
    },
    manifests: {
      root: manifestRoot,
      chrome: path.join(manifestRoot, CHROME_MANIFEST_FILE),
      edge: path.join(manifestRoot, EDGE_MANIFEST_FILE),
      firefox: path.join(manifestRoot, FIREFOX_MANIFEST_FILE),
    },
  };
}

export function nativeHostRegistryTargets(installRoot) {
  const layout = slintSmokeInstallLayout(installRoot);
  return [
    {
      browser: 'Chrome',
      key: CHROME_REGISTRY_PATH,
      manifest: layout.manifests.chrome,
    },
    {
      browser: 'Edge',
      key: EDGE_REGISTRY_PATH,
      manifest: layout.manifests.edge,
    },
    {
      browser: 'Firefox',
      key: FIREFOX_REGISTRY_PATH,
      manifest: layout.manifests.firefox,
    },
  ];
}

export function expectedBrowserExtensionIds(metadata = {}) {
  const chromium = metadata.chromiumExtensionId || DEFAULT_CHROMIUM_EXTENSION_ID;
  return {
    chromium,
    edge: metadata.edgeExtensionId || chromium,
    firefox: metadata.firefoxExtensionId || DEFAULT_FIREFOX_EXTENSION_ID,
  };
}

export async function validateInstalledNativeHostLayout({ installRoot, expectedExtensionIds } = {}) {
  if (!installRoot) {
    throw new Error('installRoot is required for Slint installer smoke validation');
  }

  const layout = slintSmokeInstallLayout(installRoot);
  await requireFile(layout.appExe, 'installed Slint app exe');
  await requireFile(layout.sidecar, 'installed Slint native-host sidecar');
  await requireFile(layout.uninstaller, 'installed NSIS uninstaller');
  await requireFile(layout.resources.installDocs, 'installed Slint install docs');
  await requireFile(layout.resources.registerScript, 'installed native-host register script');
  await requireFile(layout.resources.unregisterScript, 'installed native-host unregister script');
  await requireFile(layout.resources.releaseJson, 'installed Slint release metadata');

  const releaseMetadata = await readJsonFile(layout.resources.releaseJson, 'Slint release metadata');
  const ids = expectedExtensionIds || expectedBrowserExtensionIds(releaseMetadata);

  const manifests = [
    {
      browser: 'Chrome',
      path: layout.manifests.chrome,
      expectedKey: 'allowed_origins',
      expectedValue: `chrome-extension://${ids.chromium}/`,
    },
    {
      browser: 'Edge',
      path: layout.manifests.edge,
      expectedKey: 'allowed_origins',
      expectedValue: `chrome-extension://${ids.edge}/`,
    },
    {
      browser: 'Firefox',
      path: layout.manifests.firefox,
      expectedKey: 'allowed_extensions',
      expectedValue: ids.firefox,
    },
  ];

  const validatedManifests = [];
  for (const manifest of manifests) {
    await requireFile(manifest.path, `${manifest.browser} native-host manifest`);
    const parsed = await readJsonFile(manifest.path, `${manifest.browser} native-host manifest`);
    validateManifest(parsed, manifest, layout.sidecar);
    validatedManifests.push({
      browser: manifest.browser,
      path: manifest.path,
      hostPath: parsed.path,
    });
  }

  return {
    installRoot: layout.installRoot,
    appExePath: layout.appExe,
    sidecarPath: layout.sidecar,
    resourceDir: layout.resources.installDir,
    manifests: validatedManifests,
    registryTargets: nativeHostRegistryTargets(installRoot),
  };
}

async function requireFile(filePath, label) {
  try {
    await access(filePath);
  } catch {
    throw new Error(`Missing ${label} ${filePath}`);
  }
}

async function readJsonFile(filePath, label) {
  const content = (await readFile(filePath, 'utf8')).replace(/^\uFEFF/, '');
  try {
    return JSON.parse(content);
  } catch (error) {
    throw new Error(`Invalid ${label} JSON at ${filePath}: ${error.message}`);
  }
}

function validateManifest(manifest, expectation, sidecarPath) {
  if (manifest.name !== NATIVE_HOST_NAME) {
    throw new Error(`${expectation.browser} native-host manifest name must be ${NATIVE_HOST_NAME}`);
  }
  if (manifest.type !== 'stdio') {
    throw new Error(`${expectation.browser} native-host manifest type must be stdio`);
  }
  if (!manifest.path || !samePath(manifest.path, sidecarPath)) {
    throw new Error(
      `${expectation.browser} native-host manifest path does not point at the installed sidecar: ${manifest.path || '(missing)'}`,
    );
  }

  const values = Array.isArray(manifest[expectation.expectedKey])
    ? manifest[expectation.expectedKey]
    : [];
  if (!values.includes(expectation.expectedValue)) {
    throw new Error(
      `${expectation.browser} native-host manifest ${expectation.expectedKey} must include ${expectation.expectedValue}`,
    );
  }
}

function samePath(left, right) {
  const resolvedLeft = path.resolve(left);
  const resolvedRight = path.resolve(right);
  if (process.platform === 'win32') {
    return resolvedLeft.toLowerCase() === resolvedRight.toLowerCase();
  }
  return resolvedLeft === resolvedRight;
}

function parseCliArgs(argv) {
  const args = { installRoot: null };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--install-root') {
      args.installRoot = argv[index + 1];
      index += 1;
      continue;
    }
    if (arg === '--help' || arg === '-h') {
      args.help = true;
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }
  return args;
}

function printUsage() {
  console.log('Usage: node scripts/smoke-release-slint.mjs --install-root <path>');
}

const currentFile = fileURLToPath(import.meta.url);
if (process.argv[1] && path.resolve(process.argv[1]) === currentFile) {
  try {
    const args = parseCliArgs(process.argv.slice(2));
    if (args.help) {
      printUsage();
      process.exit(0);
    }
    const result = await validateInstalledNativeHostLayout({ installRoot: args.installRoot });
    console.log(JSON.stringify(result, null, 2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
