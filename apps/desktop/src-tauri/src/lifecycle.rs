use crate::state::SharedState;
use crate::storage::{MainWindowState, Settings, StartupLaunchMode};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    App, AppHandle, Manager, PhysicalPosition, PhysicalSize, Position, Runtime, Size,
    WebviewWindow, Window, WindowEvent,
};

#[cfg(windows)]
use winreg::enums::HKEY_CURRENT_USER;
#[cfg(windows)]
use winreg::RegKey;

pub const MAIN_WINDOW_LABEL: &str = "main";
const AUTOSTART_ARG: &str = "--autostart";
const TRAY_MENU_OPEN: &str = "open";
const TRAY_MENU_EXIT: &str = "exit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainWindowStatePolicy {
    pub size: bool,
    pub position: bool,
    pub maximized: bool,
    pub visible: bool,
}

pub fn initialize_app_lifecycle(app: &mut App, state: &SharedState) -> Result<(), String> {
    setup_tray(app)?;

    let settings = state.settings_sync();
    if let Err(error) = sync_autostart_setting(settings.start_on_startup) {
        eprintln!("failed to synchronize autostart setting: {error}");
    }

    initialize_main_window(app.handle(), state, &settings)
}

pub fn main_window_state_policy() -> MainWindowStatePolicy {
    MainWindowStatePolicy {
        size: true,
        position: true,
        maximized: true,
        visible: false,
    }
}

pub fn is_autostart_launch() -> bool {
    is_autostart_launch_from_args(std::env::args())
}

pub fn is_autostart_launch_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|argument| argument.as_ref() == AUTOSTART_ARG)
}

pub fn should_show_main_window_on_startup(
    is_autostart_launch: bool,
    startup_launch_mode: StartupLaunchMode,
) -> bool {
    !(is_autostart_launch && startup_launch_mode == StartupLaunchMode::Tray)
}

pub fn sync_autostart_setting(start_on_startup: bool) -> Result<(), String> {
    sync_autostart_setting_for_command(start_on_startup, AUTOSTART_ARG)
}

fn sync_autostart_setting_for_command(
    start_on_startup: bool,
    autostart_arg: &str,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let registry_key = startup_run_registry_key()?;
        if start_on_startup {
            let command = autostart_command(autostart_arg)?;
            registry_key
                .set_value(startup_registry_value_name(), &command)
                .map_err(|error| format!("Could not enable startup registration: {error}"))?;
        } else {
            match registry_key.delete_value(startup_registry_value_name()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!("Could not disable startup registration: {error}"));
                }
            }
        }

        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = start_on_startup;
        let _ = autostart_arg;
        Err("Startup registration is only supported on Windows in this build.".into())
    }
}

pub fn initialize_main_window<R: Runtime>(
    app: &AppHandle<R>,
    state: &SharedState,
    settings: &Settings,
) -> Result<(), String> {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return Ok(());
    };

    if let Some(window_state) = state.main_window_state_sync() {
        restore_main_window_state(&window, &window_state);
    }

    if should_show_main_window_on_startup(is_autostart_launch(), settings.startup_launch_mode) {
        show_main_window(app)
    } else {
        hide_main_window_to_tray(app)
    }
}

pub fn handle_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW_LABEL {
        return;
    }

    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };

    api.prevent_close();
    if let Some(state) = window.app_handle().try_state::<SharedState>() {
        match capture_window_state(window) {
            Ok(window_state) => {
                if let Err(error) = state.save_main_window_state_sync(window_state) {
                    eprintln!("failed to save main window state before hiding: {error}");
                }
            }
            Err(error) => eprintln!("failed to capture main window state before hiding: {error}"),
        }
    }
    if let Err(error) = window.set_skip_taskbar(true) {
        eprintln!("failed to remove main window from taskbar: {error}");
    }
    if let Err(error) = window.hide() {
        eprintln!("failed to hide main window to tray: {error}");
    }
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window
            .set_skip_taskbar(false)
            .map_err(|error| error.to_string())?;
        let _ = window.unminimize();
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
    }

    Ok(())
}

