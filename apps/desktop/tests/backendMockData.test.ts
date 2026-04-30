import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const backendMockSource = await readFile(new URL('../src/backendMock.ts', import.meta.url), 'utf8');

assert.match(
  backendMockSource,
  /transferKind:\s*'torrent'/,
  'browser-preview mock data should include at least one torrent row',
);

assert.match(
  backendMockSource,
  /state:\s*JobState\.Seeding/,
  'browser-preview mock data should include a seeding torrent row',
);
