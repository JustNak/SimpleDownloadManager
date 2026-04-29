import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backendSource = await readFile(path.join(repoRoot, 'apps/desktop/src/backend.ts'), 'utf8');
const appSource = await readFile(path.join(repoRoot, 'apps/desktop/src/App.tsx'), 'utf8');
const progressSource = await readFile(path.join(repoRoot, 'apps/desktop/src/DownloadProgressWindow.tsx'), 'utf8');
const queueSource = await readFile(path.join(repoRoot, 'apps/desktop/src/QueueView.tsx'), 'utf8');
const commandSource = await readFile(path.join(repoRoot, 'apps/desktop/src-tauri/src/commands/mod.rs'), 'utf8');
const mainSource = await readFile(path.join(repoRoot, 'apps/desktop/src-tauri/src/main.rs'), 'utf8');

assert.match(
  backendSource,
  /export async function swapFailedDownloadToBrowser\(id: string\): Promise<void>/,
  'desktop backend should expose a failed-download Swap action',
);
assert.match(
  backendSource,
  /invokeCommand\('swap_failed_download_to_browser', \{ id \}\)/,
  'failed-download Swap action should call the Tauri command',
);
assert.match(
  commandSource,
  /pub async fn swap_failed_download_to_browser\(/,
  'Tauri backend should expose a command for failed-download browser swap',
);
assert.match(
  mainSource,
  /commands::swap_failed_download_to_browser/,
  'failed-download browser swap command should be registered with Tauri',
);

assert.match(
  progressSource,
  /swapFailedDownloadToBrowser/,
  'failed progress popup should import and call the failed-download Swap action',
);
assert.match(
  progressSource,
  /isFailed && canSwapFailedDownloadToBrowser\(job\)[\s\S]*label="Swap"/,
  'failed progress popup should render Swap for browser-origin failed downloads',
);

assert.match(
  appSource,
  /async function handleSwapFailedToBrowser\(id: string\)/,
  'main app should handle failed-download browser swap from queue surfaces',
);
assert.match(
  appSource,
  /onSwapFailedToBrowser=\{handleSwapFailedToBrowser\}/,
  'QueueView should receive the failed-download Swap handler',
);
assert.match(
  queueSource,
  /onSwapFailedToBrowser: \(id: string\) => void/,
  'QueueView props should include a failed-download Swap handler',
);
assert.match(
  queueSource,
  /canSwapFailedDownloadToBrowser\(job\)[\s\S]*title="Swap"/,
  'failed queue rows should render an inline Swap button when eligible',
);
assert.match(
  queueSource,
  /canSwapFailedDownloadToBrowser\(job\)[\s\S]*label="Swap"/,
  'failed queue row menus or details should expose Swap when eligible',
);
