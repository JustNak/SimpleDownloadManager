import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

let updaterRelease;
try {
  updaterRelease = await import('../../../scripts/updater-release.mjs');
} catch (error) {
  assert.fail(`Updater release helper should exist: ${error instanceof Error ? error.message : error}`);
}

const {
  createUpdaterMetadata,
  githubReleaseAssetName,
  requireSigningEnvironment,
  releaseChannels,
  updaterAssetUrl,
  writeReleaseUpdaterMetadata,
} = updaterRelease;

assert.equal(releaseChannels.beta.metadataReleaseTag, 'updater-beta');
assert.equal(releaseChannels.beta.assetReleaseTag, 'updater-beta');
assert.equal(releaseChannels.beta.metadataFilename, 'latest-beta.json');
assert.equal(releaseChannels.alphaBridge.metadataReleaseTag, 'updater-alpha');
assert.equal(releaseChannels.alphaBridge.assetReleaseTag, 'updater-beta');
assert.equal(releaseChannels.alphaBridge.metadataFilename, 'latest-alpha.json');

const installerName = 'Simple Download Manager_0.5.0-beta_x64-setup.exe';
assert.equal(
  githubReleaseAssetName(installerName),
  'Simple.Download.Manager_0.5.0-beta_x64-setup.exe',
);
assert.equal(
  updaterAssetUrl(
    'JustNak/SimpleDownloadManager',
    releaseChannels.beta.assetReleaseTag,
    githubReleaseAssetName(installerName),
  ),
  'https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-beta/Simple.Download.Manager_0.5.0-beta_x64-setup.exe',
);

const latest = createUpdaterMetadata({
  version: '0.5.0-beta',
  notes: 'Beta update',
  pubDate: '2026-04-27T00:00:00.000Z',
  url: updaterAssetUrl(
    'JustNak/SimpleDownloadManager',
    releaseChannels.beta.assetReleaseTag,
    githubReleaseAssetName(installerName),
  ),
  signature: 'signed-content',
});

assert.equal(latest.version, '0.5.0-beta');
assert.equal(latest.notes, 'Beta update');
assert.equal(latest.pub_date, '2026-04-27T00:00:00.000Z');
assert.deepEqual(Object.keys(latest.platforms), ['windows-x86_64']);
assert.equal(latest.platforms['windows-x86_64'].signature, 'signed-content');
assert.match(latest.platforms['windows-x86_64'].url, /releases\/download\/updater-beta\/Simple\.Download\.Manager_0\.5\.0-beta_x64-setup\.exe$/);

const alphaBridge = await writeReleaseUpdaterMetadata({
  root: 'virtual-root',
  channel: releaseChannels.alphaBridge,
  version: '0.5.0-beta',
  signature: 'signed-content',
  writeFile: async () => undefined,
  readFile: async () => '{"version":"0.5.0-beta"}',
  pubDate: '2026-04-27T00:00:00.000Z',
});

assert.match(alphaBridge.paths.metadataPath, /latest-alpha\.json$/, 'alpha bridge metadata should keep the alpha feed filename');
assert.match(
  alphaBridge.metadata.platforms['windows-x86_64'].url,
  /releases\/download\/updater-beta\/Simple\.Download\.Manager_0\.5\.0-beta_x64-setup\.exe$/,
  'alpha bridge should point alpha clients at the beta installer asset',
);
assert.equal(alphaBridge.metadata.notes, 'Beta migration update');

assert.throws(
  () => requireSigningEnvironment({}),
  /TAURI_SIGNING_PRIVATE_KEY is required/,
  'release builds should fail clearly when updater signing is not configured',
);

const rootPackage = JSON.parse(await readFile('package.json', 'utf8'));
assert.equal(
  rootPackage.scripts['publish:updater-beta'],
  'node ./scripts/publish-updater-beta.mjs',
  'release tooling should expose a local GitHub beta updater publish command',
);
assert.equal(
  rootPackage.scripts['publish:updater-alpha-bridge'],
  'node ./scripts/publish-updater-alpha-bridge.mjs',
  'release tooling should expose a one-time alpha bridge publish command',
);
