use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const TRAY_TOOLTIP: &str = "Simple Download Manager";
pub const TRAY_MENU_OPEN_ID: &str = "open";
pub const TRAY_MENU_EXIT_ID: &str = "exit";
pub const TRAY_MENU_OPEN_LABEL: &str = "Open";
pub const TRAY_MENU_EXIT_LABEL: &str = "Exit";
pub const TRAY_ICON_ID: &str = "simple-download-manager";

const FALLBACK_ICON_SIZE: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    OpenMainWindow,
    ExitApplication,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButtonState {
    Up,
    Down,
}

pub struct SystemTray {
    #[cfg(windows)]
    _tray_icon: tray_icon::TrayIcon,
}

pub fn action_for_menu_id(id: &str) -> Option<TrayAction> {
    match id {
        TRAY_MENU_OPEN_ID => Some(TrayAction::OpenMainWindow),
        TRAY_MENU_EXIT_ID => Some(TrayAction::ExitApplication),
        _ => None,
    }
}

pub fn action_for_mouse_click(
    button: MouseButton,
    button_state: MouseButtonState,
) -> Option<TrayAction> {
    if button == MouseButton::Left && button_state == MouseButtonState::Up {
        Some(TrayAction::OpenMainWindow)
    } else {
        None
    }
}

#[cfg(windows)]
pub fn create_system_tray(
    on_action: impl Fn(TrayAction) + Send + Sync + 'static,
) -> Result<SystemTray, String> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{TrayIconBuilder, TrayIconEvent};

    let on_action: Arc<dyn Fn(TrayAction) + Send + Sync> = Arc::new(on_action);
    let menu = Menu::new();
    let open_item = MenuItem::with_id(TRAY_MENU_OPEN_ID, TRAY_MENU_OPEN_LABEL, true, None);
    let exit_item = MenuItem::with_id(TRAY_MENU_EXIT_ID, TRAY_MENU_EXIT_LABEL, true, None);
    menu.append_items(&[&open_item, &exit_item])
        .map_err(|error| format!("Could not build tray menu: {error}"))?;

    let menu_handler = on_action.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let Some(action) = action_for_menu_id(event.id.as_ref()) else {
            return;
        };
        dispatch_tray_action(menu_handler.clone(), action);
    }));

    let click_handler = on_action;
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        let TrayIconEvent::Click {
            button,
            button_state,
            ..
        } = event
        else {
            return;
        };
        let Some(action) = action_for_mouse_click(
            mouse_button_from_tray_icon(button),
            mouse_state_from_tray_icon(button_state),
        ) else {
            return;
        };
        dispatch_tray_action(click_handler.clone(), action);
    }));

    let icon = load_tray_icon()?;
    let tray_icon = TrayIconBuilder::new()
        .with_id(TRAY_ICON_ID)
        .with_tooltip(TRAY_TOOLTIP)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_icon(icon)
        .build()
        .map_err(|error| format!("Could not create system tray icon: {error}"))?;

    Ok(SystemTray {
        _tray_icon: tray_icon,
    })
}

#[cfg(not(windows))]
pub fn create_system_tray(
    _on_action: impl Fn(TrayAction) + Send + Sync + 'static,
) -> Result<SystemTray, String> {
    Err("System tray is only supported on Windows in this build.".into())
}

#[cfg(windows)]
fn dispatch_tray_action(on_action: Arc<dyn Fn(TrayAction) + Send + Sync>, action: TrayAction) {
    let _ = slint::invoke_from_event_loop(move || on_action(action));
}

#[cfg(windows)]
fn mouse_button_from_tray_icon(button: tray_icon::MouseButton) -> MouseButton {
    match button {
        tray_icon::MouseButton::Left => MouseButton::Left,
        tray_icon::MouseButton::Right => MouseButton::Right,
        tray_icon::MouseButton::Middle => MouseButton::Middle,
    }
}

#[cfg(windows)]
fn mouse_state_from_tray_icon(state: tray_icon::MouseButtonState) -> MouseButtonState {
    match state {
        tray_icon::MouseButtonState::Up => MouseButtonState::Up,
        tray_icon::MouseButtonState::Down => MouseButtonState::Down,
    }
}

#[cfg(windows)]
fn load_tray_icon() -> Result<tray_icon::Icon, String> {
    if let Some(path) = resolve_tray_icon_path() {
        match tray_icon::Icon::from_path(&path, None) {
            Ok(icon) => return Ok(icon),
            Err(error) => eprintln!("failed to load tray icon from {}: {error}", path.display()),
        }
    }

    tray_icon::Icon::from_rgba(
        fallback_tray_icon_rgba(),
        FALLBACK_ICON_SIZE,
        FALLBACK_ICON_SIZE,
    )
    .map_err(|error| format!("Could not build fallback tray icon: {error}"))
}

#[cfg(windows)]
fn resolve_tray_icon_path() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let install_root = current_exe.parent()?;
    let bundled_candidate = install_root
        .join("resources")
        .join("icons")
        .join("icon.ico");
    if bundled_candidate.exists() {
        return Some(bundled_candidate);
    }

    for ancestor in install_root.ancestors() {
        for relative in [
            ["icons", "icon.ico"].as_slice(),
            ["src-tauri", "icons", "icon.ico"].as_slice(),
            ["apps", "desktop", "src-tauri", "icons", "icon.ico"].as_slice(),
        ] {
            let candidate = join_segments(ancestor, relative);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(windows)]
fn join_segments(root: &Path, segments: &[&str]) -> PathBuf {
    segments
        .iter()
        .fold(root.to_path_buf(), |path, segment| path.join(segment))
}

#[cfg(windows)]
fn fallback_tray_icon_rgba() -> Vec<u8> {
    let mut rgba = Vec::with_capacity((FALLBACK_ICON_SIZE * FALLBACK_ICON_SIZE * 4) as usize);
    for y in 0..FALLBACK_ICON_SIZE {
        for x in 0..FALLBACK_ICON_SIZE {
            let border =
                x == 0 || y == 0 || x == FALLBACK_ICON_SIZE - 1 || y == FALLBACK_ICON_SIZE - 1;
            if border {
                rgba.extend_from_slice(&[24, 31, 42, 255]);
            } else {
                rgba.extend_from_slice(&[52, 152, 219, 255]);
            }
        }
    }
    rgba
}
