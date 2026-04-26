import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const backendSource = await readFile(new URL('../src/backend.ts', import.meta.url), 'utf8');

assert.match(
  backendSource,
  /transferKind:\s*'torrent'/,
  'browser-preview mock data should include at least one torrent row',
);

assert.match(
  backendSource,
  /state:\s*JobState\.Seeding/,
  'browser-preview mock data should include a seeding torrent row',
);
