import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const expectedVersion = '0.3.46-alpha';
const expectedExtensionVersion = '0.3.46-alpha';

for (const manifestPath of [
  'package.json',
  'apps/desktop/package.json',
  'packages/protocol/package.json',
]) {
  const manifest = JSON.parse(await readFile(path.join(repoRoot, manifestPath), 'utf8'));
  assert.equal(manifest.version, expectedVersion, `${manifestPath} should be bumped to ${expectedVersion}`);
}

const extensionManifest = JSON.parse(await readFile(path.join(repoRoot, 'apps/extension/package.json'), 'utf8'));
assert.equal(
  extensionManifest.version,
  expectedExtensionVersion,
  'extension package version should remain on the extension release version',
);

const tauriConfig = JSON.parse(await readFile(path.join(repoRoot, 'apps', 'desktop', 'src-tauri', 'tauri.conf.json'), 'utf8'));
assert.equal(tauriConfig.version, expectedVersion, 'Tauri config should be bumped to the release version');

for (const cargoPath of [
  'apps/desktop/src-tauri/Cargo.toml',
  'apps/native-host/Cargo.toml',
]) {
  const cargoManifest = await readFile(path.join(repoRoot, cargoPath), 'utf8');
  assert.match(cargoManifest, new RegExp(`version = "${expectedVersion}"`), `${cargoPath} should be bumped to ${expectedVersion}`);
}
