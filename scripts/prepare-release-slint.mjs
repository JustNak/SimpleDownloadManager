import { copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const slintSidecarBinaryName = 'simple-download-manager-native-host.exe';

export async function prepareSlintReleaseResources({
  root = defaultRepoRoot(),
  clean = true,
} = {}) {
  const slintReleaseRoot = path.join(root, 'release', 'slint');
  const stagingRoot = path.join(slintReleaseRoot, 'staging');
  const installResourceDir = path.join(stagingRoot, 'resources', 'install');
  const sidecarSource = path.join(
    root,
    'apps',
    'native-host',
    'target',
    'release',
    slintSidecarBinaryName,
  );
  const sidecarTarget = path.join(stagingRoot, slintSidecarBinaryName);

  if (clean) {
    await rm(stagingRoot, { recursive: true, force: true });
  }

  await mkdir(installResourceDir, { recursive: true });
  await copyFile(sidecarSource, sidecarTarget);

  for (const { source, destination } of [
    { source: 'docs/install.md', destination: 'install.md' },
    { source: 'scripts/register-native-host.ps1', destination: 'register-native-host.ps1' },
    { source: 'scripts/unregister-native-host.ps1', destination: 'unregister-native-host.ps1' },
    { source: 'apps/native-host/manifests/chromium.template.json', destination: 'chromium.template.json' },
    { source: 'apps/native-host/manifests/edge.template.json', destination: 'edge.template.json' },
    { source: 'apps/native-host/manifests/firefox.template.json', destination: 'firefox.template.json' },
  ]) {
    await copyFile(path.join(root, source), path.join(installResourceDir, destination));
  }

  const releaseConfig = JSON.parse(
    await readFile(path.join(root, 'config', 'release.json'), 'utf8'),
  );
  const releaseMetadata = {
    ...releaseConfig,
    sidecarBinaryName: slintSidecarBinaryName,
  };
  const releaseMetadataPath = path.join(installResourceDir, 'release.json');
  await writeFile(
    releaseMetadataPath,
    `${JSON.stringify(releaseMetadata, null, 2)}\n`,
    'utf8',
  );

  return {
    stagingRoot,
    installResourceDir,
    sidecarTarget,
    releaseMetadataPath,
  };
}

function defaultRepoRoot() {
  const __filename = fileURLToPath(import.meta.url);
  return path.resolve(path.dirname(__filename), '..');
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const result = await prepareSlintReleaseResources();
  console.log(
    JSON.stringify(
      {
        stagingRoot: result.stagingRoot,
        installResourceDir: result.installResourceDir,
        sidecarTarget: result.sidecarTarget,
        releaseMetadataPath: result.releaseMetadataPath,
      },
      null,
      2,
    ),
  );
}
