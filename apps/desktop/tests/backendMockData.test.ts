import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';

const backendSource = await readFile(new URL('../src/backend.ts', import.meta.url), 'utf8');
const backendMockUrl = new URL('../src/backendMock.ts', import.meta.url);

assert.ok(
  existsSync(backendMockUrl),
  'browser-preview mock behavior should live in backendMock.ts',
);

const backendMockSource = await readFile(backendMockUrl, 'utf8');

assert.match(
  backendSource,
  /type BackendMockModule = typeof import\('\.\/backendMock'\)/,
  'production backend bridge should lazy-load browser-preview mock behavior',
);

assert.doesNotMatch(
  backendSource,
  /let\s+mockState\s*:/,
  'production backend bridge should not allocate browser-preview mock state at module load',
);

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

assert.match(
  backendMockSource,
  /export async function clearTorrentSessionCache\(\)/,
  'browser-preview mock should keep torrent session cache cleanup support',
);

assert.match(
  backendMockSource,
  /duplicatePath:\s*undefined[\s\S]*duplicateFilename:\s*undefined[\s\S]*duplicateReason:\s*undefined/,
  'browser-preview mock prompt should keep duplicate path metadata fields',
);

assert.match(
  backendMockSource,
  /\?window=torrent-progress&jobId=\$\{encodeURIComponent\(id\)\}[\s\S]*torrent-progress-\$\{id\}[\s\S]*width=720,height=520/,
  'browser-preview mock should route torrent progress popups to the dedicated larger window',
);
