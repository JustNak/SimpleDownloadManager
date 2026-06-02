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
  linuxAppImageName,
  linuxReleaseTargetList,
  linuxReleaseTargets,
  requireSigningEnvironment,
  releaseChannels,
  updaterAssetUrl,
  updaterReleasePaths,
  writeReleaseUpdaterMetadata,
  windowsInstallerName,
  windowsReleaseTargetList,
  windowsReleaseTargets,
} = updaterRelease;

assert.equal(releaseChannels.beta.metadataReleaseTag, 'updater-beta');
assert.equal(releaseChannels.beta.assetReleaseTag, 'updater-beta');
assert.equal(releaseChannels.beta.metadataFilename, 'latest-beta.json');
assert.equal(releaseChannels.alphaBridge.metadataReleaseTag, 'updater-alpha');
assert.equal(releaseChannels.alphaBridge.assetReleaseTag, 'updater-beta');
assert.equal(releaseChannels.alphaBridge.metadataFilename, 'latest-alpha.json');

assert.deepEqual(
  windowsReleaseTargetList.map((target) => target.name),
  ['x64', 'arm64'],
  'release tooling should build x64 and ARM64 Windows targets by default',
);
assert.equal(windowsReleaseTargets.x64.rustTarget, 'x86_64-pc-windows-msvc');
assert.equal(windowsReleaseTargets.x64.updaterPlatform, 'windows-x86_64');
assert.equal(windowsReleaseTargets.arm64.rustTarget, 'aarch64-pc-windows-msvc');
assert.equal(windowsReleaseTargets.arm64.updaterPlatform, 'windows-aarch64');
assert.deepEqual(
  linuxReleaseTargetList.map((target) => target.name),
  ['x64'],
  'release tooling should expose Linux x64 target metadata',
);
assert.equal(linuxReleaseTargets.x64.rustTarget, 'x86_64-unknown-linux-gnu');
assert.equal(linuxReleaseTargets.x64.updaterPlatform, 'linux-x86_64');
assert.equal(
  windowsInstallerName('0.5.12-beta', windowsReleaseTargets.arm64),
  'Simple Download Manager_0.5.12-beta_arm64-setup.exe',
  'ARM64 installer names should use the arm64 artifact suffix',
);

const installerName = 'Simple Download Manager_0.5.12-beta_x64-setup.exe';
assert.equal(
  githubReleaseAssetName(installerName),
  'Simple.Download.Manager_0.5.12-beta_x64-setup.exe',
);
assert.equal(
  updaterAssetUrl(
    'JustNak/SimpleDownloadManager',
    releaseChannels.beta.assetReleaseTag,
    githubReleaseAssetName(installerName),
  ),
  'https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-beta/Simple.Download.Manager_0.5.12-beta_x64-setup.exe',
);

const latest = createUpdaterMetadata({
  version: '0.5.12-beta',
  notes: 'Beta update',
  pubDate: '2026-04-27T00:00:00.000Z',
  platformAssets: [
    {
      target: windowsReleaseTargets.x64,
      url: updaterAssetUrl(
        'JustNak/SimpleDownloadManager',
        releaseChannels.beta.assetReleaseTag,
        githubReleaseAssetName(installerName),
      ),
      signature: 'signed-content-x64',
    },
    {
      target: windowsReleaseTargets.arm64,
      url: updaterAssetUrl(
        'JustNak/SimpleDownloadManager',
        releaseChannels.beta.assetReleaseTag,
        githubReleaseAssetName('Simple Download Manager_0.5.12-beta_arm64-setup.exe'),
      ),
      signature: 'signed-content-arm64',
    },
  ],
});

assert.equal(latest.version, '0.5.12-beta');
assert.equal(latest.notes, 'Beta update');
assert.equal(latest.pub_date, '2026-04-27T00:00:00.000Z');
assert.deepEqual(Object.keys(latest.platforms), ['windows-x86_64', 'windows-aarch64']);
assert.equal(latest.platforms['windows-x86_64'].signature, 'signed-content-x64');
assert.equal(latest.platforms['windows-aarch64'].signature, 'signed-content-arm64');
assert.match(latest.platforms['windows-x86_64'].url, /releases\/download\/updater-beta\/Simple\.Download\.Manager_0\.5\.12-beta_x64-setup\.exe$/);
assert.match(latest.platforms['windows-aarch64'].url, /releases\/download\/updater-beta\/Simple\.Download\.Manager_0\.5\.12-beta_arm64-setup\.exe$/);

