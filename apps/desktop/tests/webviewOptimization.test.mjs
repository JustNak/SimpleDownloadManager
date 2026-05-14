import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const mainSource = await readFile(new URL('../src/main.ts', import.meta.url), 'utf8');
const appSource = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
const backendSource = await readFile(new URL('../src/backend.ts', import.meta.url), 'utf8');
const batchProgressSource = await readFile(new URL('../src/batchProgress.ts', import.meta.url), 'utf8');
const progressPopupSource = await readFile(new URL('../src/useProgressPopup.svelte.ts', import.meta.url), 'utf8');
const batchPopupSource = await readFile(new URL('../src/BatchProgressWindow.svelte', import.meta.url), 'utf8');
const queueSource = await readFile(new URL('../src/QueueView.svelte', import.meta.url), 'utf8');
const promptSource = await readFile(new URL('../src/DownloadPromptWindow.svelte', import.meta.url), 'utf8');
const commandsSource = await readFile(new URL('../src-tauri/src/commands/mod.rs', import.meta.url), 'utf8');
const downloadSource = await readFile(new URL('../src-tauri/src/download/mod.rs', import.meta.url), 'utf8');
const httpDownloadSource = await readFile(new URL('../src-tauri/src/download/http.rs', import.meta.url), 'utf8');
const segmentedDownloadSource = await readFile(new URL('../src-tauri/src/download/segmented.rs', import.meta.url), 'utf8');
const torrentDownloadSource = await readFile(new URL('../src-tauri/src/download/torrent.rs', import.meta.url), 'utf8');
const runtimeStateSource = await readFile(new URL('../src-tauri/src/state/runtime.rs', import.meta.url), 'utf8');
const tauriConfig = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));

for (const importPath of ['./App.svelte', './DownloadPromptWindow.svelte', './DownloadProgressWindow.svelte', './TorrentProgressWindow.svelte', './BatchProgressWindow.svelte']) {
  assert.doesNotMatch(mainSource, new RegExp(`^import\\s+.*${importPath.replace(/[./]/g, '\\$&')}`, 'm'), `main route should not eagerly import ${importPath}`);
}

for (const chunk of [
  "import('./App.svelte')",
  "import('./DownloadPromptWindow.svelte')",
  "import('./DownloadProgressWindow.svelte')",
  "import('./TorrentProgressWindow.svelte')",
  "import('./BatchProgressWindow.svelte')",
]) {
  assert.ok(mainSource.includes(chunk), `main route should dynamically import ${chunk}`);
}

for (const symbol of ['getProgressJobSnapshot', 'subscribeToProgressJobSnapshot', 'getBatchProgressSnapshot', 'subscribeToBatchProgressSnapshot', 'getSettingsSnapshot', 'subscribeToSettingsSnapshot']) {
  assert.ok(backendSource.includes(`export async function ${symbol}`), `backend should expose ${symbol}`);
}

for (const symbol of ['pauseJobs', 'resumeJobs', 'cancelJobs']) {
  assert.ok(backendSource.includes(`export async function ${symbol}`), `backend should expose scoped batch command ${symbol}`);
}

assert.doesNotMatch(appSource, /^import\s+.*\.\/SettingsPage\.svelte/m, 'main app should not eagerly import SettingsPage');
assert.doesNotMatch(appSource, /^import\s+.*\.\/AddDownloadModal\.svelte/m, 'main app should not eagerly import AddDownloadModal');
assert.match(appSource, /import\('\.\/SettingsPage\.svelte'\)/, 'main app should dynamically import SettingsPage when needed');
assert.match(appSource, /import\('\.\/AddDownloadModal\.svelte'\)/, 'main app should dynamically import AddDownloadModal when needed');
assert.match(appSource, /subscribeToDownloadUpdateBatch\(\(batch\) => \{\s*applyDownloadUpdateBatchWhenVisible\(batch\);\s*\}\)/, 'progress-only download batches should update rows without refreshing diagnostics');

