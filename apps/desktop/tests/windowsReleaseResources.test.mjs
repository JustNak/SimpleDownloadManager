import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();

assert.equal(
  await peMachine(path.join(repoRoot, 'apps/desktop/src-tauri/resources/bin/7z.exe')),
  'x64',
  'default bundled 7z.exe should remain x64 for x64 releases and local dev',
);
assert.equal(
  await peMachine(path.join(repoRoot, 'apps/desktop/src-tauri/resources/bin/7z.dll')),
  'x64',
  'default bundled 7z.dll should remain x64 for x64 releases and local dev',
);
assert.equal(
  await peMachine(path.join(repoRoot, 'apps/desktop/src-tauri/resources/bin/windows-arm64/7z.exe')),
  'arm64',
  'ARM64 release resources should include an ARM64 7z.exe',
);
assert.equal(
  await peMachine(path.join(repoRoot, 'apps/desktop/src-tauri/resources/bin/windows-arm64/7z.dll')),
  'arm64',
  'ARM64 release resources should include an ARM64 7z.dll',
);

async function peMachine(filePath) {
  const bytes = await readFile(filePath);
  const peOffset = bytes.readInt32LE(0x3c);
  const machine = bytes.readUInt16LE(peOffset + 4);
  if (machine === 0x8664) return 'x64';
  if (machine === 0xaa64) return 'arm64';
  if (machine === 0x14c) return 'x86';
  return `0x${machine.toString(16).padStart(4, '0')}`;
}
