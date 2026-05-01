import { access, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import {
  slintUpdaterMetadataFilename,
  slintWindowsInstallerName,
  updaterReleasePaths,
} from './updater-release.mjs';

export async function slintRequiredArtifactPaths({ root = defaultRepoRoot() } = {}) {
  const packageJson = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
  const version = packageJson.version;
  const releasePaths = updaterReleasePaths(root, version, {
    metadataFilename: slintUpdaterMetadataFilename,
    releaseSubdir: 'slint',
    installerName: slintWindowsInstallerName(version),
  });
  const transitionReleasePaths = updaterReleasePaths(root, version, {
    metadataFilename: 'latest-alpha.json',
    releaseSubdir: 'slint',
    installerName: slintWindowsInstallerName(version),
  });
  const releaseRoot = path.join(root, 'release', 'slint');
  const stagingRoot = path.join(releaseRoot, 'staging');
  const installResourceDir = path.join(stagingRoot, 'resources', 'install');

  return {
    ...releasePaths,
    version,
    releaseRoot,
    stagingRoot,
    transitionMetadataPath: transitionReleasePaths.metadataPath,
    stagedSidecarPath: path.join(stagingRoot, 'simple-download-manager-native-host.exe'),
    installResourceDir,
    chromiumExtensionZipPath: path.join(releaseRoot, 'simple-download-manager-chromium-extension.zip'),
    firefoxExtensionZipPath: path.join(releaseRoot, 'simple-download-manager-firefox-extension.zip'),
    installResourcePaths: [
      path.join(installResourceDir, 'install.md'),
      path.join(installResourceDir, 'register-native-host.ps1'),
      path.join(installResourceDir, 'unregister-native-host.ps1'),
      path.join(installResourceDir, 'chromium.template.json'),
      path.join(installResourceDir, 'edge.template.json'),
      path.join(installResourceDir, 'firefox.template.json'),
      path.join(installResourceDir, 'release.json'),
    ],
  };
}

export async function verifySlintReleaseArtifacts({ root = defaultRepoRoot() } = {}) {
  const paths = await slintRequiredArtifactPaths({ root });
  const required = [
    ['installer', paths.installerPath],
    ['installer signature', paths.signaturePath],
    ['staged native-host sidecar', paths.stagedSidecarPath],
    ['Chromium extension zip', paths.chromiumExtensionZipPath],
    ['Firefox extension zip', paths.firefoxExtensionZipPath],
    ['Tauri transition updater metadata', paths.transitionMetadataPath],
    ['Slint updater metadata', paths.metadataPath],
    ...paths.installResourcePaths.map((resourcePath) => [
      `install resource ${path.basename(resourcePath)}`,
      resourcePath,
    ]),
  ];

  for (const [label, filePath] of required) {
    await assertFileExists(label, filePath);
  }

  const expectedInstallerName = paths.installerName;
  const transitionMetadata = JSON.parse(await readFile(paths.transitionMetadataPath, 'utf8'));
  const nativeMetadata = JSON.parse(await readFile(paths.metadataPath, 'utf8'));
  const transitionPlatform = assertMetadataInstaller(
    transitionMetadata,
    expectedInstallerName,
    'Tauri transition updater metadata',
  );
  const nativePlatform = assertMetadataInstaller(
    nativeMetadata,
    expectedInstallerName,
    'Slint updater metadata',
  );

  if (transitionPlatform.format !== undefined) {
    throw new Error('Tauri transition updater metadata must not include a format field.');
  }
  if (nativePlatform.format !== 'nsis') {
    throw new Error('Slint updater metadata must include format: "nsis".');
  }
  if (transitionPlatform.signature !== nativePlatform.signature) {
    throw new Error('Slint transition and native updater metadata signatures must match.');
  }

  return {
    installerName: expectedInstallerName,
    installerPath: paths.installerPath,
    signaturePath: paths.signaturePath,
    transitionMetadataPath: paths.transitionMetadataPath,
    metadataPath: paths.metadataPath,
  };
}

function assertMetadataInstaller(metadata, expectedInstallerName, label) {
  const platform = metadata.platforms?.['windows-x86_64'];
  const assetName = platform?.url ? decodeURIComponent(path.basename(platform.url)) : '';
  if (assetName !== expectedInstallerName) {
    throw new Error(
      `${label} references ${assetName || '<missing>'}; expected ${expectedInstallerName}.`,
    );
  }
  return platform ?? {};
}

async function assertFileExists(label, filePath) {
  try {
    await access(filePath);
  } catch {
    throw new Error(`Missing Slint release artifact: ${label} ${filePath}`);
  }
}

function defaultRepoRoot() {
  const __filename = fileURLToPath(import.meta.url);
  return path.resolve(path.dirname(__filename), '..');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const result = await verifySlintReleaseArtifacts();
  console.log(
    JSON.stringify(
      {
        installerName: result.installerName,
        installerPath: result.installerPath,
        signaturePath: result.signaturePath,
        transitionMetadataPath: result.transitionMetadataPath,
        metadataPath: result.metadataPath,
      },
      null,
      2,
    ),
  );
}
