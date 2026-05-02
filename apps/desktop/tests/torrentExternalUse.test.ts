import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const appSource = readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8');
const backendSource = readFileSync(new URL('../src/backend.ts', import.meta.url), 'utf8');
const queueViewSource = readFileSync(new URL('../src/QueueView.svelte', import.meta.url), 'utf8');

assert.match(backendSource, /export interface ExternalUseResult[\s\S]*pausedTorrent: boolean[\s\S]*autoReseedRetrySeconds\?: number/, 'open and reveal commands should expose whether a torrent was paused plus automatic reseed timing');
assert.match(backendSource, /invokeCommand<ExternalUseResult>\('open_job_file'/, 'openJobFile should return the backend external-use result');
assert.match(backendSource, /invokeCommand<ExternalUseResult>\('reveal_job_in_folder'/, 'revealJobInFolder should return the backend external-use result');
assert.match(appSource, /result\.pausedTorrent[\s\S]*Torrent Paused[\s\S]*externalUseAutoReseedMessage\('file', result\.autoReseedRetrySeconds \?\? 60\)/, 'opening a seeding torrent should show a paused-torrent auto-reseed toast');
assert.match(appSource, /result\.pausedTorrent[\s\S]*Torrent Paused[\s\S]*externalUseAutoReseedMessage\('folder', result\.autoReseedRetrySeconds \?\? 60\)/, 'revealing a seeding torrent should show a paused-torrent auto-reseed toast');
assert.match(appSource, /Windows can use the \$\{target\}[\s\S]*reseed every 60s/, 'external-use auto-reseed toast copy should mention the 60s retry cadence');
assert.match(backendSource, /return \{ pausedTorrent: true, autoReseedRetrySeconds: 60 \}/, 'mock external use should mirror the backend auto-reseed retry timing');
assert.match(queueViewSource, /Open'[\s\S]*onOpen\(job\.id\)/, 'the context menu should keep routing Open through the open handler');
assert.match(queueViewSource, /Open Folder'[\s\S]*onReveal\(job\.id\)/, 'the context menu should keep routing Open Folder through the reveal handler');
