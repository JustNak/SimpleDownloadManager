import { readFile as readFileFromDisk, writeFile as writeFileToDisk } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import {
  resolveWindowsReleaseTargets,
  windowsInstallerName,
  windowsReleaseTargetList,
  windowsReleaseTargets,
} from './windows-release-targets.mjs';

export const updaterRepository = 'JustNak/SimpleDownloadManager';
export {
  windowsInstallerName,
  windowsReleaseTargetList,
  windowsReleaseTargets,
};

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
  platformAssets,
}) {
  const assets = platformAssets ?? [{
    target: windowsReleaseTargets.x64,
    url,
    signature,
  }];

  return {
    version,
    notes,
    pub_date: pubDate,
    platforms: Object.fromEntries(
      assets.map((asset) => [
        asset.target.updaterPlatform,
        {
          url: asset.url,
          signature: asset.signature,
        },
      ]),
    ),
  };
}

export const createLatestAlphaJson = createUpdaterMetadata;

export function updaterReleasePaths(
  root,
  version,
  channel = releaseChannels.beta,
  targets = windowsReleaseTargetList,
) {
  const releaseRoot = path.join(root, 'release');
  const installers = targets.map((target) => {
    const installerName = windowsInstallerName(version, target);
    const installerPath = path.join(releaseRoot, 'bundle', 'nsis', installerName);
    return {
      target,
      installerName,
      installerPath,
      signaturePath: `${installerPath}.sig`,
    };
  });
  const [defaultInstaller] = installers;
  return {
    releaseRoot,
    installers,
    installerName: defaultInstaller.installerName,
    installerPath: defaultInstaller.installerPath,
    signaturePath: defaultInstaller.signaturePath,
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
  signatures,
  targets = windowsReleaseTargetList,
  readFile = readFileFromDisk,
  writeFile = writeFileToDisk,
} = {}) {
  const resolvedVersion = version ?? JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8')).version;
  const paths = updaterReleasePaths(root, resolvedVersion, channel, targets);
  const platformAssets = await Promise.all(paths.installers.map(async (installer) => ({
    target: installer.target,
    url: updaterAssetUrl(
      repository,
      channel.assetReleaseTag,
      githubReleaseAssetName(installer.installerName),
    ),
    signature: await resolveInstallerSignature({
      installer,
      signature,
      signatures,
      readFile,
    }),
  })));
  const metadata = createUpdaterMetadata({
    version: resolvedVersion,
    notes,
    pubDate,
    platformAssets,
  });
  await writeFile(paths.metadataPath, `${JSON.stringify(metadata, null, 2)}\n`, 'utf8');
  return { metadata, paths };
}

async function resolveInstallerSignature({
  installer,
  signature,
  signatures,
  readFile,
}) {
  if (signatures instanceof Map) {
    const mapped = signatures.get(installer.target.name)
      ?? signatures.get(installer.target.rustTarget)
      ?? signatures.get(installer.target.updaterPlatform);
    if (mapped) return mapped;
  } else if (signatures && typeof signatures === 'object') {
    const mapped = signatures[installer.target.name]
      ?? signatures[installer.target.rustTarget]
      ?? signatures[installer.target.updaterPlatform];
    if (mapped) return mapped;
  }

  if (signature && installer.target === windowsReleaseTargets.x64) {
    return signature;
  }

  return (await readFile(installer.signaturePath, 'utf8')).trim();
}

export async function writeAllReleaseUpdaterMetadata({
  root,
  repository = updaterRepository,
  pubDate = new Date().toISOString(),
  betaNotes = process.env.SDM_UPDATER_NOTES || releaseChannels.beta.notes,
  alphaBridgeNotes = process.env.SDM_ALPHA_BRIDGE_UPDATER_NOTES || releaseChannels.alphaBridge.notes,
  targets = windowsReleaseTargetList,
} = {}) {
  const beta = await writeReleaseUpdaterMetadata({
    root,
    repository,
    channel: releaseChannels.beta,
    notes: betaNotes,
    pubDate,
    targets,
  });
  const alphaBridge = await writeReleaseUpdaterMetadata({
    root,
    repository,
    channel: releaseChannels.alphaBridge,
    notes: alphaBridgeNotes,
    pubDate,
    targets,
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
  const targetsArgIndex = process.argv.findIndex((arg) => arg === '--targets');
  const targets = targetsArgIndex >= 0
    ? resolveWindowsReleaseTargets(process.argv[targetsArgIndex + 1])
    : windowsReleaseTargetList;
  const result = await writeAllReleaseUpdaterMetadata({ root, targets });
  console.log(`Beta updater metadata written to ${result.beta.paths.metadataPath}`);
  console.log(`Alpha bridge updater metadata written to ${result.alphaBridge.paths.metadataPath}`);
}
