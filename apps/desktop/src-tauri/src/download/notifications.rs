use super::*;
use crate::commands::{emit_notification_sound, NotificationSoundKind};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

static BULK_FAILURE_SOUND_ARCHIVES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

pub(super) async fn notify_download_completed(
    app: &AppHandle,
    state: &SharedState,
    final_path: &Path,
    is_bulk_member: bool,
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
    if let Some(kind) = download_completion_sound_kind(is_bulk_member) {
        emit_notification_sound_if_enabled(app, state, kind).await;
    }
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
    emit_notification_sound_if_enabled(app, state, NotificationSoundKind::Success).await;
}

pub(super) async fn prepare_bulk_archive_sources(
    archive: BulkArchiveReady,
    seven_zip_path: Option<PathBuf>,
) -> Result<PreparedBulkArchive, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(seven_zip_path) = seven_zip_path {
            prepare_bulk_archive_sources_with_7zip(archive, seven_zip_path)
        } else {
            prepare_bulk_archive_sources_without_extraction(archive)
        }
    })
    .await
    .map_err(|error| format!("Could not prepare bulk archive task: {error}"))?
}

pub(super) async fn finish_prepared_bulk_archive(
    prepared: PreparedBulkArchive,
) -> Result<BulkArchiveCreateOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || finish_prepared_bulk_archive_sync(prepared))
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
    if should_emit_failure_sound_for_task(task) {
        emit_notification_sound_if_enabled(app, state, NotificationSoundKind::Failed).await;
    }
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

pub(crate) fn reset_bulk_failure_sound(archive_id: &str) {
    let mut played_archives = played_bulk_failure_sound_archives()
        .lock()
        .expect("bulk failure sound lock poisoned");
    reset_bulk_failure_sound_key(&mut played_archives, archive_id);
}

async fn emit_notification_sound_if_enabled(
    app: &AppHandle,
    state: &SharedState,
    kind: NotificationSoundKind,
) {
    if state.notification_sounds_enabled().await {
        emit_notification_sound(app, kind);
    }
}

fn download_completion_sound_kind(is_bulk_member: bool) -> Option<NotificationSoundKind> {
    (!is_bulk_member).then_some(NotificationSoundKind::Success)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NotificationFailureSoundKey {
    Single,
    BulkArchive(String),
}

fn notification_failure_sound_key(
    task: &crate::state::DownloadTask,
) -> NotificationFailureSoundKey {
    if task.is_bulk_member {
        if let Some(archive_id) = task.bulk_archive_id.as_deref() {
            return NotificationFailureSoundKey::BulkArchive(archive_id.to_string());
        }
    }

    NotificationFailureSoundKey::Single
}

fn should_emit_failure_sound_for_task(task: &crate::state::DownloadTask) -> bool {
    let mut played_archives = played_bulk_failure_sound_archives()
        .lock()
        .expect("bulk failure sound lock poisoned");
    should_emit_failure_sound(&mut played_archives, notification_failure_sound_key(task))
}

fn should_emit_failure_sound(
    played_bulk_archives: &mut HashSet<String>,
    key: NotificationFailureSoundKey,
) -> bool {
    match key {
        NotificationFailureSoundKey::Single => true,
        NotificationFailureSoundKey::BulkArchive(archive_id) => {
            played_bulk_archives.insert(archive_id)
        }
    }
}

fn reset_bulk_failure_sound_key(played_bulk_archives: &mut HashSet<String>, archive_id: &str) {
    played_bulk_archives.remove(archive_id);
}

fn played_bulk_failure_sound_archives() -> &'static Mutex<HashSet<String>> {
    BULK_FAILURE_SOUND_ARCHIVES.get_or_init(|| Mutex::new(HashSet::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn completion_sound_suppresses_bulk_member_successes() {
        assert_eq!(
            download_completion_sound_kind(false),
            Some(NotificationSoundKind::Success)
        );
        assert_eq!(download_completion_sound_kind(true), None);
    }

    #[test]
    fn failure_sound_coalesces_by_bulk_archive_until_reset() {
        let mut played_bulk_archives = HashSet::new();

        assert!(should_emit_failure_sound(
            &mut played_bulk_archives,
            NotificationFailureSoundKey::Single
        ));
        assert!(should_emit_failure_sound(
            &mut played_bulk_archives,
            NotificationFailureSoundKey::BulkArchive("bulk_1".into())
        ));
        assert!(!should_emit_failure_sound(
            &mut played_bulk_archives,
            NotificationFailureSoundKey::BulkArchive("bulk_1".into())
        ));
        assert!(should_emit_failure_sound(
            &mut played_bulk_archives,
            NotificationFailureSoundKey::BulkArchive("bulk_2".into())
        ));

        reset_bulk_failure_sound_key(&mut played_bulk_archives, "bulk_1");
        assert!(should_emit_failure_sound(
            &mut played_bulk_archives,
            NotificationFailureSoundKey::BulkArchive("bulk_1".into())
        ));
    }
}
