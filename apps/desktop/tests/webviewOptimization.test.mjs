import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const mainSource = await readFile(new URL('../src/main.ts', import.meta.url), 'utf8');
const backendSource = await readFile(new URL('../src/backend.ts', import.meta.url), 'utf8');
const progressPopupSource = await readFile(new URL('../src/useProgressPopup.svelte.ts', import.meta.url), 'utf8');
const batchPopupSource = await readFile(new URL('../src/BatchProgressWindow.svelte', import.meta.url), 'utf8');
const promptSource = await readFile(new URL('../src/DownloadPromptWindow.svelte', import.meta.url), 'utf8');
const commandsSource = await readFile(new URL('../src-tauri/src/commands/mod.rs', import.meta.url), 'utf8');

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

assert.match(backendSource, /app:\/\/progress-job-snapshot/, 'backend should define a lightweight progress job event');
assert.match(backendSource, /app:\/\/batch-progress-snapshot/, 'backend should define a lightweight batch progress event');
assert.match(backendSource, /app:\/\/settings-snapshot/, 'backend should define a lightweight settings event');
assert.doesNotMatch(progressPopupSource, /subscribeToStateChanged|getAppSnapshot/, 'single progress popup should not subscribe to full app snapshots');
assert.doesNotMatch(batchPopupSource, /subscribeToStateChanged|getAppSnapshot|getProgressBatchContext/, 'batch popup should not subscribe to full app snapshots');
assert.doesNotMatch(promptSource, /subscribeToStateChanged|getAppSnapshot/, 'download prompt should only subscribe to prompt and settings events');

for (const rustSymbol of ['ProgressJobSnapshot', 'BatchProgressSnapshot', 'SettingsSnapshot']) {
  assert.match(commandsSource, new RegExp(`struct ${rustSymbol}`), `commands should define ${rustSymbol}`);
}

assert.match(commandsSource, /emit_to\("main",\s*STATE_CHANGED_EVENT/, 'full desktop snapshots should only be emitted to the main webview');
assert.match(commandsSource, /emit_popup_snapshots\(app,\s*snapshot\)/, 'snapshot emission should fan out targeted popup payloads');
