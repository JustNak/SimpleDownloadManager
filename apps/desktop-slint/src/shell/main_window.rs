use crate::MainWindow;
use simple_download_manager_desktop_core::state::SharedState;
use simple_download_manager_desktop_core::storage::MainWindowState;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainWindowConfig {
    pub title: &'static str,
    pub preferred_size: WindowSize,
    pub minimum_size: WindowSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseToTrayAction {
    HideWindow,
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
    window.show().map_err(|error| error.to_string())
}

pub fn hide_main_window(ui: &MainWindow) -> Result<(), String> {
    ui.window().hide().map_err(|error| error.to_string())
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
