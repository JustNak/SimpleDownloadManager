import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const rootManifest = await readJson('package.json');
const packageLock = await readJson('package-lock.json');
const desktopVersion = rootManifest.version;

assert.equal(packageLock.version, desktopVersion, 'package-lock root version should match package.json');
assert.equal(packageLock.packages[''].version, desktopVersion, 'package-lock package entry should match package.json');

const desktopManifest = await readJson('apps/desktop/package.json');
assert.equal(desktopManifest.version, desktopVersion, 'desktop package version should match package.json');
assert.equal(
  packageLock.packages['apps/desktop'].version,
  desktopVersion,
  'package-lock desktop package entry should match package.json',
);

const tauriConfig = await readJson('apps/desktop/src-tauri/tauri.conf.json');
assert.equal(tauriConfig.version, desktopVersion, 'Tauri config version should match package.json');
assert.deepEqual(
  tauriConfig.plugins.updater.endpoints,
  ['https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-beta/latest-beta.json'],
  'desktop updater endpoint should track the beta feed',
);

const desktopCargoManifest = await readText('apps/desktop/src-tauri/Cargo.toml');
assert.equal(
  cargoPackageVersion(desktopCargoManifest, 'simple-download-manager-desktop-backend'),
  desktopVersion,
  'desktop Cargo manifest version should match package.json',
);

const desktopCargoLock = await readText('apps/desktop/src-tauri/Cargo.lock');
assert.equal(
  cargoLockPackageVersion(desktopCargoLock, 'simple-download-manager-desktop-backend'),
  desktopVersion,
  'desktop Cargo lock package entry should match package.json',
);

const protocolManifest = await readJson('packages/protocol/package.json');
assert.equal(
  packageLock.packages['packages/protocol'].version,
  protocolManifest.version,
  'package-lock protocol package entry should match protocol package.json',
);

const extensionManifest = await readJson('apps/extension/package.json');
assert.equal(
  packageLock.packages['apps/extension'].version,
  extensionManifest.version,
  'package-lock extension package entry should match extension package.json',
);

const nativeHostCargoManifest = await readText('apps/native-host/Cargo.toml');
const nativeHostVersion = cargoPackageVersion(nativeHostCargoManifest, 'simple-download-manager-native-host');
const nativeHostCargoLock = await readText('apps/native-host/Cargo.lock');
assert.equal(
  cargoLockPackageVersion(nativeHostCargoLock, 'simple-download-manager-native-host'),
  nativeHostVersion,
  'native host Cargo lock package entry should match native host Cargo manifest',
);

async function readJson(relativePath) {
  return JSON.parse(await readText(relativePath));
}

async function readText(relativePath) {
  return readFile(path.join(repoRoot, relativePath), 'utf8');
}

function cargoPackageVersion(manifest, expectedName) {
  const packageSection = manifest.match(/(?:^|\r?\n)\[package\]\r?\n([\s\S]*?)(?=\r?\n\[|$)/);
  assert(packageSection, `Cargo manifest should contain [package] for ${expectedName}`);
  assert.equal(
    tomlStringValue(packageSection[1], 'name'),
    expectedName,
    `Cargo manifest package name should be ${expectedName}`,
  );
  return tomlStringValue(packageSection[1], 'version');
}

function cargoLockPackageVersion(lockfile, expectedName) {
  for (const section of lockfile.split(/\r?\n(?=\[\[package\]\])/)) {
    if (!section.startsWith('[[package]]')) {
      continue;
    }
    if (tomlStringValue(section, 'name') === expectedName) {
      return tomlStringValue(section, 'version');
    }
  }

  assert.fail(`Cargo lock should contain package ${expectedName}`);
}

function tomlStringValue(section, key) {
  const match = section.match(new RegExp(`^${key} = "([^"]+)"`, 'm'));
  assert(match, `TOML section should contain string key ${key}`);
  return match[1];
}
