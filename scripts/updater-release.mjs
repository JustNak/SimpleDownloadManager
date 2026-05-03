import { readFile as readFileFromDisk, writeFile as writeFileToDisk } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const updaterRepository = 'JustNak/SimpleDownloadManager';

export const releaseChannels = Object.freeze({
  beta: Object.freeze({
    name: 'beta',
    metadataReleaseTag: 'updater-beta',
    assetReleaseTag: 'updater-beta',
    metadataFilename: 'latest-beta.json',
    notes: 'Beta update',
    releaseTitle: 'Simple Download Manager Beta Updates',
    releaseNotes: 'Static updater feed for beta builds.',
  }),
  alphaBridge: Object.freeze({
    name: 'alphaBridge',
    metadataReleaseTag: 'updater-alpha',
    assetReleaseTag: 'updater-beta',
    metadataFilename: 'latest-alpha.json',
    notes: 'Beta migration update',
    releaseTitle: 'Simple Download Manager Alpha Updates',
    releaseNotes: 'Static updater feed for alpha-to-beta migration.',
  }),
});

export const updaterReleaseTag = releaseChannels.beta.metadataReleaseTag;
export const updaterMetadataFilename = releaseChannels.beta.metadataFilename;

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

export function githubReleaseAssetName(assetName) {
  return assetName.replace(/\s+/g, '.');
}

export function createUpdaterMetadata({
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

export const createLatestAlphaJson = createUpdaterMetadata;

export function updaterReleasePaths(root, version, channel = releaseChannels.beta) {
  const releaseRoot = path.join(root, 'release');
  const installerName = windowsInstallerName(version);
  const installerPath = path.join(releaseRoot, 'bundle', 'nsis', installerName);
  return {
    releaseRoot,
    installerName,
    installerPath,
    signaturePath: `${installerPath}.sig`,
    metadataPath: path.join(releaseRoot, channel.metadataFilename),
  };
}

export async function writeReleaseUpdaterMetadata({
  root,
  channel = releaseChannels.beta,
  repository = updaterRepository,
  notes = channel.notes,
  pubDate = new Date().toISOString(),
  version,
  signature,
  readFile = readFileFromDisk,
  writeFile = writeFileToDisk,
} = {}) {
  const resolvedVersion = version ?? JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8')).version;
  const paths = updaterReleasePaths(root, resolvedVersion, channel);
  const resolvedSignature = signature ?? (await readFile(paths.signaturePath, 'utf8')).trim();
  const metadata = createUpdaterMetadata({
    version: resolvedVersion,
    notes,
    pubDate,
    url: updaterAssetUrl(repository, channel.assetReleaseTag, githubReleaseAssetName(paths.installerName)),
    signature: resolvedSignature,
  });
  await writeFile(paths.metadataPath, `${JSON.stringify(metadata, null, 2)}\n`, 'utf8');
  return { metadata, paths };
}

export async function writeAllReleaseUpdaterMetadata({
  root,
  repository = updaterRepository,
  pubDate = new Date().toISOString(),
  betaNotes = process.env.SDM_UPDATER_NOTES || releaseChannels.beta.notes,
  alphaBridgeNotes = process.env.SDM_ALPHA_BRIDGE_UPDATER_NOTES || releaseChannels.alphaBridge.notes,
} = {}) {
  const beta = await writeReleaseUpdaterMetadata({
    root,
    repository,
    channel: releaseChannels.beta,
    notes: betaNotes,
    pubDate,
  });
  const alphaBridge = await writeReleaseUpdaterMetadata({
    root,
    repository,
    channel: releaseChannels.alphaBridge,
    notes: alphaBridgeNotes,
    pubDate,
  });
  return { beta, alphaBridge };
}

export async function writeLatestAlphaJson(options = {}) {
  return writeReleaseUpdaterMetadata({
    ...options,
    channel: releaseChannels.alphaBridge,
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const __filename = fileURLToPath(import.meta.url);
  const root = path.resolve(path.dirname(__filename), '..');
  const result = await writeAllReleaseUpdaterMetadata({ root });
  console.log(`Beta updater metadata written to ${result.beta.paths.metadataPath}`);
  console.log(`Alpha bridge updater metadata written to ${result.alphaBridge.paths.metadataPath}`);
}
