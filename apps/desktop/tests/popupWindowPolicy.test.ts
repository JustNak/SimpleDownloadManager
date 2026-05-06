import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const windowsSource = readFileSync(new URL('../src-tauri/src/windows.rs', import.meta.url), 'utf8');

assert.equal(
  windowsSource.match(/return show_existing_popup_window\(&window,\s*.*_window_geometry\(\)\);/g)?.length,
  4,
  'download prompt, normal progress, torrent progress, and batch progress reuse paths should restore existing popups through one geometry-aware helper',
);

assert.match(
  windowsSource,
  /fn show_existing_popup_window\(\s*window: &WebviewWindow,\s*geometry: PopupWindowGeometry,\s*\) -> Result<\(\), String> \{[\s\S]*let _ = window\.unminimize\(\);[\s\S]*window\.show\(\)\.map_err\(\|error\| error\.to_string\(\)\)\?;[\s\S]*repair_popup_window_bounds\(window,\s*geometry\);[\s\S]*window\.set_focus\(\)\.map_err\(\|error\| error\.to_string\(\)\)/,
  'existing popup restore helper should tolerate unminimize failures, show, repair bounds, then focus the window',
);

assert.match(
  windowsSource,
  /pub fn handle_popup_window_event<R: Runtime>\(window: &Window<R>, event: &WindowEvent\)[\s\S]*WindowEvent::Focused\(true\)[\s\S]*repair_existing_popup_window\(.*PopupRestoreFocus::Preserve\)/,
  'popup focus events from Alt-Tab should repair popup bounds without forcing another focus call',
);

assert.match(
  windowsSource,
  /fn popup_window_geometry_for_label\(label: &str\) -> Option<PopupWindowGeometry>[\s\S]*DOWNLOAD_PROMPT_WINDOW[\s\S]*PROGRESS_WINDOW_PREFIX[\s\S]*TORRENT_PROGRESS_WINDOW_PREFIX[\s\S]*BATCH_PROGRESS_WINDOW_PREFIX/,
  'popup window labels should map to fixed geometry for reset and focus-event repair',
);

assert.match(
  windowsSource,
  /fn repair_popup_window_bounds<R: Runtime>[\s\S]*\.set_size\(Size::Logical\(LogicalSize::new[\s\S]*geometry\.width[\s\S]*geometry\.height[\s\S]*popup_rect_is_visible_on_any_monitor[\s\S]*\.set_position\(Position::Physical\(centered_popup_position/,
  'popup bounds repair should restore fixed size and recenter invalid or offscreen popup rectangles',
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
