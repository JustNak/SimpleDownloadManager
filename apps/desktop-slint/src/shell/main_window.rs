use crate::MainWindow;
use i_slint_backend_winit::WinitWindowAccessor;
use simple_download_manager_desktop_core::state::SharedState;
use simple_download_manager_desktop_core::storage::{MainWindowState, StartupLaunchMode};
use slint::{
    CloseRequestResponse, ComponentHandle, PhysicalPosition, PhysicalSize, WindowPosition,
    WindowSize as SlintWindowSize,
};

use super::WindowSize;

pub const MAIN_WINDOW_TITLE: &str = "Simple Download Manager";
pub const MAIN_WINDOW_WIDTH: u32 = 1360;
pub const MAIN_WINDOW_HEIGHT: u32 = 860;
pub const MAIN_WINDOW_MIN_WIDTH: u32 = 1360;
pub const MAIN_WINDOW_MIN_HEIGHT: u32 = 720;
pub const TITLEBAR_HEIGHT: u32 = 44;
pub const TITLEBAR_TITLE: &str = "Download Manager";
pub const AUTOSTART_ARG: &str = "--autostart";
pub const POST_UPDATE_ARG: &str = "--post-update";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainWindowConfig {
    pub title: &'static str,
    pub preferred_size: WindowSize,
    pub minimum_size: WindowSize,
    pub frameless: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseToTrayAction {
    HideWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlebarAction {
    StartDrag,
    ToggleMaximize,
    Minimize,
    CloseToTray,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlebarControl {
    Minimize,
    Maximize,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupWindowAction {
    Show,
    KeepHidden,
}

pub fn main_window_config() -> MainWindowConfig {
    MainWindowConfig {
        title: MAIN_WINDOW_TITLE,
        preferred_size: WindowSize {
            width: MAIN_WINDOW_WIDTH,
            height: MAIN_WINDOW_HEIGHT,
        },
        minimum_size: WindowSize {
            width: MAIN_WINDOW_MIN_WIDTH,
            height: MAIN_WINDOW_MIN_HEIGHT,
        },
        frameless: true,
    }
}

pub fn titlebar_action_for_drag_band(is_double_click: bool) -> TitlebarAction {
    if is_double_click {
        TitlebarAction::ToggleMaximize
    } else {
        TitlebarAction::StartDrag
    }
}

pub fn titlebar_action_for_control(control: TitlebarControl) -> TitlebarAction {
    match control {
        TitlebarControl::Minimize => TitlebarAction::Minimize,
        TitlebarControl::Maximize => TitlebarAction::ToggleMaximize,
        TitlebarControl::Close => TitlebarAction::CloseToTray,
    }
}

pub fn is_autostart_launch_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|argument| argument.as_ref() == AUTOSTART_ARG)
}

pub fn is_post_update_launch_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|argument| argument.as_ref() == POST_UPDATE_ARG)
}

pub fn startup_window_action(
    is_autostart_launch: bool,
    is_post_update_launch: bool,
    startup_launch_mode: StartupLaunchMode,
) -> StartupWindowAction {
    if is_post_update_launch
        || !(is_autostart_launch && startup_launch_mode == StartupLaunchMode::Tray)
    {
        StartupWindowAction::Show
    } else {
        StartupWindowAction::KeepHidden
    }
}

pub fn startup_window_action_from_args<I, S>(
    args: I,
    startup_launch_mode: StartupLaunchMode,
) -> StartupWindowAction
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut is_autostart_launch = false;
    let mut is_post_update_launch = false;
    for argument in args {
        let argument = argument.as_ref();
        is_autostart_launch |= argument == AUTOSTART_ARG;
        is_post_update_launch |= argument == POST_UPDATE_ARG;
    }

    startup_window_action(
        is_autostart_launch,
        is_post_update_launch,
        startup_launch_mode,
    )
}

pub fn current_startup_window_action(
    startup_launch_mode: StartupLaunchMode,
) -> StartupWindowAction {
    startup_window_action_from_args(std::env::args(), startup_launch_mode)
}

