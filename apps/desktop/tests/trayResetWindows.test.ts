import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const lifecycleSource = readFileSync(new URL('../src-tauri/src/lifecycle.rs', import.meta.url), 'utf8');

assert.match(
  lifecycleSource,
  /const TRAY_MENU_RESET_WINDOWS: &str = "reset-windows";/,
  'tray menu should define a stable Reset windows menu id',
);

assert.match(
  lifecycleSource,
  /let open_item = MenuItem::with_id[\s\S]*let reset_windows_item\s*=\s*MenuItem::with_id\([\s\S]*TRAY_MENU_RESET_WINDOWS,[\s\S]*"Reset windows"[\s\S]*let exit_item = MenuItem::with_id/,
  'tray menu should create Reset windows between Open and Exit',
);

assert.match(
  lifecycleSource,
  /Menu::with_items\(app,\s*&\[\s*&open_item,\s*&reset_windows_item,\s*&exit_item\s*\]\)/,
  'tray menu order should be Open, Reset windows, Exit',
);

assert.match(
  lifecycleSource,
  /TRAY_MENU_RESET_WINDOWS => \{[\s\S]*reset_windows\(app\)[\s\S]*\}/,
  'Reset windows tray command should dispatch to reset_windows',
);

assert.match(
  lifecycleSource,
  /pub fn reset_windows<R: Runtime>\(app: &AppHandle<R>\) -> Result<\(\), String>[\s\S]*reset_main_window\(app\)\?;[\s\S]*crate::windows::reset_popup_windows\(app\);/,
  'reset_windows should repair the main window and all live popup windows',
);

assert.match(
  lifecycleSource,
  /fn reset_main_window<R: Runtime>\(app: &AppHandle<R>\) -> Result<\(\), String>[\s\S]*window[\s\S]*\.set_size\(Size::Logical\(LogicalSize::new\(config\.width,\s*config\.height\)\)\)[\s\S]*window[\s\S]*\.set_position\(Position::Physical\(centered_main_window_position/,
  'main window reset should restore default size and recenter on a valid monitor',
);

assert.match(
  lifecycleSource,
  /state\.save_main_window_state_sync\(capture_webview_window_state\(&window\)\?\)/,
  'main window reset should persist the repaired main window state',
);
