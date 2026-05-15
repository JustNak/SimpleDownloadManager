import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const mainSource = await readFile(new URL('../src/main.ts', import.meta.url), 'utf8');
const appSource = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
const backendSource = await readFile(new URL('../src/backend.ts', import.meta.url), 'utf8');
const popupReadySource = await readFile(new URL('../src/popupReady.ts', import.meta.url), 'utf8').catch(() => '');
const batchProgressSource = await readFile(new URL('../src/batchProgress.ts', import.meta.url), 'utf8');
const progressPopupSource = await readFile(new URL('../src/useProgressPopup.svelte.ts', import.meta.url), 'utf8');
const batchPopupSource = await readFile(new URL('../src/BatchProgressWindow.svelte', import.meta.url), 'utf8');
const queueSource = await readFile(new URL('../src/QueueView.svelte', import.meta.url), 'utf8');
const promptSource = await readFile(new URL('../src/DownloadPromptWindow.svelte', import.meta.url), 'utf8');
const commandsSource = await readFile(new URL('../src-tauri/src/commands/mod.rs', import.meta.url), 'utf8');
const commandsEventsSource = await readFile(new URL('../src-tauri/src/commands/events.rs', import.meta.url), 'utf8');
const commandsRuntimeSource = `${commandsSource}\n${commandsEventsSource}`;
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
assert.match(backendSource, /export async function markPopupReady\(\): Promise<void>[\s\S]*invokeCommand\('mark_popup_ready'\)/, 'backend should expose a narrow popup-ready command without a state snapshot');
assert.match(popupReadySource, /import \{ tick \} from 'svelte'/, 'popup readiness helper should wait for Svelte DOM flush');
assert.match(popupReadySource, /import \{ markPopupReady \} from '\.\/backend'/, 'popup readiness helper should own the native ready command call');
assert.match(popupReadySource, /await tick\(\)[\s\S]*requestAnimationFrame[\s\S]*await markPopupReady\(\)[\s\S]*classList\.add\('popup-ready'\)/, 'popup readiness helper should reveal after a DOM flush and a browser paint');
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

assert.match(mainSource, /const POPUP_WINDOW_MODES = new Set\(\[[\s\S]*download-prompt[\s\S]*download-progress[\s\S]*torrent-progress[\s\S]*batch-progress[\s\S]*\]\)/, 'main entry should identify popup-only routes for readiness gating');
assert.doesNotMatch(mainSource, /finally \{[\s\S]*signalPopupReady\(\);[\s\S]*\}/, 'main route should not reveal successful popup mounts before their initial snapshots apply theme');
assert.doesNotMatch(mainSource, /markPopupReady\(\)[\s\S]*document\.documentElement\.classList\.add\('popup-ready'\)/, 'main route should not own normal popup readiness');
assert.match(mainSource, /catch \(error\)[\s\S]*renderPopupLoadFailure\(target\);[\s\S]*await revealPopupWhenReady\(\);/, 'popup route import or mount failures should still reveal the themed fallback shell');
assert.match(mainSource, /function renderPopupLoadFailure[\s\S]*Popup failed to load[\s\S]*popup-load-failure/, 'popup route import or mount failures should render a themed fallback shell');

assert.match(
  progressPopupSource,
  /import \{ revealPopupWhenReady \} from '\.\/popupReady'[\s\S]*applySnapshotAppearance\(snapshot\);[\s\S]*applySnapshotJob\(snapshot\);[\s\S]*await revealPopupWhenReady\(\);[\s\S]*const nextDispose = await subscribeToProgressJobSnapshot/,
  'single progress popups should reveal only after initial snapshot appearance and job state are applied',
);
assert.match(
  batchPopupSource,
  /import \{ revealPopupWhenReady \} from '\.\/popupReady'[\s\S]*applySnapshotAppearance\(snapshot\);[\s\S]*jobs = orderedBatchJobs\(snapshot\.context, snapshot\.jobs\);[\s\S]*await revealPopupWhenReady\(\);[\s\S]*const nextDispose = await subscribeToBatchProgressSnapshot/,
  'batch progress popup should reveal only after initial snapshot appearance and job state are applied',
);
assert.match(
  promptSource,
  /import \{ revealPopupWhenReady \} from '\.\/popupReady'[\s\S]*applySnapshotAppearance\(settingsSnapshot\);[\s\S]*prompt = currentPrompt;[\s\S]*await revealPopupWhenReady\(\);[\s\S]*const nextPromptDispose = await subscribeToDownloadPromptChanged/,
  'download prompt should reveal only after initial settings appearance and prompt state are applied',
);

for (const rustSymbol of ['ProgressJobSnapshot', 'BatchProgressSnapshot', 'SettingsSnapshot', 'DownloadUpdateBatch']) {
  assert.match(commandsRuntimeSource, new RegExp(`struct ${rustSymbol}`), `commands should define ${rustSymbol}`);
}

for (const command of ['pause_jobs', 'resume_jobs', 'cancel_jobs', 'delete_jobs']) {
  assert.match(commandsSource, new RegExp(`pub async fn ${command}\\(`), `commands should expose ${command}`);
}

assert.match(commandsRuntimeSource, /emit_to\("main",\s*STATE_CHANGED_EVENT/, 'full desktop snapshots should only be emitted to the main webview');
assert.match(commandsRuntimeSource, /DOWNLOADS_UPDATE_BATCH_EVENT/, 'commands should define a batched download update event');
assert.match(commandsRuntimeSource, /emit_progress_delta/, 'commands should define a lightweight progress delta emitter');
assert.match(commandsRuntimeSource, /emit_popup_snapshots\(app,\s*snapshot\)/, 'snapshot emission should fan out targeted popup payloads');
assert.match(commandsRuntimeSource, /fn progress_job_for_window_label[\s\S]*strip_prefix\("download-progress-"\)[\s\S]*job\.transfer_kind == TransferKind::Http[\s\S]*strip_prefix\("torrent-progress-"\)[\s\S]*job\.transfer_kind == TransferKind::Torrent/, 'single progress popup snapshots should be scoped by label family and transfer kind');
assert.doesNotMatch(commandsRuntimeSource, /progress_window_label\(&job\.id\)[\s\S]*\|\|[\s\S]*torrent_progress_window_label\(&job\.id\)/, 'download and torrent progress labels should not share an OR-matched snapshot lookup');
assert.doesNotMatch(downloadSource, /powershell\.exe|Compress-Archive/, 'bulk archive creation should not shell out to PowerShell');
assert.match(httpDownloadSource, /update_job_progress_delta/, 'single-stream HTTP progress ticks should use lightweight progress deltas');
assert.match(segmentedDownloadSource, /update_segmented_job_progress_delta/, 'segmented HTTP progress ticks should use lightweight progress deltas');
assert.match(segmentedDownloadSource, /sync_downloaded_bytes_delta/, 'stopped segmented progress sync should use lightweight progress deltas');
assert.match(torrentDownloadSource, /update_torrent_progress_delta/, 'torrent progress ticks should use lightweight progress deltas');
assert.doesNotMatch(runtimeStateSource, /\.map\(add_artifact_existence\)|Path::new\(&job\.target_path\)\.exists\(\)/, 'full snapshots should not probe completed artifacts on disk');
