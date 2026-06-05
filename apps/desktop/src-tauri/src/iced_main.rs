#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use simple_download_manager_desktop_backend::{iced_app, state};

fn main() -> iced::Result {
    let shared_state = match state::SharedState::new() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("failed to initialize app state: {error}");
            std::process::exit(1);
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to initialize iced bootstrap runtime");
    let snapshot = runtime.block_on(shared_state.snapshot());

    iced_app::run_iced_desktop(shared_state, snapshot)
}
