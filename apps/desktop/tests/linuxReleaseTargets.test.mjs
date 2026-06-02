import assert from 'node:assert/strict';
import path from 'node:path';

let linuxTargets;
try {
  linuxTargets = await import('../../../scripts/linux-release-targets.mjs');
} catch (error) {
  assert.fail(`Linux release target helper should exist: ${error instanceof Error ? error.message : error}`);
}

const {
  linuxAppImageName,
  linuxBundleArtifactNames,
  linuxReleaseTargetForName,
  linuxReleaseTargetForRustTarget,
  linuxReleaseTargetList,
  linuxReleaseTargets,
  resolveLinuxReleaseTargets,
  tauriLinuxTargetBundleDir,
} = linuxTargets;

assert.deepEqual(
  linuxReleaseTargetList.map((target) => target.name),
  ['x64'],
  'Linux release tooling should initially build x64 only',
);

assert.equal(linuxReleaseTargets.x64.rustTarget, 'x86_64-unknown-linux-gnu');
assert.equal(linuxReleaseTargets.x64.updaterPlatform, 'linux-x86_64');
assert.equal(linuxReleaseTargets.x64.appImageArch, 'amd64');
assert.equal(linuxReleaseTargets.x64.debArch, 'amd64');
assert.equal(linuxReleaseTargets.x64.rpmArch, 'x86_64');

assert.equal(
  linuxAppImageName('0.8.7-beta', linuxReleaseTargets.x64),
  'Simple Download Manager_0.8.7-beta_amd64.AppImage',
);

assert.deepEqual(
  linuxBundleArtifactNames('0.8.7-beta', linuxReleaseTargets.x64),
  {
    appimage: 'Simple Download Manager_0.8.7-beta_amd64.AppImage',
    deb: 'Simple Download Manager_0.8.7-beta_amd64.deb',
    rpm: 'Simple Download Manager-0.8.7-beta-1.x86_64.rpm',
  },
);

assert.equal(linuxReleaseTargetForName('amd64'), linuxReleaseTargets.x64);
assert.equal(linuxReleaseTargetForName('x86_64'), linuxReleaseTargets.x64);
assert.equal(linuxReleaseTargetForRustTarget('x86_64-unknown-linux-gnu'), linuxReleaseTargets.x64);
assert.deepEqual(resolveLinuxReleaseTargets('x64,amd64'), [linuxReleaseTargets.x64, linuxReleaseTargets.x64]);

assert.equal(
  tauriLinuxTargetBundleDir('virtual-root', linuxReleaseTargets.x64),
  path.join('virtual-root', 'apps', 'desktop', 'src-tauri', 'target', 'x86_64-unknown-linux-gnu', 'release', 'bundle'),
);

assert.throws(
  () => linuxReleaseTargetForName('arm64'),
  /Unsupported Linux release target/,
  'ARM64 should not be advertised until the Linux release pipeline builds and verifies it',
);
