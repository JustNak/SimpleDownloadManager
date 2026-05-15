import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const windowsSource = readFileSync(new URL('../src-tauri/src/windows.rs', import.meta.url), 'utf8');
const commandsSource = readFileSync(new URL('../src-tauri/src/commands/mod.rs', import.meta.url), 'utf8');
const mainRsSource = readFileSync(new URL('../src-tauri/src/main.rs', import.meta.url), 'utf8');

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
  /fn popup_window_url\([\s\S]*appearance: Option<&PopupAppearanceQuery>[\s\S]*append_pair\("window"[\s\S]*append_appearance_query_params/,
  'native popup URLs should be built through one helper that always includes appearance parameters',
);

assert.match(
  windowsSource,
  /async fn popup_window_appearance_query\(app: &AppHandle\) -> Option<PopupAppearanceQuery>[\s\S]*state\.settings\(\)\.await[\s\S]*theme: settings\.theme[\s\S]*accent_color: settings\.accent_color/,
  'popup appearance query should load the current desktop theme and accent color through async state access',
);

assert.match(
  windowsSource,
  /struct PopupAppearanceQuery \{[\s\S]*theme: Theme,[\s\S]*accent_color: String,[\s\S]*native_theme: TauriTheme,[\s\S]*background_color: Color,/,
  'popup appearance should include both renderer URL appearance and native shell appearance',
);

assert.match(
  windowsSource,
  /fn apply_popup_native_appearance<'a, R: Runtime, M: Manager<R>>\([\s\S]*WebviewWindowBuilder<'a, R, M>[\s\S]*\.theme\(Some\(appearance\.native_theme\)\)[\s\S]*\.background_color\(appearance\.background_color\)/,
  'new popup builders should apply native theme and webview background before creation',
);

assert.equal(
  windowsSource.match(/\.visible\(false\)/g)?.length,
  4,
  'download prompt, HTTP progress, torrent progress, and batch progress should be created hidden until the renderer is themed',
);

assert.equal(
  windowsSource.match(/schedule_popup_ready_timeout\(&window,\s*geometry\);/g)?.length,
  4,
  'each new popup path should schedule a bounded hidden-window reveal fallback',
);

assert.match(
  windowsSource,
  /const POPUP_READY_TIMEOUT: Duration = Duration::from_millis\(1500\);/,
  'popup ready fallback should be a safety net now that renderers own normal readiness',
);

assert.match(
  windowsSource,
  /fn popup_initialization_script\(appearance: Option<&PopupAppearanceQuery>\) -> Option<String>[\s\S]*document\.documentElement[\s\S]*classList\.toggle\('dark'[\s\S]*--color-primary[\s\S]*--color-selected/,
  'native popup builders should carry an inline first-frame appearance initialization script',
);

assert.equal(
  windowsSource.match(/\.initialization_script\(popup_init_script\.clone\(\)\)/g)?.length,
  4,
  'download prompt, HTTP progress, torrent progress, and batch progress should install the native appearance init script',
);

assert.match(
  windowsSource,
  /pub fn mark_popup_ready<R: Runtime>\(window: &WebviewWindow<R>\) -> Result<\(\), String>[\s\S]*reveal_popup_window\(/,
  'renderer-ready popups should reveal through a shared native popup ready helper',
);

assert.match(
  windowsSource,
  /fn resolve_system_native_theme\(app: &AppHandle\) -> TauriTheme[\s\S]*webview_windows\(\)[\s\S]*system_theme_from_registry/,
  'system popup theme should prefer an existing Tauri window and fall back to the OS registry on Windows',
);

assert.match(
  windowsSource,
  /fn append_appearance_query_params\([\s\S]*appearance: Option<&PopupAppearanceQuery>[\s\S]*append_pair\("theme"[\s\S]*append_pair\(\s*"accentColor",\s*&normalize_popup_accent_color\(&appearance\.accent_color\)/,
  'native popup URLs should carry supplied theme and a normalized accent color before the webview paints',
);

assert.doesNotMatch(
  windowsSource,
  /settings_sync|blocking_read|blocking_write/,
  'popup window code must not use synchronous Tokio state accessors',
);

for (const staleUrlPattern of [
  /WebviewUrl::App\("index\.html\?window=download-prompt"\.into\(\)\)/,
  /format!\("index\.html\?window=download-progress&jobId=\{job_id\}"\)/,
  /format!\("index\.html\?window=torrent-progress&jobId=\{job_id\}"\)/,
  /format!\("index\.html\?window=batch-progress&batchId=\{batch_id\}"\)/,
]) {
  assert.doesNotMatch(windowsSource, staleUrlPattern, 'popup builders should not use raw unthemed index.html popup URLs');
}

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

assert.match(
  commandsSource,
  /pub fn mark_popup_ready\(window:\s*WebviewWindow\) -> Result<\(\), String>[\s\S]*crate::windows::mark_popup_ready\(&window\)/,
  'renderer popup readiness should be exposed through a narrow Tauri command',
);

assert.match(
  mainRsSource,
  /commands::mark_popup_ready/,
  'popup readiness command should be registered with the Tauri invoke handler',
);
