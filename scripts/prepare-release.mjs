import { copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { execSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const root = path.resolve(__dirname, '..');

const config = JSON.parse(await readFile(path.join(root, 'config', 'release.json'), 'utf8'));
const hostTuple = execSync('rustc -vV', { cwd: root, encoding: 'utf8' })
  .split(/\r?\n/)
  .find((line) => line.startsWith('host: '))
  ?.replace('host: ', '')
  .trim();

if (!hostTuple) {
  throw new Error('Could not determine Rust host tuple.');
}

const hostBinaryName = 'simple-download-manager-native-host.exe';
const hostBinarySource = path.join(root, 'apps', 'native-host', 'target', 'release', hostBinaryName);
const sidecarDir = path.join(root, 'apps', 'desktop', 'src-tauri', 'binaries');
const sidecarTarget = path.join(
  sidecarDir,
  `simple-download-manager-native-host-${hostTuple}.exe`,
);

const installerResourceDir = path.join(root, 'apps', 'desktop', 'src-tauri', 'resources', 'install');

await rm(sidecarDir, { recursive: true, force: true });
await rm(installerResourceDir, { recursive: true, force: true });
await mkdir(sidecarDir, { recursive: true });
await mkdir(installerResourceDir, { recursive: true });

await copyFile(hostBinarySource, sidecarTarget);

for (const { source, destination } of [
  { source: 'docs/install.md', destination: 'install.md' },
  { source: 'scripts/register-native-host.ps1', destination: 'register-native-host.ps1' },
  { source: 'scripts/unregister-native-host.ps1', destination: 'unregister-native-host.ps1' },
  { source: 'apps/native-host/manifests/chromium.template.json', destination: 'chromium.template.json' },
  { source: 'apps/native-host/manifests/edge.template.json', destination: 'edge.template.json' },
  { source: 'apps/native-host/manifests/firefox.template.json', destination: 'firefox.template.json' },
]) {
  await copyFile(path.join(root, source), path.join(installerResourceDir, destination));
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

console.log(
  JSON.stringify(
    {
      hostTuple,
      sidecarTarget,
      installerResourceDir,
      chromiumExtensionId: config.chromiumExtensionId,
      firefoxExtensionId: config.firefoxExtensionId,
    },
    null,
    2,
  ),
);
