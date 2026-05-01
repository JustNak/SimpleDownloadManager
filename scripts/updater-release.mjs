import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const updaterReleaseTag = 'updater-alpha';
export const updaterMetadataFilename = 'latest-alpha.json';
export const slintUpdaterMetadataFilename = 'latest-alpha-slint.json';
export const updaterRepository = 'JustNak/SimpleDownloadManager';

export function requireSigningEnvironment(env = process.env) {
  if (!env.TAURI_SIGNING_PRIVATE_KEY?.trim()) {
    throw new Error('TAURI_SIGNING_PRIVATE_KEY is required to build signed updater artifacts.');
  }
}

export function windowsInstallerName(version) {
  return `Simple Download Manager_${version}_x64-setup.exe`;
}

export function slintWindowsInstallerName(version) {
  return `simple-download-manager_${version}_x64-setup.exe`;
}

export function updaterAssetUrl(repository, releaseTag, assetName) {
  return `https://github.com/${repository}/releases/download/${releaseTag}/${encodeURIComponent(assetName)}`;
}

export function githubReleaseAssetName(assetName) {
  return assetName.replace(/\s+/g, '.');
}

export function createLatestAlphaJson({
  version,
  notes,
  pubDate,
  url,
  signature,
  format,
}) {
  const platform = {
    url,
    signature,
  };
  if (format) {
    platform.format = format;
  }

  return {
    version,
    notes,
    pub_date: pubDate,
    platforms: {
      'windows-x86_64': platform,
    },
  };
}

export function createSlintLatestAlphaJson(options) {
  return createLatestAlphaJson({
    ...options,
    format: 'nsis',
  });
}

export function updaterReleasePaths(root, version, {
  metadataFilename = updaterMetadataFilename,
  releaseSubdir = '',
  installerName = windowsInstallerName(version),
} = {}) {
  const releaseRoot = releaseSubdir
    ? path.join(root, 'release', releaseSubdir)
    : path.join(root, 'release');
  const installerPath = path.join(releaseRoot, 'bundle', 'nsis', installerName);
  return {
    releaseRoot,
    installerName,
    installerPath,
    signaturePath: `${installerPath}.sig`,
    metadataPath: path.join(releaseRoot, metadataFilename),
  };
}

export async function writeLatestAlphaJson({
  root,
  repository = updaterRepository,
  releaseTag = updaterReleaseTag,
  metadataFilename = updaterMetadataFilename,
  createMetadata = createLatestAlphaJson,
  installerNameForVersion = windowsInstallerName,
  releaseSubdir = '',
  notes = 'Alpha update',
  pubDate = new Date().toISOString(),
} = {}) {
  const packageJson = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
  const version = packageJson.version;
  const paths = updaterReleasePaths(root, version, {
    metadataFilename,
    releaseSubdir,
    installerName: installerNameForVersion(version),
  });
  const signature = (await readFile(paths.signaturePath, 'utf8')).trim();
  const metadata = createMetadata({
    version,
    notes,
    pubDate,
    url: updaterAssetUrl(repository, releaseTag, githubReleaseAssetName(paths.installerName)),
    signature,
  });
  await writeFile(paths.metadataPath, `${JSON.stringify(metadata, null, 2)}\n`, 'utf8');
  return { metadata, paths };
}

export function writeSlintLatestAlphaJson(options = {}) {
  return writeLatestAlphaJson({
    ...options,
    metadataFilename: slintUpdaterMetadataFilename,
    createMetadata: createSlintLatestAlphaJson,
    installerNameForVersion: slintWindowsInstallerName,
    releaseSubdir: options.releaseSubdir ?? 'slint',
  });
}

export function writeSlintTransitionLatestAlphaJson(options = {}) {
  return writeLatestAlphaJson({
    ...options,
    metadataFilename: updaterMetadataFilename,
    createMetadata: createLatestAlphaJson,
    installerNameForVersion: slintWindowsInstallerName,
    releaseSubdir: options.releaseSubdir ?? 'slint',
  });
}

export async function writeSlintReleaseFeeds(options = {}) {
  const transition = await writeSlintTransitionLatestAlphaJson(options);
  const native = await writeSlintLatestAlphaJson(options);
  return { transition, native };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const __filename = fileURLToPath(import.meta.url);
  const root = path.resolve(path.dirname(__filename), '..');
  const notes = process.env.SDM_UPDATER_NOTES || 'Alpha update';
  const isSlint = process.argv.includes('--slint');
  if (isSlint) {
    const { transition, native } = await writeSlintReleaseFeeds({ root, notes });
    console.log(`Updater metadata written to ${transition.paths.metadataPath}`);
    console.log(`Updater metadata written to ${native.paths.metadataPath}`);
  } else {
    const { paths } = await writeLatestAlphaJson({ root, notes });
    console.log(`Updater metadata written to ${paths.metadataPath}`);
  }
}
