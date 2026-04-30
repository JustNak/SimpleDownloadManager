import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backendSource = await readFile(path.join(repoRoot, 'apps/desktop/src/backend.ts'), 'utf8');
const promptSource = await readFile(path.join(repoRoot, 'apps/desktop/src/DownloadPromptWindow.tsx'), 'utf8');
const hostProtocolSource = await readFile(
  path.join(repoRoot, 'apps/desktop-core/src/host_protocol.rs'),
  'utf8',
);
const windowsSource = await readFile(path.join(repoRoot, 'apps/desktop/src-tauri/src/windows.rs'), 'utf8');
const progressSources = [
  ['single download progress', await readFile(path.join(repoRoot, 'apps/desktop/src/DownloadProgressWindow.tsx'), 'utf8')],
  ['batch download progress', await readFile(path.join(repoRoot, 'apps/desktop/src/BatchProgressWindow.tsx'), 'utf8')],
];

assert.match(
  backendSource,
  /export async function swapDownloadPrompt/,
  'desktop backend should expose a swapDownloadPrompt action',
);
assert.match(
  backendSource,
  /swap_download_prompt/,
  'swapDownloadPrompt should call the swap_download_prompt Tauri command',
);
assert.match(
  backendSource,
  /duplicateAction/,
  'confirmDownloadPrompt should send an explicit duplicate action instead of a boolean allowDuplicate flag',
);
assert.match(
  backendSource,
  /renamedFilename/,
  'confirmDownloadPrompt should support an explicit renamed filename for duplicate prompts',
);
assert.match(promptSource, /function BrowserWindowIcon/, 'prompt should include a generic browser SVG icon');
assert.match(promptSource, />\s*Swap\s*</, 'prompt should still render a Swap button for non-duplicate browser prompts');
assert.match(
  promptSource,
  /bg-foreground px-3 text-xs font-semibold text-background/,
  'Swap button should use inverse high-contrast foreground/background styling',
);
assert.match(
  promptSource,
  /bg-destructive px-3 text-xs font-semibold text-destructive-foreground/,
  'Cancel button should use destructive red styling',
);
assert.match(
  promptSource,
  /prompt\?\.source\?\.entryPoint === 'browser_download' && !isDuplicate/,
  'Swap button should only be shown for non-duplicate browser download prompts',
);
assert.match(
  promptSource,
  /prompt\?\.duplicateJob \|\| prompt\?\.duplicatePath/,
  'destination-only duplicate prompts should render the same duplicate action UI as URL duplicates',
);
assert.match(
  promptSource,
  />\s*Choose Action\s*</,
  'duplicate prompt should use one primary Choose Action button',
);
assert.match(
  promptSource,
  />\s*Overwrite\s*</,
  'duplicate action menu should offer Overwrite',
);
assert.match(
  promptSource,
  />\s*Rename\s*</,
  'duplicate action menu should offer Rename',
);
assert.match(
  promptSource,
  />\s*Download Anyway\s*</,
  'duplicate action menu should offer Download Anyway',
);
assert.match(
  promptSource,
  /confirmDuplicateAction\('overwrite'\)/,
  'Overwrite should send a replace-queue duplicate action',
);
assert.match(
  promptSource,
  /duplicateAction:\s*'rename'/,
  'Rename should send a rename duplicate action',
);
assert.match(
  promptSource,
  /confirmDuplicateAction\('download_anyway'\)/,
  'Download Anyway should send an allow-copy duplicate action',
);
assert.doesNotMatch(
  promptSource,
  />\s*Show Existing\s*</,
  'compact duplicate prompt should not keep the old Show Existing secondary action',
);
assert.match(
  windowsSource,
  /\.inner_size\(460\.0,\s*280\.0\)[\s\S]*\.min_inner_size\(460\.0,\s*280\.0\)[\s\S]*\.max_inner_size\(460\.0,\s*280\.0\)/,
  'download prompt window should use the compact fixed 460x280 geometry',
);
assert.match(
  promptSource,
  /className="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface px-3 py-2"/,
  'download prompt content should use a compact overflow-protected shell',
);
assert.match(
  promptSource,
  /className="mt-auto flex min-h-\[38px\] shrink-0 items-center justify-between gap-2 border-t border-border pt-2"/,
  'download prompt action bar should keep a predictable compact height',
);
assert.match(
  promptSource,
  /function MetaValue[\s\S]*className=\{`min-w-0 truncate/,
  'prompt metadata values should be min-width constrained and truncated',
);
assert.match(
  promptSource,
  /title=\{duplicateLabel\}/,
  'duplicate prompt filename should preserve its full value in a tooltip while compact',
);
assert.match(
  hostProtocolSource,
  /"enqueue_download"[\s\S]*prepare_download_prompt[\s\S]*prompt_has_duplicate[\s\S]*run_prompt_download/,
  'auto enqueue handoffs should reroute detected duplicates through the prompt flow',
);

for (const [name, source] of progressSources) {
  assert.doesNotMatch(source, />\s*Swap\s*</, `${name} UI should not render a Swap button`);
  assert.doesNotMatch(source, /swapDownloadPrompt/, `${name} UI should not call the prompt Swap action`);
  assert.doesNotMatch(source, /BrowserWindowIcon/, `${name} UI should not include the browser Swap icon`);
}
