import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const lifecycleSource = await readFile(new URL('../src-tauri/src/lifecycle.rs', import.meta.url), 'utf8');
const windowsSource = await readFile(new URL('../src-tauri/src/windows.rs', import.meta.url), 'utf8');
const ipcSource = await readFile(new URL('../src-tauri/src/ipc/mod.rs', import.meta.url), 'utf8');
const commandsSource = await readFile(new URL('../src-tauri/src/commands/mod.rs', import.meta.url), 'utf8');

function bodyAfter(source, signature) {
  const start = source.indexOf(signature);
  assert.notEqual(start, -1, `missing signature: ${signature}`);

  const bodyStart = source.indexOf('{', start);
  assert.notEqual(bodyStart, -1, `missing body for: ${signature}`);

  let depth = 0;
  for (let index = bodyStart; index < source.length; index += 1) {
    const char = source[index];
    if (char === '{') {
      depth += 1;
    } else if (char === '}') {
      depth -= 1;
      if (depth === 0) {
        return source.slice(bodyStart + 1, index);
      }
    }
  }

  assert.fail(`unterminated body for: ${signature}`);
}

function sectionBetween(source, startNeedle, endNeedle) {
  const start = source.indexOf(startNeedle);
  assert.notEqual(start, -1, `missing section start: ${startNeedle}`);
  const end = source.indexOf(endNeedle, start);
  assert.notEqual(end, -1, `missing section end: ${endNeedle}`);
  return source.slice(start, end);
}

assert.match(
  lifecycleSource,
  /pub async fn show_main_window_async<R:\s*Runtime>\(/,
  'lifecycle should expose an async-safe main window focus path',
);
assert.match(
  lifecycleSource,
  /pub async fn show_main_window_with_selected_job_async<R:\s*Runtime>\(/,
  'lifecycle should expose an async-safe selected-job focus path',
);
assert.match(
  lifecycleSource,
  /fn ensure_main_window_with_restored_state<R:\s*Runtime>\(/,
  'main window creation should share a helper that receives preloaded restored state',
);

for (const signature of [
  'pub async fn show_main_window_async',
  'pub async fn show_main_window_with_selected_job_async',
]) {
  const body = bodyAfter(lifecycleSource, signature);
  assert.doesNotMatch(
    body,
    /main_window_state_sync|save_main_window_state_sync|settings_sync|blocking_read|blocking_write/,
    `${signature} must not call synchronous Tokio state accessors from async runtime context`,
  );
}

assert.match(
  windowsSource,
  /pub async fn focus_main_window_async\(app:\s*&AppHandle\)/,
  'windows module should provide an async focus helper for async callers',
);
assert.match(
  bodyAfter(windowsSource, 'pub async fn focus_main_window_async'),
  /crate::lifecycle::show_main_window_async\(app\)\.await/,
  'async main-window focus should await the async lifecycle path',
);
assert.match(
  windowsSource,
  /pub async fn focus_job_in_main_window_async\(app:\s*&AppHandle,\s*job_id:\s*&str\)/,
  'windows module should provide an async selected-job focus helper for async callers',
);
assert.match(
  bodyAfter(windowsSource, 'pub async fn focus_job_in_main_window_async'),
  /crate::lifecycle::show_main_window_with_selected_job_async\(app,\s*job_id\)\.await/,
  'async selected-job focus should await the async lifecycle path',
);

const openAppBranch = sectionBetween(ipcSource, '"open_app" | "show_window" => {', '"save_extension_settings" => {');
assert.match(
  openAppBranch,
  /focus_main_window_async\(&app\)\.await/,
  'duplicate-instance open_app/show_window handling should await async-safe focus',
);
assert.doesNotMatch(
  openAppBranch,
  /focus_main_window\(&app\)/,
  'duplicate-instance open_app/show_window handling must not call synchronous focus from the pipe task',
);

assert.match(
  bodyAfter(ipcSource, 'async fn run_prompt_download'),
  /focus_job_in_main_window_async\(app,\s*&job\.id\)\.await/,
  'async native-host duplicate prompt flow should await async selected-job focus',
);
assert.match(
  bodyAfter(commandsSource, 'pub async fn show_existing_download_prompt'),
  /focus_job_in_main_window_async\(&app,\s*&job_id\)\.await/,
  'async command duplicate prompt flow should await async selected-job focus',
);
assert.match(
  bodyAfter(commandsSource, 'pub async fn test_extension_handoff'),
  /focus_job_in_main_window_async\(&worker_app,\s*&job\.id\)\.await/,
  'async test handoff duplicate prompt flow should await async selected-job focus',
);
