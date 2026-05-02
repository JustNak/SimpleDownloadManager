import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const torrentProgressUrl = new URL('../src/TorrentProgressWindow.svelte', import.meta.url);
const sharedPopupUrl = new URL('../src/useProgressPopup.svelte.ts', import.meta.url);

assert.ok(existsSync(sharedPopupUrl), 'progress popup lifecycle should be extracted into useProgressPopup.svelte.ts');
assert.ok(existsSync(torrentProgressUrl), 'torrent progress UI should live in a dedicated TorrentProgressWindow.svelte file');

const torrentSource = readFileSync(torrentProgressUrl, 'utf8');
const mainSource = readFileSync(new URL('../src/main.ts', import.meta.url), 'utf8');
const backendSource = readFileSync(new URL('../src/backend.ts', import.meta.url), 'utf8');
const windowsSource = readFileSync(new URL('../src-tauri/src/windows.rs', import.meta.url), 'utf8');
const capabilitySource = readFileSync(new URL('../src-tauri/capabilities/default.json', import.meta.url), 'utf8');

assert.match(mainSource, /windowMode === 'torrent-progress'[\s\S]*import\('\.\/TorrentProgressWindow\.svelte'\)/, 'main route should lazily mount TorrentProgressWindow for ?window=torrent-progress');
assert.match(backendSource, /job\.transferKind === 'torrent'[\s\S]*\?window=torrent-progress&jobId=\$\{encodeURIComponent\(id\)\}[\s\S]*torrent-progress-\$\{id\}[\s\S]*width=720,height=520/, 'browser fallback openProgressWindow should route torrent jobs to a 720x520 torrent popup');
assert.match(windowsSource, /const TORRENT_PROGRESS_WINDOW_PREFIX: &str = "torrent-progress-";[\s\S]*show_torrent_progress_window[\s\S]*index\.html\?window=torrent-progress&jobId=\{job_id\}[\s\S]*torrent_progress_window_geometry/, 'native windows should define a dedicated torrent progress popup route and prefix');
assert.match(windowsSource, /fn torrent_progress_window_geometry\(\) -> ProgressWindowGeometry \{[\s\S]*width:\s*720\.0,[\s\S]*height:\s*520\.0,[\s\S]*min_width:\s*720\.0,[\s\S]*min_height:\s*520\.0,/, 'torrent progress native geometry should match the approved larger popup size');
assert.match(capabilitySource, /"torrent-progress-\*"/, 'Tauri capabilities should allow the torrent progress popup label');

for (const label of ['Torrent session', 'Info hash', 'Downloaded', 'Uploaded', 'ETA', 'peers', 'seeds', 'Ratio', 'Files', 'Target', 'Source']) {
  assert.match(torrentSource, new RegExp(label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')), `torrent progress window should render ${label}`);
}

assert.match(torrentSource, /Pause[\s\S]*pauseJob\(job\.id\)/, 'active torrent actions should keep Pause behavior');
assert.match(torrentSource, /Resume[\s\S]*resumeJob\(job\.id\)/, 'paused torrent actions should keep Resume behavior');
assert.match(torrentSource, /Retry[\s\S]*retryJob\(job\.id\)/, 'failed torrent actions should keep Retry behavior');
assert.match(torrentSource, /Open[\s\S]*openJobFile\(job\.id\)/, 'completed torrent actions should keep Open behavior');
assert.match(torrentSource, /Confirm cancel[\s\S]*popup\.onCancelClick/, 'torrent popup should keep two-click Cancel behavior');
assert.doesNotMatch(torrentSource, /<span className="text-muted-foreground">[>>]<\/span>/, 'torrent detail rows should not render decorative chevrons');
assert.doesNotMatch(torrentSource, /#[0-9a-fA-F]{3,8}|teal|cyan|emerald/, 'torrent popup should use existing theme tokens instead of hardcoded custom colors');