const alphaBridge = await writeReleaseUpdaterMetadata({
  root: 'virtual-root',
  channel: releaseChannels.alphaBridge,
  version: '0.5.12-beta',
  signatures: new Map([
    ['x64', 'signed-content-x64'],
    ['arm64', 'signed-content-arm64'],
  ]),
  writeFile: async () => undefined,
  readFile: async () => '{"version":"0.5.12-beta"}',
  pubDate: '2026-04-27T00:00:00.000Z',
});

assert.match(alphaBridge.paths.metadataPath, /latest-alpha\.json$/, 'alpha bridge metadata should keep the alpha feed filename');
assert.match(
  alphaBridge.metadata.platforms['windows-x86_64'].url,
  /releases\/download\/updater-beta\/Simple\.Download\.Manager_0\.5\.12-beta_x64-setup\.exe$/,
  'alpha bridge should point alpha clients at the beta installer asset',
);
assert.match(
  alphaBridge.metadata.platforms['windows-aarch64'].url,
  /releases\/download\/updater-beta\/Simple\.Download\.Manager_0\.5\.12-beta_arm64-setup\.exe$/,
  'alpha bridge should point ARM64 alpha clients at the beta ARM64 installer asset',
);
assert.equal(alphaBridge.metadata.notes, 'Beta migration update');

const x64Only = await writeReleaseUpdaterMetadata({
  root: 'virtual-root',
  version: '0.5.12-beta',
  targets: [windowsReleaseTargets.x64],
  signatures: new Map([['x64', 'signed-content-x64']]),
  writeFile: async () => undefined,
  readFile: async () => '{"version":"0.5.12-beta"}',
  pubDate: '2026-04-27T00:00:00.000Z',
});

assert.deepEqual(
  Object.keys(x64Only.metadata.platforms),
  ['windows-x86_64'],
  'single-target release metadata should not require unbuilt installers',
);

const releasePaths = updaterReleasePaths('virtual-root', '0.5.12-beta');
assert.deepEqual(
  releasePaths.installers.map((installer) => installer.target.name),
  ['x64', 'arm64'],
  'release paths should include both Windows installers by default',
);
assert.match(releasePaths.installers[0].installerPath, /_x64-setup\.exe$/);
assert.match(releasePaths.installers[1].installerPath, /_arm64-setup\.exe$/);

const mixedReleasePaths = updaterReleasePaths(
  'virtual-root',
  '0.8.7-beta',
  releaseChannels.beta,
  [windowsReleaseTargets.x64, linuxReleaseTargets.x64],
);

assert.deepEqual(
  mixedReleasePaths.installers.map((installer) => installer.target.updaterPlatform),
  ['windows-x86_64', 'linux-x86_64'],
  'release paths should support mixed Windows and Linux updater platforms',
);
assert.match(mixedReleasePaths.installers[1].installerPath, /bundle[/\\]appimage[/\\]Simple Download Manager_0\.8\.7-beta_amd64\.AppImage$/);
assert.match(mixedReleasePaths.installers[1].signaturePath, /\.AppImage\.sig$/);

const mixedMetadata = createUpdaterMetadata({
  version: '0.8.7-beta',
  notes: 'Beta update',
  pubDate: '2026-06-02T00:00:00.000Z',
  platformAssets: [
    {
      target: windowsReleaseTargets.x64,
      url: updaterAssetUrl(
        'JustNak/SimpleDownloadManager',
        releaseChannels.beta.assetReleaseTag,
        githubReleaseAssetName(windowsInstallerName('0.8.7-beta', windowsReleaseTargets.x64)),
      ),
      signature: 'signed-windows',
    },
    {
      target: linuxReleaseTargets.x64,
      url: updaterAssetUrl(
        'JustNak/SimpleDownloadManager',
        releaseChannels.beta.assetReleaseTag,
        githubReleaseAssetName(linuxAppImageName('0.8.7-beta', linuxReleaseTargets.x64)),
      ),
      signature: 'signed-linux',
    },
  ],
});

assert.deepEqual(Object.keys(mixedMetadata.platforms), ['windows-x86_64', 'linux-x86_64']);
assert.match(
  mixedMetadata.platforms['linux-x86_64'].url,
  /releases\/download\/updater-beta\/Simple\.Download\.Manager_0\.8\.7-beta_amd64\.AppImage$/,
);
assert.equal(mixedMetadata.platforms['linux-x86_64'].signature, 'signed-linux');

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
