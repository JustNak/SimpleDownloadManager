use crate::commands::TauriShellServices;
use crate::state::SharedState;
use crate::storage::{TorrentInfo, TorrentSettings};
use simple_download_manager_desktop_core::transfer as core_transfer;
use tauri::AppHandle;

pub fn schedule_downloads(app: AppHandle, state: SharedState) {
    let shell = core_transfer::TransferShell::new(TauriShellServices::new(app));
    tauri::async_runtime::spawn(async move {
        if let Err(error) = core_transfer::schedule_downloads(shell, state).await {
            eprintln!("failed to claim queued jobs: {error}");
        }
    });
}

pub fn apply_torrent_runtime_settings(settings: &TorrentSettings) {
    core_transfer::apply_torrent_runtime_settings(settings);
}

pub async fn forget_torrent_session_for_restart(
    state: &SharedState,
    torrent: &TorrentInfo,
) -> Result<(), String> {
    core_transfer::forget_torrent_session_for_restart(state, torrent).await
}

pub async fn forget_known_torrent_sessions(torrents: &[TorrentInfo]) -> Result<(), String> {
    core_transfer::forget_known_torrent_sessions(torrents).await
}

pub async fn schedule_external_reseed(app: AppHandle, state: SharedState, id: String) {
    let shell = core_transfer::TransferShell::new(TauriShellServices::new(app));
    core_transfer::schedule_external_reseed(shell, state, id).await;
}