pub fn hide_main_window_to_tray<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        save_main_window_state(app)?;
        window
            .set_skip_taskbar(true)
            .map_err(|error| error.to_string())?;
        window.hide().map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn setup_tray(app: &mut App) -> Result<(), String> {
    let open_item = MenuItem::with_id(app, TRAY_MENU_OPEN, "Open", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let exit_item = MenuItem::with_id(app, TRAY_MENU_EXIT, "Exit", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let menu =
        Menu::with_items(app, &[&open_item, &exit_item]).map_err(|error| error.to_string())?;
    let icon = app
        .default_window_icon()
        .ok_or_else(|| "Could not load the default application icon.".to_string())?
        .clone();

    TrayIconBuilder::new()
        .tooltip("Simple Download Manager")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            TRAY_MENU_OPEN => {
                if let Err(error) = show_main_window(app) {
                    eprintln!("failed to open main window from tray: {error}");
                }
            }
            TRAY_MENU_EXIT => exit_application(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Err(error) = show_main_window(tray.app_handle()) {
                    eprintln!("failed to open main window from tray click: {error}");
                }
            }
        })
        .build(app)
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn exit_application<R: Runtime>(app: &AppHandle<R>) {
    if let Err(error) = save_main_window_state(app) {
        eprintln!("failed to save main window state before exit: {error}");
    }
    app.exit(0);
}

fn save_main_window_state<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return Ok(());
    };
    let Some(state) = app.try_state::<SharedState>() else {
        return Ok(());
    };

    let window_state = capture_webview_window_state(&window)?;
    state.save_main_window_state_sync(window_state)
}

fn capture_window_state<R: Runtime>(window: &Window<R>) -> Result<MainWindowState, String> {
    let size = window.inner_size().map_err(|error| error.to_string())?;
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let maximized = window.is_maximized().map_err(|error| error.to_string())?;

    Ok(MainWindowState {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
        maximized,
    })
}

fn capture_webview_window_state<R: Runtime>(
    window: &WebviewWindow<R>,
) -> Result<MainWindowState, String> {
    let size = window.inner_size().map_err(|error| error.to_string())?;
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let maximized = window.is_maximized().map_err(|error| error.to_string())?;

    Ok(MainWindowState {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
        maximized,
    })
}

fn restore_main_window_state<R: Runtime>(window: &WebviewWindow<R>, state: &MainWindowState) {
    if state.width > 0 && state.height > 0 {
        let _ = window.set_size(Size::Physical(PhysicalSize::new(state.width, state.height)));
    }
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(state.x, state.y)));
    if state.maximized {
        let _ = window.maximize();
    } else {
        let _ = window.unmaximize();
    }
}

#[cfg(windows)]
fn startup_run_registry_key() -> Result<RegKey, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .map_err(|error| format!("Could not open startup registry key: {error}"))?;
    Ok(key)
}

#[cfg(windows)]
fn startup_registry_value_name() -> &'static str {
    "Simple Download Manager"
}

#[cfg(windows)]
fn autostart_command(autostart_arg: &str) -> Result<String, String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("Could not locate app binary: {error}"))?;
    Ok(format!("\"{}\" {autostart_arg}", executable.display()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn detects_autostart_launch_argument() {
        assert!(super::is_autostart_launch_from_args([
            "simple-download-manager.exe",
            "--autostart",
        ]));
        assert!(!super::is_autostart_launch_from_args([
            "simple-download-manager.exe",
            "--not-autostart",
        ]));
        assert!(!super::is_autostart_launch_from_args([
            "simple-download-manager.exe",
            "--flag=--autostart",
        ]));
    }

    #[test]
    fn tray_startup_hides_only_autostart_launches() {
        assert!(super::should_show_main_window_on_startup(
            false,
            crate::storage::StartupLaunchMode::Tray,
        ));
        assert!(!super::should_show_main_window_on_startup(
            true,
            crate::storage::StartupLaunchMode::Tray,
        ));
        assert!(super::should_show_main_window_on_startup(
            true,
            crate::storage::StartupLaunchMode::Open,
        ));
    }

    #[test]
    fn window_state_tracks_geometry_without_visibility() {
        let policy = super::main_window_state_policy();

        assert!(policy.size);
        assert!(policy.position);
        assert!(policy.maximized);
        assert!(!policy.visible);
    }

    #[test]
    fn autostart_command_quotes_executable_and_appends_flag() {
        let command = super::autostart_command("--autostart").expect("command should resolve");

        assert!(command.starts_with('"'));
        assert!(command.ends_with("\" --autostart"));
    }
}