pub fn apply_startup_window_action(
    ui: &MainWindow,
    action: StartupWindowAction,
) -> Result<(), String> {
    match action {
        StartupWindowAction::Show => show_main_window(ui),
        StartupWindowAction::KeepHidden => Ok(()),
    }
}

pub fn initialize_main_window(ui: &MainWindow, state: &SharedState) {
    if let Some(window_state) = state.main_window_state_sync() {
        restore_main_window_state(ui, &window_state);
    }
    install_close_handler(ui, state.clone());
}

pub fn show_main_window(ui: &MainWindow) -> Result<(), String> {
    let window = ui.window();
    window.set_minimized(false);
    window.show().map_err(|error| error.to_string())?;
    focus_main_window(ui);
    Ok(())
}

pub fn hide_main_window(ui: &MainWindow) -> Result<(), String> {
    ui.window().hide().map_err(|error| error.to_string())
}

pub fn minimize_main_window(ui: &MainWindow) {
    ui.window().set_minimized(true);
}

pub fn toggle_main_window_maximized(ui: &MainWindow) -> bool {
    let window = ui.window();
    let maximized = !window.is_maximized();
    window.set_maximized(maximized);
    ui.set_main_window_maximized(maximized);
    maximized
}

pub fn focus_main_window(ui: &MainWindow) {
    let window = ui.window();
    let _ = window.with_winit_window(|winit_window| winit_window.focus_window());
}

pub fn start_main_window_drag(ui: &MainWindow) -> Result<(), String> {
    ui.window()
        .with_winit_window(|winit_window| {
            winit_window
                .drag_window()
                .map_err(|error| format!("Could not start main window drag: {error}"))
        })
        .unwrap_or(Ok(()))
}

pub fn close_main_window_to_tray(ui: &MainWindow, state: &SharedState) -> Result<(), String> {
    persist_current_main_window_state(ui, state)?;
    hide_main_window(ui)
}

pub fn request_exit(ui: &MainWindow, state: &SharedState) -> Result<(), String> {
    persist_current_main_window_state(ui, state)?;
    slint::quit_event_loop().map_err(|error| error.to_string())
}

pub fn install_close_handler(ui: &MainWindow, state: SharedState) {
    let weak = ui.as_weak();
    ui.window().on_close_requested(move || {
        let Some(ui) = weak.upgrade() else {
            return CloseRequestResponse::HideWindow;
        };
        handle_main_window_close(&ui, &state)
    });
}

pub fn handle_main_window_close(ui: &MainWindow, state: &SharedState) -> CloseRequestResponse {
    if let Err(error) = persist_current_main_window_state(ui, state) {
        eprintln!("failed to save main window state before hiding: {error}");
    }
    CloseRequestResponse::HideWindow
}

pub fn close_to_tray_action() -> CloseToTrayAction {
    CloseToTrayAction::HideWindow
}

pub fn persist_current_main_window_state(
    ui: &MainWindow,
    state: &SharedState,
) -> Result<(), String> {
    state.save_main_window_state_sync(capture_main_window_state(ui))
}

pub fn capture_main_window_state(ui: &MainWindow) -> MainWindowState {
    let window = ui.window();
    main_window_state_from_parts(window.size(), window.position(), window.is_maximized())
}

pub fn restore_main_window_state(ui: &MainWindow, state: &MainWindowState) {
    let window = ui.window();
    if let Some(size) = persisted_state_size(state) {
        window.set_size(SlintWindowSize::Physical(size));
    }
    window.set_position(WindowPosition::Physical(persisted_state_position(state)));
    window.set_maximized(state.maximized);
    ui.set_main_window_maximized(state.maximized);
}

pub fn persisted_state_size(state: &MainWindowState) -> Option<PhysicalSize> {
    if state.width == 0 || state.height == 0 {
        return None;
    }

    Some(PhysicalSize::new(state.width, state.height))
}

pub fn persisted_state_position(state: &MainWindowState) -> PhysicalPosition {
    PhysicalPosition::new(state.x, state.y)
}

pub fn main_window_state_from_parts(
    size: PhysicalSize,
    position: PhysicalPosition,
    maximized: bool,
) -> MainWindowState {
    MainWindowState {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
        maximized,
    }
}
