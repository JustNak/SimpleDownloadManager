use crate::state::SharedState;
use crate::storage::{DesktopSnapshot, DownloadJob, JobState, TransferKind};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const UPDATE_INSTALL_PROGRESS_EVENT: &str = "app://update-install-progress";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateMetadata {
    pub version: String,
    pub current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCommandError {
    pub code: &'static str,
    pub message: String,
}

impl UpdateCommandError {
    pub fn no_pending_update() -> Self {
        Self {
            code: "NO_PENDING_UPDATE",
            message: "There is no pending update to install.".into(),
        }
    }

    pub fn updater(error: impl std::fmt::Display) -> Self {
        Self {
            code: "UPDATER_ERROR",
            message: error.to_string(),
        }
    }

    pub fn bulk_download_active() -> Self {
        Self {
            code: "BULK_DOWNLOAD_ACTIVE",
            message: "Finish or pause active bulk downloads before installing the update.".into(),
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            code: "INTERNAL_ERROR",
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum UpdateInstallProgressEvent {
    Started { content_length: Option<u64> },
    Progress { chunk_length: usize },
    Finished,
}

#[derive(Default)]
pub struct PendingUpdateState {
    update: Mutex<Option<Update>>,
}

impl PendingUpdateState {
    fn replace(&self, update: Option<Update>) -> Result<(), UpdateCommandError> {
        let mut pending_update = self.update.lock().map_err(|error| {
            UpdateCommandError::internal(format!("Could not lock pending update: {error}"))
        })?;
        *pending_update = update;
        Ok(())
    }

    fn take(&self) -> Result<Option<Update>, UpdateCommandError> {
        let mut pending_update = self.update.lock().map_err(|error| {
            UpdateCommandError::internal(format!("Could not lock pending update: {error}"))
        })?;
        Ok(pending_update.take())
    }
}

pub fn metadata_for_update(update: Option<&Update>) -> Option<AppUpdateMetadata> {
    update.map(|update| AppUpdateMetadata {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        date: update.date.map(|date| date.to_string()),
        body: update.body.clone(),
    })
}

pub fn bulk_update_blocker_for_jobs(jobs: &[DownloadJob]) -> Option<String> {
    if jobs.iter().any(is_active_bulk_member) {
        return Some("bulk_download".into());
    }
    if jobs.iter().any(is_finalizing_bulk_archive) {
        return Some("bulk_archive".into());
    }
    None
}

pub fn ensure_no_bulk_update_blocker(snapshot: &DesktopSnapshot) -> Result<(), UpdateCommandError> {
    if bulk_update_blocker_for_jobs(&snapshot.jobs).is_some() {
        return Err(UpdateCommandError::bulk_download_active());
    }
    Ok(())
}

fn is_active_bulk_member(job: &DownloadJob) -> bool {
    job.transfer_kind == TransferKind::Http
        && job.bulk_archive.is_some()
        && matches!(
            job.state,
            JobState::Queued | JobState::Starting | JobState::Downloading
        )
}

fn is_finalizing_bulk_archive(job: &DownloadJob) -> bool {
    job.transfer_kind == TransferKind::Http
        && job
            .bulk_archive
            .as_ref()
            .is_some_and(|archive| archive.archive_status.is_finalizing())
}

#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    pending_update: State<'_, PendingUpdateState>,
) -> Result<Option<AppUpdateMetadata>, UpdateCommandError> {
    let update = app
        .updater()
        .map_err(UpdateCommandError::updater)?
        .check()
        .await
        .map_err(UpdateCommandError::updater)?;
    let metadata = metadata_for_update(update.as_ref());
    pending_update.replace(update)?;
    Ok(metadata)
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    pending_update: State<'_, PendingUpdateState>,
    state: State<'_, SharedState>,
) -> Result<(), UpdateCommandError> {
    let snapshot = state.snapshot().await;
    ensure_no_bulk_update_blocker(&snapshot)?;

    let Some(update) = pending_update.take()? else {
        return Err(UpdateCommandError::no_pending_update());
    };

    let progress_app = app.clone();
    let finished_app = app.clone();
    let mut emitted_started = false;

    update
        .download_and_install(
            move |chunk_length, content_length| {
                if !emitted_started {
                    emit_update_install_progress(
                        &progress_app,
                        UpdateInstallProgressEvent::Started { content_length },
                    );
                    emitted_started = true;
                }
                emit_update_install_progress(
                    &progress_app,
                    UpdateInstallProgressEvent::Progress { chunk_length },
                );
            },
            move || {
                emit_update_install_progress(&finished_app, UpdateInstallProgressEvent::Finished);
            },
        )
        .await
        .map_err(UpdateCommandError::updater)?;

    Ok(())
}

fn emit_update_install_progress(app: &AppHandle, event: UpdateInstallProgressEvent) {
    let _ = app.emit(UPDATE_INSTALL_PROGRESS_EVENT, event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{
        BulkArchiveInfo, BulkArchiveOutputKind, BulkArchiveStatus, ConnectionState, DownloadJob,
        JobState, ResumeSupport, Settings, TransferKind,
    };

    #[test]
    fn update_metadata_none_is_clean_no_update_result() {
        assert_eq!(metadata_for_update(None), None);
    }

    #[test]
    fn install_without_pending_update_returns_user_readable_error() {
        let error = UpdateCommandError::no_pending_update();

        assert_eq!(error.code, "NO_PENDING_UPDATE");
        assert_eq!(error.message, "There is no pending update to install.");
    }

    #[test]
    fn bulk_update_blocker_detects_active_members_and_archive_finalization() {
        let pending_member = download_job("job_1", JobState::Queued, BulkArchiveStatus::Pending);
        let paused_member = download_job("job_2", JobState::Paused, BulkArchiveStatus::Pending);
        let finalizing_archive =
            download_job("job_3", JobState::Completed, BulkArchiveStatus::Compressing);
        let completed_archive =
            download_job("job_4", JobState::Completed, BulkArchiveStatus::Completed);
        let canceled_member = download_job("job_5", JobState::Canceled, BulkArchiveStatus::Pending);
        let failed_member = download_job("job_6", JobState::Failed, BulkArchiveStatus::Pending);

        assert_eq!(
            bulk_update_blocker_for_jobs(&[pending_member]).as_deref(),
            Some("bulk_download")
        );
        assert_eq!(
            bulk_update_blocker_for_jobs(&[
                paused_member,
                completed_archive,
                canceled_member,
                failed_member
            ]),
            None
        );
        assert_eq!(
            bulk_update_blocker_for_jobs(&[finalizing_archive]).as_deref(),
            Some("bulk_archive")
        );
    }

    #[test]
    fn update_install_guard_rejects_active_bulk_work() {
        let snapshot = crate::storage::DesktopSnapshot {
            connection_state: ConnectionState::Connected,
            jobs: vec![download_job(
                "job_1",
                JobState::Downloading,
                BulkArchiveStatus::Pending,
            )],
            settings: Settings::default(),
        };

        let error = ensure_no_bulk_update_blocker(&snapshot).unwrap_err();

        assert_eq!(error.code, "BULK_DOWNLOAD_ACTIVE");
        assert_eq!(
            error.message,
            "Finish or pause active bulk downloads before installing the update."
        );
    }

    #[test]
    fn update_errors_serialize_for_frontend_display() {
        let error = UpdateCommandError::updater("network unavailable");

        let serialized = serde_json::to_value(error).expect("error should serialize");

        assert_eq!(serialized["code"], "UPDATER_ERROR");
        assert_eq!(serialized["message"], "network unavailable");
    }

    fn download_job(id: &str, state: JobState, archive_status: BulkArchiveStatus) -> DownloadJob {
        DownloadJob {
            id: id.into(),
            url: format!("https://example.com/{id}.bin"),
            filename: format!("{id}.bin"),
            source: None,
            transfer_kind: TransferKind::Http,
            integrity_check: None,
            torrent: None,
            state,
            removal_state: None,
            created_at: 1,
            progress: 0.0,
            total_bytes: 100,
            downloaded_bytes: 0,
            speed: 0,
            eta: 0,
            active_segments: None,
            planned_segments: None,
            error: None,
            failure_category: None,
            resume_support: ResumeSupport::Supported,
            retry_attempts: 0,
            auto_restart_attempts: 0,
            resolved_from_url: None,
            hoster_preflight: None,
            target_path: format!("C:/Downloads/{id}.bin"),
            temp_path: format!("C:/Downloads/{id}.bin.part"),
            artifact_exists: None,
            bulk_archive: Some(BulkArchiveInfo {
                id: "bulk_1".into(),
                name: "bulk-download.zip".into(),
                output_kind: BulkArchiveOutputKind::Archive,
                archive_status,
                requires_extraction: None,
                output_path: Some("C:/Downloads/bulk-download.zip".into()),
                error: None,
                warning: None,
                finalize_total_bytes: None,
                finalize_processed_bytes: None,
                finalize_mode: None,
            }),
        }
    }
}
