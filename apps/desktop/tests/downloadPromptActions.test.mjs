import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backendSource = await readFile(path.join(repoRoot, 'apps/desktop/src/backend.ts'), 'utf8');
const promptSource = await readFile(path.join(repoRoot, 'apps/desktop/src/DownloadPromptWindow.svelte'), 'utf8');
const ipcSource = await readFile(path.join(repoRoot, 'apps/desktop/src-tauri/src/ipc/mod.rs'), 'utf8');
const windowsSource = await readFile(path.join(repoRoot, 'apps/desktop/src-tauri/src/windows.rs'), 'utf8');
const progressSources = [
  ['single download progress', await readFile(path.join(repoRoot, 'apps/desktop/src/DownloadProgressWindow.svelte'), 'utf8')],
  ['batch download progress', await readFile(path.join(repoRoot, 'apps/desktop/src/BatchProgressWindow.svelte'), 'utf8')],
];

assert.match(backendSource, /export async function swapDownloadPrompt/, 'desktop backend should expose a swapDownloadPrompt action');
assert.match(backendSource, /swap_download_prompt/, 'swapDownloadPrompt should call the swap_download_prompt Tauri command');
assert.match(backendSource, /duplicateAction/, 'confirmDownloadPrompt should send an explicit duplicate action instead of a boolean allowDuplicate flag');
assert.match(backendSource, /renamedFilename/, 'confirmDownloadPrompt should support an explicit renamed filename for duplicate prompts');
assert.match(promptSource, /#snippet BrowserWindowIcon/, 'prompt should include a generic browser SVG icon');
assert.match(promptSource, /\bSwap\b/, 'prompt should still render a Swap button for non-duplicate browser prompts');
assert.match(promptSource, /bg-foreground px-3 text-xs font-semibold text-background/, 'Swap button should use inverse high-contrast foreground/background styling');
assert.match(promptSource, /bg-destructive px-3 text-xs font-semibold text-destructive-foreground/, 'Cancel button should use destructive red styling');
assert.match(promptSource, /prompt\?\.source\?\.entryPoint === 'browser_download' && !isDuplicate/, 'Swap button should only be shown for non-duplicate browser download prompts');
assert.match(promptSource, /prompt\?\.duplicateJob \|\| prompt\?\.duplicatePath/, 'destination-only duplicate prompts should render the same duplicate action UI as URL duplicates');

for (const label of ['Choose Action', 'Overwrite', 'Rename', 'Download Anyway']) {
  assert.match(promptSource, new RegExp(`>\\s*${label}\\s*<`), `duplicate prompt should render ${label}`);
}

assert.match(promptSource, /confirmDuplicateAction\('overwrite'\)/, 'Overwrite should send a replace duplicate action');
assert.match(promptSource, /duplicateAction:\s*'rename'/, 'Rename should send a rename duplicate action');
assert.match(promptSource, /confirmDuplicateAction\('download_anyway'\)/, 'Download Anyway should send an allow-copy duplicate action');
assert.doesNotMatch(promptSource, />\s*Show Existing\s*</, 'compact duplicate prompt should not keep the old Show Existing secondary action');
assert.match(windowsSource, /fn download_prompt_window_geometry\(\) -> PopupWindowGeometry \{[\s\S]*width:\s*460\.0,[\s\S]*height:\s*280\.0,[\s\S]*min_width:\s*460\.0,[\s\S]*min_height:\s*280\.0/, 'download prompt geometry policy should keep the compact fixed 460x280 dimensions');
assert.match(windowsSource, /WebviewWindowBuilder::new\([\s\S]*DOWNLOAD_PROMPT_WINDOW[\s\S]*\.inner_size\(geometry\.width,\s*geometry\.height\)[\s\S]*\.min_inner_size\(geometry\.min_width,\s*geometry\.min_height\)[\s\S]*\.max_inner_size\(geometry\.width,\s*geometry\.height\)/, 'download prompt window should read its compact geometry from the shared popup geometry policy');
assert.match(promptSource, /class="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface px-3 py-2"/, 'download prompt content should use a compact overflow-protected shell');
assert.match(promptSource, /class="mt-auto flex min-h-\[38px\] shrink-0 items-center justify-between gap-2 border-t border-border pt-2"/, 'download prompt action bar should keep a predictable compact height');
assert.match(promptSource, /#snippet MetaValue[\s\S]*min-w-0 truncate/, 'prompt metadata values should be min-width constrained and truncated');
assert.match(promptSource, /title=\{duplicateLabel\}/, 'duplicate prompt filename should preserve its full value in a tooltip while compact');
assert.match(ipcSource, /"enqueue_download"[\s\S]*handoff_transfer_kind\([\s\S]*if source\.entry_point == "browser_download" && transfer_kind == TransferKind::Http[\s\S]*prepare_download_prompt[\s\S]*prompt_has_duplicate[\s\S]*run_prompt_download/, 'HTTP auto enqueue handoffs should still reroute detected duplicates through the prompt flow');
assert.match(ipcSource, /"prompt_download"[\s\S]*handoff_transfer_kind\([\s\S]*if transfer_kind == TransferKind::Torrent[\s\S]*return enqueue_handoff_download\(/, 'torrent prompt handoffs should bypass the download prompt and enqueue directly');
assert.match(ipcSource, /"enqueue_download"[\s\S]*if transfer_kind == TransferKind::Torrent[\s\S]*return enqueue_handoff_download\(/, 'torrent enqueue handoffs should bypass duplicate prompt routing');

for (const [name, source] of progressSources) {
  assert.doesNotMatch(source, />\s*Swap\s*</, `${name} UI should not render a Swap button`);
  assert.doesNotMatch(source, /swapDownloadPrompt/, `${name} UI should not call the prompt Swap action`);
  assert.doesNotMatch(source, /BrowserWindowIcon/, `${name} UI should not include the browser Swap icon`);
}
