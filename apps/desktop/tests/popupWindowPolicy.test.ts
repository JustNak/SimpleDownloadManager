import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const windowsSource = readFileSync(new URL('../src-tauri/src/windows.rs', import.meta.url), 'utf8');

assert.equal(
  windowsSource.match(/return show_existing_popup_window\(&window\);/g)?.length,
  4,
  'download prompt, normal progress, torrent progress, and batch progress reuse paths should restore existing popups through one helper',
);

assert.match(
  windowsSource,
  /fn show_existing_popup_window\(window: &WebviewWindow\) -> Result<\(\), String> \{[\s\S]*window\.unminimize\(\)\.map_err\(\|error\| error\.to_string\(\)\)\?;[\s\S]*window\.show\(\)\.map_err\(\|error\| error\.to_string\(\)\)\?;[\s\S]*window\.set_focus\(\)\.map_err\(\|error\| error\.to_string\(\)\)/,
  'existing popup restore helper should unminimize before showing and focusing the window',
);

assert.match(
  windowsSource,
  /fn download_prompt_window_policy\(\) -> DownloadPromptWindowPolicy \{[\s\S]*minimizable:\s*true,[\s\S]*always_on_top:\s*true,/,
  'download prompt policy should explicitly remain minimizable and always on top',
);

assert.match(
  windowsSource,
  /WebviewWindowBuilder::new\([\s\S]*DOWNLOAD_PROMPT_WINDOW[\s\S]*\.minimizable\(policy\.minimizable\)[\s\S]*\.always_on_top\(policy\.always_on_top\)/,
  'download prompt builder should use the explicit prompt shell policy',
);

assert.match(
  windowsSource,
  /fn progress_window_policy\(\) -> ProgressWindowPolicy \{[\s\S]*minimizable:\s*true,[\s\S]*always_on_top:\s*false,/,
  'progress popup policy should keep progress windows minimizable and not always on top',
);
