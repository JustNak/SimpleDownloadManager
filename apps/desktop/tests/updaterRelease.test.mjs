import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

let updaterRelease;
try {
  updaterRelease = await import('../../../scripts/updater-release.mjs');
} catch (error) {
  assert.fail(`Updater release helper should exist: ${error instanceof Error ? error.message : error}`);
}

const {
  createLatestAlphaJson,
  githubReleaseAssetName,
  requireSigningEnvironment,
  slintUpdaterMetadataFilename,
  updaterAssetUrl,
  updaterMetadataFilename,
  updaterReleaseTag,
} = updaterRelease;

assert.equal(updaterReleaseTag, 'updater-alpha');
assert.equal(updaterMetadataFilename, 'latest-alpha.json');
assert.equal(slintUpdaterMetadataFilename, 'latest-alpha-slint.json');

const installerName = 'Simple Download Manager_0.3.48-alpha_x64-setup.exe';
assert.equal(
  githubReleaseAssetName(installerName),
  'Simple.Download.Manager_0.3.48-alpha_x64-setup.exe',
);
assert.equal(
  updaterAssetUrl(
    'JustNak/SimpleDownloadManager',
    updaterReleaseTag,
    githubReleaseAssetName(installerName),
  ),
  'https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-alpha/Simple.Download.Manager_0.3.48-alpha_x64-setup.exe',
);

const latest = createLatestAlphaJson({
  version: '0.3.48-alpha',
  notes: 'Alpha update',
  pubDate: '2026-04-27T00:00:00.000Z',
  url: updaterAssetUrl(
    'JustNak/SimpleDownloadManager',
    updaterReleaseTag,
    githubReleaseAssetName(installerName),
  ),
  signature: 'signed-content',
});

assert.equal(latest.version, '0.3.48-alpha');
assert.equal(latest.notes, 'Alpha update');
assert.equal(latest.pub_date, '2026-04-27T00:00:00.000Z');
assert.deepEqual(Object.keys(latest.platforms), ['windows-x86_64']);
assert.equal(latest.platforms['windows-x86_64'].signature, 'signed-content');
assert.match(latest.platforms['windows-x86_64'].url, /Simple\.Download\.Manager_0\.3\.48-alpha_x64-setup\.exe$/);

assert.throws(
  () => requireSigningEnvironment({}),
  /TAURI_SIGNING_PRIVATE_KEY is required/,
  'release builds should fail clearly when updater signing is not configured',
);

const rootPackage = JSON.parse(await readFile('package.json', 'utf8'));
assert.equal(
  rootPackage.scripts['publish:updater-alpha'],
  'node ./scripts/publish-updater-alpha.mjs',
  'release tooling should expose a local GitHub updater publish command',
);
