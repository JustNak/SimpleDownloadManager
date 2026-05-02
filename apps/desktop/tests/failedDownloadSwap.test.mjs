import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backendSource = await readFile(path.join(repoRoot, 'apps/desktop/src/backend.ts'), 'utf8');
const appSource = await readFile(path.join(repoRoot, 'apps/desktop/src/App.svelte'), 'utf8');
const progressSource = await readFile(path.join(repoRoot, 'apps/desktop/src/DownloadProgressWindow.svelte'), 'utf8');
const queueSource = await readFile(path.join(repoRoot, 'apps/desktop/src/QueueView.svelte'), 'utf8');
const commandSource = await readFile(path.join(repoRoot, 'apps/desktop/src-tauri/src/commands/mod.rs'), 'utf8');
const mainSource = await readFile(path.join(repoRoot, 'apps/desktop/src-tauri/src/main.rs'), 'utf8');

assert.match(backendSource, /export async function swapFailedDownloadToBrowser\(id: string\): Promise<void>/, 'desktop backend should expose a failed-download browser action');
assert.match(backendSource, /invokeCommand\('swap_failed_download_to_browser', \{ id \}\)/, 'failed-download browser action should call the Tauri command');
assert.match(commandSource, /pub async fn swap_failed_download_to_browser\(/, 'Tauri backend should expose a command for failed-download browser swap');
assert.match(mainSource, /commands::swap_failed_download_to_browser/, 'failed-download browser swap command should be registered with Tauri');
assert.match(progressSource, /swapFailedDownloadToBrowser/, 'failed progress popup should import and call the failed-download browser action');
assert.match(progressSource, /canSwapFailedDownloadToBrowser\(job\)[\s\S]*Open in browser/, 'failed progress popup should render browser handoff for eligible downloads');
assert.match(appSource, /async function handleSwapFailedToBrowser\(id: string\)/, 'main app should handle failed-download browser handoff from queue surfaces');
assert.match(appSource, /onSwapFailedToBrowser=\{\(id\) => void handleSwapFailedToBrowser\(id\)\}/, 'QueueView should receive the failed-download browser handler');
assert.match(queueSource, /onSwapFailedToBrowser: \(id: string\) => void/, 'QueueView props should include a failed-download browser handler');
assert.match(queueSource, /canSwapFailedDownloadToBrowser\(job\)[\s\S]*Open in browser/, 'failed queue row menus should expose browser handoff when eligible');
