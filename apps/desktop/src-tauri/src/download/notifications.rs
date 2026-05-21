use super::*;
use crate::commands::NotificationSoundKind;
use std::collections::HashSet;
#[cfg(test)]
use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

static BULK_FAILURE_SOUND_ARCHIVES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static BULK_FINALIZATION_IO_GOVERNOR: OnceLock<Arc<Semaphore>> = OnceLock::new();

pub(super) async fn notify_download_completed<A: DownloadUi>(
    app: &A,
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

pub(super) async fn notify_bulk_archive_completed<A: DownloadUi>(
    app: &A,
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

pub(super) async fn prepare_bulk_archive_sources_with_progress(
    archive: BulkArchiveReady,
    seven_zip_path: Option<PathBuf>,
    progress: BulkFinalizeProgressReporter,
) -> Result<PreparedBulkArchive, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(seven_zip_path) = seven_zip_path {
            prepare_bulk_archive_sources_with_7zip_and_progress(archive, seven_zip_path, progress)
        } else {
            prepare_bulk_archive_sources_without_extraction(archive)
        }
    })
    .await
    .map_err(|error| format!("Could not prepare bulk archive task: {error}"))?
}

pub(super) async fn plan_bulk_archive_finalization(
    archive: BulkArchiveReady,
) -> Result<BulkFinalizationPlan, String> {
    tauri::async_runtime::spawn_blocking(move || bulk_finalization_plan(&archive))
        .await
        .map_err(|error| format!("Could not plan bulk archive task: {error}"))?
}

pub(super) async fn finish_prepared_bulk_archive_with_progress(
    prepared: PreparedBulkArchive,
    progress: BulkFinalizeProgressReporter,
) -> Result<BulkArchiveCreateOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        finish_prepared_bulk_archive_sync_with_progress(prepared, progress)
    })
    .await
    .map_err(|error| format!("Could not create bulk archive task: {error}"))?
}

pub(super) async fn acquire_bulk_finalization_io_permit() -> OwnedSemaphorePermit {
    bulk_finalization_io_governor()
        .clone()
        .acquire_owned()
        .await
        .expect("bulk finalization I/O governor should not be closed")
}

#[cfg(test)]
pub(super) async fn with_bulk_finalization_io_permit<F, T>(future: F) -> T
where
    F: Future<Output = T>,
{
    let _permit = acquire_bulk_finalization_io_permit().await;
    future.await
}

fn bulk_finalization_io_governor() -> &'static Arc<Semaphore> {
    BULK_FINALIZATION_IO_GOVERNOR.get_or_init(|| Arc::new(Semaphore::new(1)))
}

pub(super) async fn notify_download_failure<A: DownloadUi>(
    app: &A,
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

pub(super) async fn notify<A: DownloadUi>(app: &A, state: &SharedState, title: &str, body: &str) {
    if !state.notifications_enabled().await {
        return;
    }

    app.show_notification(title, body);
}

pub(crate) fn reset_bulk_failure_sound(archive_id: &str) {
    let mut played_archives = played_bulk_failure_sound_archives()
        .lock()
        .expect("bulk failure sound lock poisoned");
    reset_bulk_failure_sound_key(&mut played_archives, archive_id);
}

async fn emit_notification_sound_if_enabled(
    app: &impl DownloadUi,
    state: &SharedState,
    kind: NotificationSoundKind,
) {
    if state.notification_sounds_enabled().await {
        app.emit_notification_sound(kind);
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

    #[tokio::test]
    async fn bulk_finalization_io_governor_allows_one_heavy_task_at_a_time() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use tokio::sync::Barrier;

        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(2));

        let first = {
            let active = active.clone();
            let max_active = max_active.clone();
            let start = start.clone();
            tokio::spawn(async move {
                start.wait().await;
                with_bulk_finalization_io_permit(async move {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                })
                .await;
            })
        };
        let second = {
            let active = active.clone();
            let max_active = max_active.clone();
            let start = start.clone();
            tokio::spawn(async move {
                start.wait().await;
                with_bulk_finalization_io_permit(async move {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                })
                .await;
            })
        };

        first.await.unwrap();
        second.await.unwrap();

        assert_eq!(
            max_active.load(Ordering::SeqCst),
            1,
            "bulk finalization governor should serialize heavy finalizers by default"
        );
    }
}
