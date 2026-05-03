use super::*;

pub(super) async fn notify_download_completed(
    app: &AppHandle,
    state: &SharedState,
    final_path: &Path,
) {
    let file_name = final_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Download completed");

    notify(
        app,
        state,
        "Download completed",
        &format!("{file_name} is ready."),
    )
    .await;
}

pub(super) async fn notify_bulk_archive_completed(
    app: &AppHandle,
    state: &SharedState,
    final_path: &Path,
) {
    let file_name = final_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Bulk archive");

    notify(
        app,
        state,
        "Bulk archive created",
        &format!("{file_name} is ready."),
    )
    .await;
}

pub(super) async fn create_bulk_archive(archive: BulkArchiveReady) -> Result<PathBuf, String> {
    tauri::async_runtime::spawn_blocking(move || create_bulk_archive_sync(archive))
        .await
        .map_err(|error| format!("Could not create bulk archive task: {error}"))?
}

pub(super) async fn notify_download_failure(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
    error: Option<&str>,
) {
    let fallback = task
        .target_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let body = error
        .map(|message| format!("{fallback}: {message}"))
        .unwrap_or_else(|| format!("{fallback} failed."));

    notify(app, state, "Download failed", &body).await;
}

pub(super) async fn notify(app: &AppHandle, state: &SharedState, title: &str, body: &str) {
    if !state.notifications_enabled().await {
        return;
    }

    let notification = app.notification();
    if matches!(notification.permission_state(), Ok(PermissionState::Prompt)) {
        let _ = notification.request_permission();
    }

    if !matches!(
        notification.permission_state(),
        Ok(PermissionState::Granted)
    ) {
        return;
    }

    let _ = notification.builder().title(title).body(body).show();
}
