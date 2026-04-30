import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const coreLib = await readFile('apps/desktop-core/src/lib.rs', 'utf8');
const coreManifest = await readFile('apps/desktop-core/Cargo.toml', 'utf8');

assert.doesNotMatch(
  coreLib,
  /src-tauri/,
  'desktop-core modules should be physically owned by desktop-core, not path-included from Tauri',
);

for (const forbiddenDependency of [
  'tauri',
  'tauri-plugin',
  'rfd',
  'winreg',
  'windows-sys',
  'reqwest',
  'librqbit',
]) {
  assert.doesNotMatch(
    coreManifest,
    new RegExp(`^${forbiddenDependency}\\b`, 'm'),
    `desktop-core should not depend on ${forbiddenDependency}`,
  );
}
