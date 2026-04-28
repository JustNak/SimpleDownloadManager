import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const typesSource = readFileSync(new URL('../src/types.ts', import.meta.url), 'utf8');

assert.match(
  typesSource,
  /lastRuntimeUploadedBytes\?: number/,
  'torrent metadata should expose the optional runtime upload baseline',
);
