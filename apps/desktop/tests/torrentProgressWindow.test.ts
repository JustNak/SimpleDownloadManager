import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const torrentProgressUrl = new URL('../src/TorrentProgressWindow.svelte', import.meta.url);
const sharedPopupUrl = new URL('../src/useProgressPopup.svelte.ts', import.meta.url);

assert.ok(existsSync(sharedPopupUrl), 'progress popup lifecycle should be extracted into useProgressPopup.svelte.ts');
assert.ok(existsSync(torrentProgressUrl), 'torrent progress UI should live in a dedicated TorrentProgressWindow.svelte file');

const torrentSource = readFileSync(torrentProgressUrl, 'utf8');
const sharedPopupSource = readFileSync(sharedPopupUrl, 'utf8');
const mainSource = readFileSync(new URL('../src/main.ts', import.meta.url), 'utf8');
const backendPreviewSource = readFileSync(new URL('../src/backendPreview.ts', import.meta.url), 'utf8');
const windowsSource = readFileSync(new URL('../src-tauri/src/windows.rs', import.meta.url), 'utf8');
const capabilitySource = readFileSync(new URL('../src-tauri/capabilities/popups.json', import.meta.url), 'utf8');

assert.match(mainSource, /windowMode === 'torrent-progress'[\s\S]*import\('\.\/TorrentProgressWindow\.svelte'\)/, 'main route should lazily mount TorrentProgressWindow for ?window=torrent-progress');
assert.match(backendPreviewSource, /job\.transferKind === 'torrent'[\s\S]*\?window=torrent-progress&jobId=\$\{encodeURIComponent\(id\)\}[\s\S]*torrent-progress-\$\{id\}[\s\S]*width=720,height=520/, 'browser fallback openProgressWindow should route torrent jobs to a 720x520 torrent popup');
assert.match(windowsSource, /const TORRENT_PROGRESS_WINDOW_PREFIX: &str = "torrent-progress-";[\s\S]*show_torrent_progress_window[\s\S]*index\.html\?window=torrent-progress&jobId=\{job_id\}[\s\S]*torrent_progress_window_geometry/, 'native windows should define a dedicated torrent progress popup route and prefix');
assert.match(windowsSource, /fn torrent_progress_window_geometry\(\) -> PopupWindowGeometry \{[\s\S]*width:\s*720\.0,[\s\S]*height:\s*520\.0,[\s\S]*min_width:\s*720\.0,[\s\S]*min_height:\s*520\.0,/, 'torrent progress native geometry should match the approved larger popup size');
assert.match(capabilitySource, /"torrent-progress-\*"/, 'Tauri capabilities should allow the torrent progress popup label');

for (const label of ['Torrent session', 'Info hash', 'Down', 'Up', 'ETA', 'Peers', 'Seeds', 'Ratio', 'Files', 'Save to', 'Source']) {
  assert.match(torrentSource, new RegExp(label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')), `torrent progress window should render ${label}`);
}

assert.match(torrentSource, /h-\[72px\]\s+w-\[72px\]/, 'torrent popup should keep the Svelte 72px magnet badge');
assert.match(torrentSource, /grid\s+h-3\.5\s+grid-cols-\[repeat\(42,minmax\(0,1fr\)\)\]\s+gap-1/, 'torrent popup should keep the segmented Svelte progress strip');
for (const label of ['Down', 'Up', 'ETA', 'Peers', 'Seeds', 'Ratio']) {
  assert.match(torrentSource, new RegExp(`label="${label}"|['"]${label}['"]`), `torrent popup should keep the dense ${label} metric`);
}
assert.match(torrentSource, /Peer health/, 'torrent popup should render the peer health detail row');
assert.match(torrentSource, /label="Save to"|['"]Save to['"]/, 'torrent popup should render the save destination detail row');
assert.match(torrentSource, /border-primary\s+bg-background\s+text-primary\s+hover:bg-primary-soft/, 'torrent primary actions should use the Svelte outlined primary palette');
assert.match(torrentSource, /type TorrentActionVariant = 'default' \| 'primary' \| 'cancel' \| 'confirm'/, 'torrent progress actions should use explicit variants for cancel confirmation states');
assert.match(torrentSource, /cancelActionVariant\(popup\.isConfirmingCancel\)/, 'torrent cancel button should derive its style from the confirmation state');
assert.match(torrentSource, /case 'cancel':[\s\S]*border-destructive bg-destructive text-destructive-foreground[\s\S]*cursor-pointer/, 'torrent Cancel should be a red button with white text and an action cursor');
assert.match(torrentSource, /case 'confirm':[\s\S]*border-border bg-white text-black[\s\S]*cursor-pointer/, 'torrent Confirm should be a white button with black text and an action cursor');
assert.match(torrentSource, /disabled:cursor-not-allowed/, 'torrent action buttons should show a disabled cursor while busy');
assert.match(torrentSource, /flex\s+h-8\s+min-w-\[128px\]\s+items-center\s+justify-center\s+gap-2\.5\s+rounded-md\s+px-5\s+text-sm\s+font-semibold/, 'torrent action buttons should keep Svelte popup sizing');
assert.doesNotMatch(torrentSource, /bg-primary\s+text-primary-foreground/, 'torrent primary actions should not use the compact filled primary palette');
assert.match(torrentSource, /Pause[\s\S]*pauseJob\(job\.id\)/, 'active torrent actions should keep Pause behavior');
assert.match(torrentSource, /Resume[\s\S]*resumeJob\(job\.id\)/, 'paused torrent actions should keep Resume behavior');
assert.match(torrentSource, /Retry[\s\S]*retryJob\(job\.id\)/, 'failed torrent actions should keep Retry behavior');
assert.match(torrentSource, /Open[\s\S]*openJobFile\(job\.id\)/, 'completed torrent actions should keep Open behavior');
assert.match(torrentSource, /Open[\s\S]*openJobFile\(job\.id\);[\s\S]*\{ closeOnSuccess: true \}/, 'completed torrent Open should close the popup after a successful action');
assert.match(torrentSource, /Show[\s\S]*revealJobInFolder\(job\.id\);[\s\S]*\{ closeOnSuccess: true \}/, 'completed torrent Show should close the popup after a successful action');
assert.match(torrentSource, /Confirm delete[\s\S]*popup\.onCancelClick/, 'torrent popup should make the confirmed cancel state destructive');
assert.match(sharedPopupSource, /cancelJob\(activeJobId,\s*\{\s*deleteFromDisk:\s*true\s*\}\)[\s\S]*closeOnSuccess:\s*true/, 'confirmed torrent Cancel should delete files from disk and close through the shared popup lifecycle');
assert.match(torrentSource, /isCanceled\(job\)[\s\S]*Action\('Close'/, 'canceled torrent progress should expose a safe Close fallback');
assert.doesNotMatch(torrentSource, /<span className="text-muted-foreground">[>>]<\/span>/, 'torrent detail rows should not render decorative chevrons');
assert.doesNotMatch(torrentSource, /(?<!\{)#[0-9a-fA-F]{3,8}\b|teal|cyan|emerald/, 'torrent popup should use existing theme tokens instead of hardcoded custom colors');
