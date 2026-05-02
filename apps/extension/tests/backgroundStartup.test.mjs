import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backgroundSource = await readFile(path.join(repoRoot, 'apps/extension/src/background/index.ts'), 'utf8');

assert.doesNotMatch(
  backgroundSource,
  /\nvoid refreshConnectionState\(\);\s*\nregisterHandoffAuthHeaderCapture\(\);/,
  'background startup should register listeners without pinging the native host unconditionally',
);
assert.match(
  backgroundSource,
  /let refreshConnectionStatePromise:/,
  'connection refreshes should share an in-flight promise instead of double-pinging the native host',
);

