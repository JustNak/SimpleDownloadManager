import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backendSource = await readFile(path.join(repoRoot, 'apps/desktop/src/backend.ts'), 'utf8');
const promptSource = await readFile(path.join(repoRoot, 'apps/desktop/src/DownloadPromptWindow.tsx'), 'utf8');
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

for (const [name, source] of progressSources) {
  assert.doesNotMatch(source, />\s*Swap\s*</, `${name} UI should not render a Swap button`);
  assert.doesNotMatch(source, /swapDownloadPrompt/, `${name} UI should not call the prompt Swap action`);
  assert.doesNotMatch(source, /BrowserWindowIcon/, `${name} UI should not include the browser Swap icon`);
}
