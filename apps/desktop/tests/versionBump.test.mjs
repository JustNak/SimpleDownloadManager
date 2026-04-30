import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const expectedVersion = '0.3.52-alpha';
const expectedProtocolVersion = '0.3.48-alpha';
const expectedNativeHostVersion = '0.3.48-alpha';
const expectedExtensionVersion = '0.3.47-alpha';

for (const manifestPath of [
  'package.json',
  'apps/desktop/package.json',
]) {
  const manifest = JSON.parse(await readFile(path.join(repoRoot, manifestPath), 'utf8'));
  assert.equal(manifest.version, expectedVersion, `${manifestPath} should be bumped to ${expectedVersion}`);
}

const protocolManifest = JSON.parse(await readFile(path.join(repoRoot, 'packages/protocol/package.json'), 'utf8'));
assert.equal(
  protocolManifest.version,
  expectedProtocolVersion,
  'protocol package version should remain unchanged for this desktop-only release bump',
);

const extensionManifest = JSON.parse(await readFile(path.join(repoRoot, 'apps/extension/package.json'), 'utf8'));
assert.equal(
  extensionManifest.version,
  expectedExtensionVersion,
  'extension package version should remain on the extension release version',
);

const tauriConfig = JSON.parse(await readFile(path.join(repoRoot, 'apps', 'desktop', 'src-tauri', 'tauri.conf.json'), 'utf8'));
assert.equal(tauriConfig.version, expectedVersion, 'Tauri config should be bumped to the release version');

const desktopCargoManifest = await readFile(path.join(repoRoot, 'apps/desktop/src-tauri/Cargo.toml'), 'utf8');
assert.match(
  desktopCargoManifest,
  new RegExp(`version = "${expectedVersion}"`),
  `apps/desktop/src-tauri/Cargo.toml should be bumped to ${expectedVersion}`,
);

const nativeHostCargoManifest = await readFile(path.join(repoRoot, 'apps/native-host/Cargo.toml'), 'utf8');
assert.match(
  nativeHostCargoManifest,
  new RegExp(`version = "${expectedNativeHostVersion}"`),
  'native host package version should remain unchanged for this desktop UI release',
);
