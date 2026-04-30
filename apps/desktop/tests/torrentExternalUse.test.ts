import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const appSource = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8');
const backendSource = readFileSync(new URL('../src/backend.ts', import.meta.url), 'utf8');
const backendMockUrl = new URL('../src/backendMock.ts', import.meta.url);
const queueViewSource = readFileSync(new URL('../src/QueueView.tsx', import.meta.url), 'utf8');

assert.ok(
  existsSync(backendMockUrl),
  'browser-preview mock behavior should live in backendMock.ts',
);

const backendMockSource = readFileSync(backendMockUrl, 'utf8');

assert.match(
  backendSource,
  /export interface ExternalUseResult[\s\S]*pausedTorrent: boolean[\s\S]*autoReseedRetrySeconds\?: number/,
  'open and reveal commands should expose whether a torrent was paused plus automatic reseed timing',
);

assert.match(
  backendSource,
  /invokeCommand<ExternalUseResult>\('open_job_file'/,
  'openJobFile should return the backend external-use result',
);

assert.match(
  backendSource,
  /invokeCommand<ExternalUseResult>\('reveal_job_in_folder'/,
  'revealJobInFolder should return the backend external-use result',
);

assert.match(
  appSource,
  /result\.pausedTorrent[\s\S]*Torrent Paused[\s\S]*externalUseAutoReseedMessage\('file', retrySeconds\)/,
  'opening a seeding torrent should show a paused-torrent auto-reseed toast',
);

assert.match(
  appSource,
  /result\.pausedTorrent[\s\S]*Torrent Paused[\s\S]*externalUseAutoReseedMessage\('folder', retrySeconds\)/,
  'revealing a seeding torrent should show a paused-torrent auto-reseed toast',
);

assert.match(
  appSource,
  /Windows can use the \$\{target\}[\s\S]*reseed every 60s/,
  'external-use auto-reseed toast copy should mention the 60s retry cadence',
);

assert.match(
  backendMockSource,
  /return \{ pausedTorrent: true, autoReseedRetrySeconds: 60 \}/,
  'mock external use should mirror the backend auto-reseed retry timing',
);

assert.match(
  queueViewSource,
  /label="Open File" onClick=\{\(\) => onOpen\(job\.id\)\}/,
  'the context menu should keep routing Open File through the open handler',
);

assert.match(
  queueViewSource,
  /label="Open Folder" onClick=\{\(\) => onReveal\(job\.id\)\}/,
  'the context menu should keep routing Open Folder through the reveal handler',
);
