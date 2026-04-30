pub mod commands;
pub mod download;
pub mod ipc;
pub mod lifecycle;
pub mod native_host;
pub mod prompts {
    pub use simple_download_manager_desktop_core::prompts::*;
}
pub mod state;
pub mod storage {
    pub use simple_download_manager_desktop_core::storage::*;
}
pub mod torrent;
pub mod updates;
pub mod windows;

pub use simple_download_manager_desktop_core as desktop_core;
