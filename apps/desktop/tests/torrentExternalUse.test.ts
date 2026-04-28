import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const appSource = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8');
const backendSource = readFileSync(new URL('../src/backend.ts', import.meta.url), 'utf8');
const queueViewSource = readFileSync(new URL('../src/QueueView.tsx', import.meta.url), 'utf8');

assert.match(
  backendSource,
  /export interface ExternalUseResult[\s\S]*pausedTorrent: boolean/,
  'open and reveal commands should expose whether a torrent was paused for external file use',
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
  /result\.pausedTorrent[\s\S]*Torrent Paused[\s\S]*Windows can use the file/,
  'opening a seeding torrent should show a paused-torrent toast',
);

assert.match(
  appSource,
  /result\.pausedTorrent[\s\S]*Torrent Paused[\s\S]*Windows can use the folder/,
  'revealing a seeding torrent should show a paused-torrent toast',
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