assert.match(backendSource, /app:\/\/downloads-update-batch/, 'backend should define a batched download update event');
assert.match(backendSource, /export interface DownloadUpdateBatch/, 'backend should expose the download update batch payload type');
assert.match(backendSource, /export async function subscribeToDownloadUpdateBatch/, 'backend should expose a download update batch subscription');
assert.match(backendSource, /applyDownloadUpdateBatch/, 'backend should expose a helper for applying download update batches');
assert.match(backendSource, /app:\/\/progress-job-snapshot/, 'backend should define a lightweight progress job event');
assert.match(backendSource, /app:\/\/batch-progress-snapshot/, 'backend should define a lightweight batch progress event');
assert.match(backendSource, /app:\/\/settings-snapshot/, 'backend should define a lightweight settings event');
assert.doesNotMatch(backendSource, /^import\s+\*\s+as\s+previewBackend\s+from\s+['"]\.\/backendPreview['"]/m, 'backend should not eagerly import browser-preview mocks into production webview chunks');
assert.match(backendSource, /import\(['"]\.\/backendPreview['"]\)/, 'backend should lazy-load browser-preview mocks only when preview mode is active');
assert.match(batchProgressSource, /export function createStoredProgressBatchContext/, 'batch progress should provide a lightweight stored-context helper outside preview mocks');
assert.doesNotMatch(progressPopupSource, /subscribeToStateChanged|getAppSnapshot/, 'single progress popup should not subscribe to full app snapshots');
assert.doesNotMatch(batchPopupSource, /subscribeToStateChanged|getAppSnapshot|getProgressBatchContext/, 'batch popup should not subscribe to full app snapshots');
assert.match(queueSource, /extraHeights/, 'main queue virtualization should account for expanded bulk row height');
assert.match(queueSource, /bulkMemberVirtualQueue/, 'main bulk row expansion should virtualize large member lists');
assert.match(batchPopupSource, /getVirtualQueueWindow/, 'batch popup should virtualize large job lists');
assert.match(batchPopupSource, /renderedBatchJobs/, 'batch popup should render only the virtual job window for large batches');
assert.match(batchPopupSource, /pauseJobs|resumeJobs|cancelJobs/, 'batch popup should use scoped batch commands for hundreds-link actions');
assert.doesNotMatch(batchPopupSource, /runForJobs\(targetJobs, action\)|runForJobs\(jobs\.filter/, 'batch popup should not loop per-job IPC actions for batch controls');
assert.doesNotMatch(promptSource, /subscribeToStateChanged|getAppSnapshot/, 'download prompt should only subscribe to prompt and settings events');
assert.match(progressPopupSource, /const nextDispose = await subscribeToProgressJobSnapshot[\s\S]*if \(disposed\) \{[\s\S]*void nextDispose\(\);[\s\S]*return;[\s\S]*dispose = nextDispose/, 'progress popup should immediately release late async subscriptions after teardown');
assert.match(batchPopupSource, /const nextDispose = await subscribeToBatchProgressSnapshot[\s\S]*if \(disposed\) \{[\s\S]*void nextDispose\(\);[\s\S]*return;[\s\S]*dispose = nextDispose/, 'batch popup should immediately release late async subscriptions after teardown');
assert.match(promptSource, /const nextPromptDispose = await subscribeToDownloadPromptChanged[\s\S]*if \(disposed\) \{[\s\S]*void nextPromptDispose\(\);[\s\S]*return;[\s\S]*promptDispose = nextPromptDispose/, 'download prompt should immediately release late prompt subscriptions after teardown');
assert.equal(tauriConfig.build.removeUnusedCommands, true, 'Tauri should prune unused commands during build');

for (const rustSymbol of ['ProgressJobSnapshot', 'BatchProgressSnapshot', 'SettingsSnapshot', 'DownloadUpdateBatch']) {
  assert.match(commandsSource, new RegExp(`struct ${rustSymbol}`), `commands should define ${rustSymbol}`);
}

for (const command of ['pause_jobs', 'resume_jobs', 'cancel_jobs', 'delete_jobs']) {
  assert.match(commandsSource, new RegExp(`pub async fn ${command}\\(`), `commands should expose ${command}`);
}

assert.match(commandsSource, /emit_to\("main",\s*STATE_CHANGED_EVENT/, 'full desktop snapshots should only be emitted to the main webview');
assert.match(commandsSource, /DOWNLOADS_UPDATE_BATCH_EVENT/, 'commands should define a batched download update event');
assert.match(commandsSource, /emit_progress_delta/, 'commands should define a lightweight progress delta emitter');
assert.match(commandsSource, /emit_popup_snapshots\(app,\s*snapshot\)/, 'snapshot emission should fan out targeted popup payloads');
assert.match(commandsSource, /fn progress_job_for_window_label[\s\S]*strip_prefix\("download-progress-"\)[\s\S]*job\.transfer_kind == TransferKind::Http[\s\S]*strip_prefix\("torrent-progress-"\)[\s\S]*job\.transfer_kind == TransferKind::Torrent/, 'single progress popup snapshots should be scoped by label family and transfer kind');
assert.doesNotMatch(commandsSource, /progress_window_label\(&job\.id\)[\s\S]*\|\|[\s\S]*torrent_progress_window_label\(&job\.id\)/, 'download and torrent progress labels should not share an OR-matched snapshot lookup');
assert.doesNotMatch(downloadSource, /powershell\.exe|Compress-Archive/, 'bulk archive creation should not shell out to PowerShell');
assert.match(httpDownloadSource, /update_job_progress_delta/, 'single-stream HTTP progress ticks should use lightweight progress deltas');
assert.match(segmentedDownloadSource, /update_segmented_job_progress_delta/, 'segmented HTTP progress ticks should use lightweight progress deltas');
assert.match(segmentedDownloadSource, /sync_downloaded_bytes_delta/, 'stopped segmented progress sync should use lightweight progress deltas');
assert.match(torrentDownloadSource, /update_torrent_progress_delta/, 'torrent progress ticks should use lightweight progress deltas');
assert.doesNotMatch(runtimeStateSource, /\.map\(add_artifact_existence\)|Path::new\(&job\.target_path\)\.exists\(\)/, 'full snapshots should not probe completed artifacts on disk');
