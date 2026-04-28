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
assert.match(promptSource, /function BrowserWindowIcon/, 'prompt should include a generic browser SVG icon');
assert.match(promptSource, />\s*Swap\s*</, 'prompt should render a Swap button');
assert.match(
  promptSource,
  /bg-foreground px-3 text-sm font-semibold text-background/,
  'Swap button should use inverse high-contrast foreground/background styling',
);
assert.match(
  promptSource,
  /bg-destructive px-3 text-sm font-semibold text-destructive-foreground/,
  'Cancel button should use destructive red styling',
);
assert.match(
  promptSource,
  /prompt\?\.source\?\.entryPoint === 'browser_download'/,
  'Swap button should only be shown for browser download prompts',
);

for (const [name, source] of progressSources) {
  assert.doesNotMatch(source, />\s*Swap\s*</, `${name} UI should not render a Swap button`);
  assert.doesNotMatch(source, /swapDownloadPrompt/, `${name} UI should not call the prompt Swap action`);
  assert.doesNotMatch(source, /BrowserWindowIcon/, `${name} UI should not include the browser Swap icon`);
}
