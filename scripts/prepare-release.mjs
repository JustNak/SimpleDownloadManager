import { access, copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { execSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  nativeHostTargetDir,
  windowsReleaseTargetForRustTarget,
} from './windows-release-targets.mjs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const root = path.resolve(__dirname, '..');
const args = parseArgs(process.argv.slice(2));

const config = JSON.parse(await readFile(path.join(root, 'config', 'release.json'), 'utf8'));
const hostTuple = args.target ?? execSync('rustc -vV', { cwd: root, encoding: 'utf8' })
  .split(/\r?\n/)
  .find((line) => line.startsWith('host: '))
  ?.replace('host: ', '')
  .trim();

if (!hostTuple) {
  throw new Error('Could not determine Rust host tuple.');
}

const releaseTarget = windowsReleaseTargetForRustTarget(hostTuple);
const hostBinaryName = 'simple-download-manager-native-host.exe';
const hostBinarySource = await resolveHostBinarySource({
  root,
  releaseTarget,
  hostBinaryName,
  explicitTarget: Boolean(args.target),
});
const sidecarDir = path.join(root, 'apps', 'desktop', 'src-tauri', 'binaries');
const sidecarTarget = path.join(
  sidecarDir,
  `simple-download-manager-native-host-${hostTuple}.exe`,
);

const installerResourceDir = path.join(root, 'apps', 'desktop', 'src-tauri', 'resources', 'install');
const desktopTauriRoot = path.join(root, 'apps', 'desktop', 'src-tauri');
const installerResources = [
  { source: 'docs/install.md', destination: 'resources/install/install.md' },
  { source: 'scripts/register-native-host.ps1', destination: 'resources/install/register-native-host.ps1' },
  { source: 'scripts/unregister-native-host.ps1', destination: 'resources/install/unregister-native-host.ps1' },
  { source: 'apps/native-host/manifests/chromium.template.json', destination: 'resources/install/chromium.template.json' },
  { source: 'apps/native-host/manifests/edge.template.json', destination: 'resources/install/edge.template.json' },
  { source: 'apps/native-host/manifests/firefox.template.json', destination: 'resources/install/firefox.template.json' },
];
const sevenZipResources = releaseTarget.sevenZipResources.map((resource) => ({
  source: path.join('apps/desktop/src-tauri', resource.source),
  destination: resource.destination,
}));

await rm(sidecarDir, { recursive: true, force: true });
await rm(installerResourceDir, { recursive: true, force: true });
await mkdir(sidecarDir, { recursive: true });
await mkdir(installerResourceDir, { recursive: true });

for (const resource of sevenZipResources) {
  const filePath = path.join(root, resource.source);
  await access(filePath).catch(() => {
    throw new Error(`Missing bundled 7-Zip resource: ${filePath}`);
  });
}

await copyFile(hostBinarySource, sidecarTarget);

for (const { source, destination } of installerResources) {
  await copyFile(
    path.join(root, source),
    path.join(desktopTauriRoot, destination),
  );
}

await writeFile(
  path.join(installerResourceDir, 'release.json'),
  JSON.stringify(
    {
      ...config,
      hostTuple,
      sidecarBinaryName: path.basename(sidecarTarget),
    },
    null,
    2,
  ),
  'utf8',
);

if (args.configOut) {
  await writeTauriConfigOverride({
    configPath: path.resolve(root, args.configOut),
    resources: [
      ...installerResources,
      { source: path.relative(root, path.join(installerResourceDir, 'release.json')), destination: 'resources/install/release.json' },
      ...sevenZipResources,
    ],
  });
}

console.log(
  JSON.stringify(
    {
      target: releaseTarget.name,
      hostTuple,
      sidecarTarget,
      installerResourceDir,
      tauriConfigOverride: args.configOut ? path.resolve(root, args.configOut) : null,
      chromiumExtensionId: config.chromiumExtensionId,
      firefoxExtensionId: config.firefoxExtensionId,
    },
    null,
    2,
  ),
);

function parseArgs(argv) {
  const parsed = {};
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--target') {
      parsed.target = argv[index + 1];
      index += 1;
      continue;
    }
    if (arg === '--config-out') {
      parsed.configOut = argv[index + 1];
      index += 1;
      continue;
    }
    throw new Error(`Unsupported prepare-release argument: ${arg}`);
  }
  return parsed;
}

async function writeTauriConfigOverride({ configPath, resources }) {
  await mkdir(path.dirname(configPath), { recursive: true });
  const resourceMap = Object.fromEntries(
    resources.map((resource) => [
      path.resolve(root, resource.source),
      resource.destination,
    ]),
  );
  await writeFile(
    configPath,
    `${JSON.stringify({
      build: {
        beforeBuildCommand: null,
      },
      bundle: {
        resources: resourceMap,
      },
    }, null, 2)}\n`,
    'utf8',
  );
}

async function resolveHostBinarySource({
  root,
  releaseTarget,
  hostBinaryName,
  explicitTarget,
}) {
  const targetPath = path.join(nativeHostTargetDir(root, releaseTarget), hostBinaryName);
  if (explicitTarget) {
    return targetPath;
  }

  try {
    await access(targetPath);
    return targetPath;
  } catch {
    const legacyPath = path.join(
      root,
      'apps',
      'native-host',
      'target',
      'release',
      hostBinaryName,
    );
    await access(legacyPath);
    return legacyPath;
  }
}
