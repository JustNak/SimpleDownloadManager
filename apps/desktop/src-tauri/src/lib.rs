pub(crate) mod archive_parts;
#[cfg(any(test, debug_assertions))]
pub mod bulk_bench;
pub mod commands;
pub mod download;
pub mod hosters;
#[cfg(any(test, debug_assertions))]
pub mod http_bench;
pub mod iced_app;
pub mod ipc;
pub mod lifecycle;
pub mod prompts;
pub(crate) mod runtime;
pub mod sidecars;
pub mod state;
pub mod storage;
pub mod torrent;
#[cfg(any(test, debug_assertions))]
pub mod torrent_bench;
pub mod updates;
pub mod windows;
