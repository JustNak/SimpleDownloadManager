import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const tauriConfig = JSON.parse(
  await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
);
const lifecycleSource = await readFile(new URL('../src-tauri/src/lifecycle.rs', import.meta.url), 'utf8');
const mainSource = await readFile(new URL('../src-tauri/src/main.rs', import.meta.url), 'utf8');
const windowsSource = await readFile(new URL('../src-tauri/src/windows.rs', import.meta.url), 'utf8');
const appSource = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');

assert.deepEqual(
  tauriConfig.app.windows,
  [],
  'main webview should not be created from tauri.conf.json at app startup',
);

assert.match(
  lifecycleSource,
  /WebviewWindowBuilder::new\(\s*app,\s*config\.label,\s*WebviewUrl::App\(main_window_url\(/s,
  'main webview should be lazily created by lifecycle with a selectable URL',
);

assert.match(
  lifecycleSource,
  /window\.destroy\(\)/,
  'closing the main window should destroy the WebView2 webview instead of hiding it',
);

assert.doesNotMatch(
  mainSource,
  /\.run\(tauri::generate_context!\(\)\)/,
  'desktop runtime should not use Builder::run shorthand because it cannot prevent implicit last-window exits',
);

assert.match(
  mainSource,
  /\.build\(tauri::generate_context!\(\)\)[\s\S]*\.run\(\|_app_handle,\s*event\|\s*lifecycle::handle_run_event\(event\)\)/,
  'desktop runtime should install a RunEvent handler that can keep the tray alive after destroying the main webview',
);

assert.match(
  lifecycleSource,
  /RunEvent::ExitRequested\s*\{\s*code,\s*api,\s*\.\.[\s\S]*should_prevent_exit_request\(code\)[\s\S]*api\.prevent_exit\(\)/,
  'implicit Tauri exit requests should be prevented so close-to-tray keeps the process alive',
);

assert.match(
  lifecycleSource,
  /should_prevent_exit_request\(exit_code:\s*Option<i32>\)[\s\S]*exit_code\.is_none\(\)/,
  'explicit app.exit/restart exit codes should remain allowed while implicit last-window exits are blocked',
);

assert.match(
  lifecycleSource,
  /should_create_main_window_on_startup/,
  'autostart tray launches should be able to skip main webview creation',
);

assert.match(
  windowsSource,
  /crate::lifecycle::show_main_window\(app\)/,
  'tray/native-host focus should delegate to lifecycle so a destroyed main webview is recreated',
);

assert.match(
  windowsSource,
  /crate::lifecycle::show_main_window_with_selected_job\(app,\s*job_id\)/,
  'job focus should create the main webview with the selected job encoded in the URL',
);

assert.match(
  appSource,
  /initialSelectedJobIdFromSearch\(window\.location\.search\)/,
  'main App should read the initial selected job from the recreated window URL',
);
