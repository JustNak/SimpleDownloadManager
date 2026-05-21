import assert from 'node:assert/strict';

let publishUpdaterBeta;
let publishUpdaterAlphaBridge;
let updaterRelease;
try {
  publishUpdaterBeta = await import('../../../scripts/publish-updater-beta.mjs');
  publishUpdaterAlphaBridge = await import('../../../scripts/publish-updater-alpha-bridge.mjs');
  updaterRelease = await import('../../../scripts/updater-release.mjs');
} catch (error) {
  assert.fail(`Release publish helpers should be importable without running gh: ${error instanceof Error ? error.message : error}`);
}

const paths = updaterRelease.updaterReleasePaths('virtual-root', '0.8.3-beta', updaterRelease.releaseChannels.beta);

assert.deepEqual(
  publishUpdaterBeta.updaterBetaUploadPaths(paths).map((filePath) => filePath.replaceAll('\\', '/')),
  [
    'virtual-root/release/bundle/nsis/Simple Download Manager_0.8.3-beta_x64-setup.exe',
    'virtual-root/release/bundle/nsis/Simple Download Manager_0.8.3-beta_x64-setup.exe.sig',
    'virtual-root/release/bundle/nsis/Simple Download Manager_0.8.3-beta_arm64-setup.exe',
    'virtual-root/release/bundle/nsis/Simple Download Manager_0.8.3-beta_arm64-setup.exe.sig',
    'virtual-root/release/latest-beta.json',
  ],
  'beta publishing should upload both Windows installers, signatures, and beta metadata',
);

const alphaPaths = updaterRelease.updaterReleasePaths('virtual-root', '0.8.3-beta', updaterRelease.releaseChannels.alphaBridge);
assert.deepEqual(
  publishUpdaterAlphaBridge.updaterAlphaBridgeUploadPaths(alphaPaths).map((filePath) => filePath.replaceAll('\\', '/')),
  ['virtual-root/release/latest-alpha.json'],
  'alpha bridge publishing should upload only the bridge metadata',
);
