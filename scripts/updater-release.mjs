import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const updaterReleaseTag = 'updater-alpha';
export const updaterMetadataFilename = 'latest-alpha.json';
export const updaterRepository = 'JustNak/SimpleDownloadManager';

export function requireSigningEnvironment(env = process.env) {
  if (!env.TAURI_SIGNING_PRIVATE_KEY?.trim()) {
    throw new Error('TAURI_SIGNING_PRIVATE_KEY is required to build signed updater artifacts.');
  }
}

export function windowsInstallerName(version) {
  return `Simple Download Manager_${version}_x64-setup.exe`;
}

export function updaterAssetUrl(repository, releaseTag, assetName) {
  return `https://github.com/${repository}/releases/download/${releaseTag}/${encodeURIComponent(assetName)}`;
}

export function createLatestAlphaJson({
  version,
  notes,
  pubDate,
  url,
  signature,
}) {
  return {
    version,
    notes,
    pub_date: pubDate,
    platforms: {
      'windows-x86_64': {
        url,
        signature,
      },
    },
  };
}

export function updaterReleasePaths(root, version) {
  const releaseRoot = path.join(root, 'release');
  const installerName = windowsInstallerName(version);
  const installerPath = path.join(releaseRoot, 'bundle', 'nsis', installerName);
  return {
    releaseRoot,
    installerName,
    installerPath,
    signaturePath: `${installerPath}.sig`,
    metadataPath: path.join(releaseRoot, updaterMetadataFilename),
  };
}

export async function writeLatestAlphaJson({
  root,
  repository = updaterRepository,
  releaseTag = updaterReleaseTag,
  notes = 'Alpha update',
  pubDate = new Date().toISOString(),
} = {}) {
  const packageJson = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
  const version = packageJson.version;
  const paths = updaterReleasePaths(root, version);
  const signature = (await readFile(paths.signaturePath, 'utf8')).trim();
  const metadata = createLatestAlphaJson({
    version,
    notes,
    pubDate,
    url: updaterAssetUrl(repository, releaseTag, paths.installerName),
    signature,
  });
  await writeFile(paths.metadataPath, `${JSON.stringify(metadata, null, 2)}\n`, 'utf8');
  return { metadata, paths };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const __filename = fileURLToPath(import.meta.url);
  const root = path.resolve(path.dirname(__filename), '..');
  const notes = process.env.SDM_UPDATER_NOTES || 'Alpha update';
  const { paths } = await writeLatestAlphaJson({ root, notes });
  console.log(`Updater metadata written to ${paths.metadataPath}`);
}
