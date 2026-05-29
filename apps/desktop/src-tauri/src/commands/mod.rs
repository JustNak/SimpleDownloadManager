use crate::download::{
    apply_torrent_runtime_settings, clear_in_memory_torrent_engine_if_idle,
    forget_known_torrent_sessions, forget_torrent_session_for_restart, schedule_downloads,
    schedule_external_reseed, EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS,
};
use crate::ipc::gather_host_registration_diagnostics;
use crate::lifecycle::sync_autostart_setting;
use crate::prompts::{PromptDecision, PromptDuplicateAction, PromptRegistry, PROMPT_CHANGED_EVENT};
use crate::state::{
    clear_torrent_session_cache_directory, validate_settings, BatchDownloadEntry,
    DestructiveCleanupJob, DuplicatePolicy, EnqueueResult, EnqueueStatus, ProgressDelta,
    SharedState, TorrentSessionCacheClearResult,
};
use crate::storage::{
    BulkArchiveOutputKind, DesktopSnapshot, DiagnosticLevel, DiagnosticsSnapshot, DownloadJob,
    DownloadPrompt, DownloadSource, HosterPreflightInfo, HosterPreflightStatus, JobState,
    LocalRecoveryPreview, Settings, TransferKind,
};
use crate::windows::{
    close_download_prompt_window, focus_job_in_main_window_async, show_batch_progress_window,
    show_download_prompt_window, show_progress_window_for_transfer_kind, DOWNLOAD_PROMPT_WINDOW,
};
use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime, State, WebviewWindow};

mod events;
mod native_host;
mod shell;

pub use self::events::{
    emit_download_update, emit_notification_sound, emit_progress_delta, emit_snapshot,
    BatchProgressSnapshot, DownloadUpdateBatch, ExternalUseResult, NotificationSoundEvent,
    NotificationSoundKind, ProgressBatchContext, ProgressBatchKind, ProgressBatchRegistry,
    ProgressJobSnapshot, SettingsSnapshot,
};
#[cfg(windows)]
use self::native_host::ensure_native_host_registration;
use self::native_host::{register_native_host, resolve_install_resource_path};
use self::shell::{open_path, open_url, reveal_path};

pub const STATE_CHANGED_EVENT: &str = "app://state-changed";
const DOWNLOADS_UPDATE_BATCH_EVENT: &str = "app://downloads-update-batch";
pub const NOTIFICATION_SOUND_EVENT: &str = "app://notification-sound";
const PROGRESS_JOB_SNAPSHOT_EVENT: &str = "app://progress-job-snapshot";
const BATCH_PROGRESS_SNAPSHOT_EVENT: &str = "app://batch-progress-snapshot";
const SETTINGS_SNAPSHOT_EVENT: &str = "app://settings-snapshot";
const DOWNLOAD_UPDATE_BATCH_FLUSH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddJobResult {
    pub job_id: String,
    pub filename: String,
    pub status: String,
}

impl From<EnqueueResult> for AddJobResult {
    fn from(result: EnqueueResult) -> Self {
        Self {
            job_id: result.job_id,
            filename: result.filename,
            status: result.status.as_protocol_value().into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedBatchItem {
    pub url: String,
    pub message: String,
}

impl From<crate::hosters::FailedHosterLink> for FailedBatchItem {
    fn from(item: crate::hosters::FailedHosterLink) -> Self {
        Self {
            url: item.url,
            message: item.message,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddJobsResult {
    pub results: Vec<AddJobResult>,
    pub queued_count: usize,
    pub duplicate_count: usize,
    pub failed_items: Vec<FailedBatchItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkMemberRetryResult {
    pub queued_count: usize,
    pub failed_items: Vec<FailedBatchItem>,
}

#[derive(Debug, Clone)]
struct HosterPreflightTarget {
    job_id: String,
    source_url: String,
}

async fn complete_prompt_action(
    app: &AppHandle,
    prompts: PromptRegistry,
    id: &str,
    decision: PromptDecision,
) -> Result<(), String> {
    let remember_prompt_position = matches!(&decision, PromptDecision::Download { .. });
    let next_prompt = prompts.resolve(id, decision).await?;
    if let Some(prompt) = next_prompt {
        show_download_prompt_window(app).await?;
        app.emit_to(DOWNLOAD_PROMPT_WINDOW, PROMPT_CHANGED_EVENT, prompt)
            .map_err(|error| error.to_string())?;
    } else {
        close_download_prompt_window(app, remember_prompt_position);
    }
    Ok(())
}

#[tauri::command]
pub async fn get_app_snapshot(state: State<'_, SharedState>) -> Result<DesktopSnapshot, String> {
    Ok(state.snapshot().await)
}

#[tauri::command]
pub async fn preview_local_recovery(
    state: State<'_, SharedState>,
    root: Option<String>,
) -> Result<LocalRecoveryPreview, String> {
    state
        .preview_local_recovery(root)
        .await
        .map_err(|error| error.message)
}

#[tauri::command]
pub async fn import_local_recovery(
    app: AppHandle,
    state: State<'_, SharedState>,
    candidate_ids: Vec<String>,
) -> Result<DesktopSnapshot, String> {
    let snapshot = state
        .import_local_recovery(candidate_ids)
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub async fn get_progress_job_snapshot(
    state: State<'_, SharedState>,
    id: String,
) -> Result<ProgressJobSnapshot, String> {
    let (job, settings) = state.progress_job_snapshot_parts(&id).await;
    Ok(ProgressJobSnapshot { job, settings })
}

#[tauri::command]
pub async fn get_batch_progress_snapshot(
    state: State<'_, SharedState>,
    registry: State<'_, ProgressBatchRegistry>,
    batch_id: String,
) -> Result<BatchProgressSnapshot, String> {
    let context = registry.get(&batch_id);
    let (jobs, settings) = match context.as_ref() {
        Some(context) => state.batch_progress_snapshot_parts(&context.job_ids).await,
        None => (Vec::new(), state.settings().await),
    };
    Ok(BatchProgressSnapshot {
        context,
        jobs,
        settings,
    })
}

#[tauri::command]
pub async fn get_settings_snapshot(
    state: State<'_, SharedState>,
) -> Result<SettingsSnapshot, String> {
    Ok(SettingsSnapshot {
        settings: state.settings().await,
    })
}

#[tauri::command]
pub fn mark_popup_ready(window: WebviewWindow) -> Result<(), String> {
    crate::windows::mark_popup_ready(&window)
}

#[tauri::command]
pub async fn get_diagnostics(state: State<'_, SharedState>) -> Result<DiagnosticsSnapshot, String> {
    let host_registration = gather_host_registration_diagnostics()?;
    Ok(state.diagnostics_snapshot(host_registration).await)
}

#[tauri::command]
pub async fn export_diagnostics_report(
    state: State<'_, SharedState>,
) -> Result<Option<String>, String> {
    let host_registration = gather_host_registration_diagnostics()?;
    let diagnostics = state.diagnostics_export(host_registration).await;
    let report = serde_json::to_string_pretty(&diagnostics)
        .map_err(|error| format!("Could not serialize diagnostics report: {error}"))?;

    let path = tauri::async_runtime::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_file_name("simple-download-manager-diagnostics.json")
            .save_file()
    })
    .await
    .map_err(|error| format!("Could not open save dialog: {error}"))?;

    let Some(path) = path else {
        return Ok(None);
    };

    std::fs::write(&path, report)
        .map_err(|error| format!("Could not write diagnostics report: {error}"))?;

    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub async fn add_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    url: String,
    expected_sha256: Option<String>,
    transfer_kind: Option<TransferKind>,
) -> Result<AddJobResult, String> {
    let result = state
        .enqueue_download_with_options(
            url,
            crate::state::EnqueueOptions {
                expected_sha256,
                transfer_kind,
                ..Default::default()
            },
        )
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &result.snapshot);
    if result.status == EnqueueStatus::Queued {
        schedule_downloads(app, state.inner().clone());
    }

    Ok(result.into())
}

fn add_jobs_result_from_parts(
    results: Vec<EnqueueResult>,
    failed_items: Vec<FailedBatchItem>,
) -> AddJobsResult {
    let queued_count = results
        .iter()
        .filter(|result| result.status == EnqueueStatus::Queued)
        .count();
    let duplicate_count = results.len().saturating_sub(queued_count);

    AddJobsResult {
        results: results.into_iter().map(Into::into).collect(),
        queued_count,
        duplicate_count,
        failed_items,
    }
}

#[tauri::command]
pub async fn add_jobs(
    app: AppHandle,
    state: State<'_, SharedState>,
    urls: Vec<String>,
    bulk_archive_name: Option<String>,
    resolve_hoster_links: Option<bool>,
    start_paused: Option<bool>,
    bulk_output_kind: Option<BulkArchiveOutputKind>,
) -> Result<AddJobsResult, String> {
    let start_paused = start_paused.unwrap_or(false);
    let _ = bulk_output_kind;
    let bulk_output_kind = BulkArchiveOutputKind::Folder;
    let failed_items = Vec::new();
    let mut preflight_targets = Vec::new();
    let results = if resolve_hoster_links.unwrap_or(false) {
        let prepared_entries = urls
            .into_iter()
            .map(|url| {
                let source_url = url.trim().to_string();
                let is_hoster = crate::hosters::is_supported_hoster_url(&source_url);
                let preflight = is_hoster.then_some(HosterPreflightInfo {
                    status: HosterPreflightStatus::Checking,
                    message: None,
                });
                (
                    is_hoster.then(|| source_url.clone()),
                    BatchDownloadEntry {
                        url: source_url.clone(),
                        filename_hint: crate::hosters::source_filename_hint_for_url(&source_url),
                        resolved_from_url: is_hoster.then_some(source_url),
                        hoster_preflight: preflight,
                    },
                )
            })
            .collect::<Vec<_>>();
        let preflight_sources = prepared_entries
            .iter()
            .map(|(source_url, _entry)| source_url.clone())
            .collect::<Vec<_>>();
        let entries = prepared_entries
            .into_iter()
            .map(|(_source_url, entry)| entry)
            .collect::<Vec<_>>();
        let results = if entries.is_empty() {
            Vec::new()
        } else {
            let archive_name = bulk_archive_name.filter(|_| entries.len() > 1);
            state
                .enqueue_download_entries_with_bulk_options(
                    entries,
                    None,
                    archive_name,
                    start_paused,
                    bulk_output_kind,
                )
                .await
                .map_err(|error| error.message)?
        };
        preflight_targets = results
            .iter()
            .zip(preflight_sources)
            .filter_map(|(result, source_url)| {
                let source_url = source_url?;
                (result.status == EnqueueStatus::Queued).then(|| HosterPreflightTarget {
                    job_id: result.job_id.clone(),
                    source_url,
                })
            })
            .collect();
        results
    } else {
        state
            .enqueue_downloads_with_bulk_options(
                urls,
                None,
                bulk_archive_name,
                start_paused,
                bulk_output_kind,
            )
            .await
            .map_err(|error| error.message)?
    };

    if let Some(result) = results.last() {
        emit_snapshot(&app, &result.snapshot);
    }

    if !preflight_targets.is_empty() {
        spawn_hoster_preflight_checks(app.clone(), state.inner().clone(), preflight_targets);
    }

    if results
        .iter()
        .any(|result| result.status == EnqueueStatus::Queued)
        && !start_paused
    {
        schedule_downloads(app, state.inner().clone());
    }

    Ok(add_jobs_result_from_parts(results, failed_items))
}

const HOSTER_PREFLIGHT_CONCURRENCY: usize = 6;

fn spawn_hoster_preflight_checks(
    app: AppHandle,
    state: SharedState,
    targets: Vec<HosterPreflightTarget>,
) {
    tauri::async_runtime::spawn(async move {
        stream::iter(targets)
            .for_each_concurrent(HOSTER_PREFLIGHT_CONCURRENCY, |target| {
                let app = app.clone();
                let state = state.clone();
                async move {
                    let preflight =
                        match crate::hosters::preflight_hoster_source(&target.source_url).await {
                            Ok(Some(_)) => HosterPreflightInfo {
                                status: HosterPreflightStatus::Ready,
                                message: None,
                            },
                            Ok(None) => HosterPreflightInfo {
                                status: HosterPreflightStatus::Failed,
                                message: Some("Unsupported hoster URL.".into()),
                            },
                            Err(error) => HosterPreflightInfo {
                                status: HosterPreflightStatus::Failed,
                                message: Some(error.message),
                            },
                        };

                    match state.set_hoster_preflight(&target.job_id, preflight).await {
                        Ok(snapshot) => emit_snapshot(&app, &snapshot),
                        Err(error) => eprintln!("failed to update hoster preflight: {error}"),
                    }
                }
            })
            .await;
    });
}

#[tauri::command]
pub async fn pause_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    let wait_for_torrent_release = state
        .torrent_pause_requires_worker_release(&id)
        .await
        .map_err(|error| error.message)?;
    let snapshot = state.pause_job(&id).await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app.clone(), state.inner().clone());
    if wait_for_torrent_release {
        state
            .wait_for_torrent_removal_release(&id)
            .await
            .map_err(|error| error.message)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn pause_jobs(
    app: AppHandle,
    state: State<'_, SharedState>,
    ids: Vec<String>,
) -> Result<(), String> {
    let ids = normalized_job_ids(ids);
    if ids.is_empty() {
        return Ok(());
    }

    let mut wait_for_release_ids = Vec::new();
    for id in &ids {
        if state
            .torrent_pause_requires_worker_release(id)
            .await
            .map_err(|error| error.message)?
        {
            wait_for_release_ids.push(id.clone());
        }
    }

    let mut snapshot = None;
    for id in &ids {
        snapshot = Some(state.pause_job(id).await.map_err(|error| error.message)?);
    }
    emit_optional_batch_snapshot(&app, state.inner().clone(), snapshot);

    for id in wait_for_release_ids {
        state
            .wait_for_torrent_removal_release(&id)
            .await
            .map_err(|error| error.message)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn resume_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    let snapshot = state.resume_job(&id).await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn resume_jobs(
    app: AppHandle,
    state: State<'_, SharedState>,
    ids: Vec<String>,
) -> Result<(), String> {
    let ids = normalized_job_ids(ids);
    if ids.is_empty() {
        return Ok(());
    }

    let mut snapshot = None;
    for id in &ids {
        snapshot = Some(state.resume_job(id).await.map_err(|error| error.message)?);
    }
    emit_optional_batch_snapshot(&app, state.inner().clone(), snapshot);
    Ok(())
}

#[tauri::command]
pub async fn pause_all_jobs(app: AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let snapshot = state
        .pause_all_jobs()
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn resume_all_jobs(app: AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let snapshot = state
        .resume_all_jobs()
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn cancel_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
    delete_from_disk: Option<bool>,
) -> Result<(), String> {
    let delete_from_disk = delete_from_disk.unwrap_or(false);
    if delete_from_disk {
        let prepared = state
            .cancel_jobs_for_delete(std::slice::from_ref(&id))
            .await
            .map_err(|error| error.message)?;
        emit_snapshot(&app, &prepared.snapshot);
        schedule_destructive_cleanup(app.clone(), state.inner().clone(), prepared.jobs);
    } else {
        let snapshot = state.cancel_job(&id).await.map_err(|error| error.message)?;
        emit_snapshot(&app, &snapshot);
        schedule_downloads(app, state.inner().clone());
        return Ok(());
    }
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn cancel_jobs(
    app: AppHandle,
    state: State<'_, SharedState>,
    ids: Vec<String>,
    delete_from_disk: Option<bool>,
) -> Result<(), String> {
    let ids = normalized_job_ids(ids);
    if ids.is_empty() {
        return Ok(());
    }

    let delete_from_disk = delete_from_disk.unwrap_or(false);
    if delete_from_disk {
        let prepared = state
            .cancel_jobs_for_delete(&ids)
            .await
            .map_err(|error| error.message)?;
        emit_snapshot(&app, &prepared.snapshot);
        schedule_destructive_cleanup(app.clone(), state.inner().clone(), prepared.jobs);
    } else {
        let snapshot = state
            .cancel_jobs(&ids)
            .await
            .map_err(|error| error.message)?;
        emit_snapshot(&app, &snapshot);
    }
    Ok(())
}

fn schedule_destructive_cleanup(
    app: AppHandle,
    state: SharedState,
    jobs: Vec<DestructiveCleanupJob>,
) {
    if jobs.is_empty() {
        return;
    }

    tauri::async_runtime::spawn(async move {
        match state.run_destructive_cleanup(jobs).await {
            Ok(snapshot) => {
                emit_snapshot(&app, &snapshot);
                schedule_downloads(app, state);
            }
            Err(error) => {
                eprintln!(
                    "failed to finalize canceled disk cleanup: {}",
                    error.message
                );
            }
        }
    });
}

#[tauri::command]
pub async fn retry_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    let snapshot = state.retry_job(&id).await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn restart_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    if let Some(torrent) = state
        .torrent_restart_cleanup_info(&id)
        .await
        .map_err(|error| error.message)?
    {
        forget_torrent_session_for_restart(state.inner(), &torrent).await?;
    }

    let snapshot = state
        .restart_job(&id)
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn retry_failed_jobs(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let snapshot = state
        .retry_failed_jobs()
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn swap_failed_download_to_browser(
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    let job = state
        .job_snapshot(&id)
        .await
        .ok_or_else(|| "Download was not found.".to_string())?;
    let url = failed_browser_download_url(&job)?;
    open_url(url)
}

#[tauri::command]
pub async fn remove_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    prepare_torrent_removal(&state, &id).await?;
    let snapshot = state.remove_job(&id).await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn delete_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
    delete_from_disk: bool,
) -> Result<(), String> {
    if delete_from_disk {
        let prepared = state
            .delete_jobs_for_disk_cleanup(&[id])
            .await
            .map_err(|error| error.message)?;
        emit_snapshot(&app, &prepared.snapshot);
        schedule_destructive_cleanup(app.clone(), state.inner().clone(), prepared.jobs);
        schedule_downloads(app, state.inner().clone());
        return Ok(());
    }

    prepare_torrent_removal(&state, &id).await?;
    let snapshot = state
        .delete_job(&id, false)
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn delete_jobs(
    app: AppHandle,
    state: State<'_, SharedState>,
    ids: Vec<String>,
    delete_from_disk: bool,
) -> Result<(), String> {
    let ids = normalized_job_ids(ids);
    if ids.is_empty() {
        return Ok(());
    }

    if delete_from_disk {
        let prepared = state
            .delete_jobs_for_disk_cleanup(&ids)
            .await
            .map_err(|error| error.message)?;
        emit_snapshot(&app, &prepared.snapshot);
        schedule_destructive_cleanup(app.clone(), state.inner().clone(), prepared.jobs);
        schedule_downloads(app, state.inner().clone());
        return Ok(());
    }

    for id in &ids {
        prepare_torrent_removal(&state, id).await?;
    }

    let mut snapshot = None;
    for id in &ids {
        snapshot = Some(
            state
                .delete_job(id, delete_from_disk)
                .await
                .map_err(|error| error.message)?,
        );
    }
    emit_optional_batch_snapshot(&app, state.inner().clone(), snapshot);
    Ok(())
}

fn normalized_job_ids(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty() && seen.insert(id.clone()))
        .collect()
}

fn emit_optional_batch_snapshot(
    app: &AppHandle,
    state: SharedState,
    snapshot: Option<DesktopSnapshot>,
) {
    if let Some(snapshot) = snapshot {
        emit_snapshot(app, &snapshot);
        schedule_downloads(app.clone(), state);
    }
}

async fn prepare_torrent_removal(state: &State<'_, SharedState>, id: &str) -> Result<(), String> {
    let Some(cleanup) = state
        .torrent_removal_cleanup_info(id)
        .await
        .map_err(|error| error.message)?
    else {
        return Ok(());
    };

    if cleanup.wait_for_worker_release {
        state
            .wait_for_torrent_removal_release(id)
            .await
            .map_err(|error| error.message)?;
    }

    forget_torrent_session_for_restart(state.inner(), &cleanup.torrent).await
}

#[tauri::command]
pub async fn rename_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
    filename: String,
) -> Result<(), String> {
    let snapshot = state
        .rename_job(&id, &filename)
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn clear_completed_jobs(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let snapshot = state
        .clear_completed_jobs()
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    Ok(())
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, SharedState>,
    mut settings: Settings,
) -> Result<Settings, String> {
    validate_settings(&mut settings)?;
    sync_autostart_setting(settings.start_on_startup)?;
    let snapshot = state.save_settings(settings).await?;
    let saved_settings = snapshot.settings.clone();
    emit_snapshot(&app, &snapshot);
    if let Err(error) = apply_torrent_runtime_settings(state.inner()).await {
        eprintln!("failed to refresh torrent runtime settings: {error}");
    }
    schedule_downloads(app, state.inner().clone());
    Ok(saved_settings)
}

#[tauri::command]
pub async fn browse_directory() -> Result<Option<String>, String> {
    let selected = tauri::async_runtime::spawn_blocking(|| rfd::FileDialog::new().pick_folder())
        .await
        .map_err(|error| format!("Could not open folder picker: {error}"))?;

    Ok(selected.map(|path| path.display().to_string()))
}

#[tauri::command]
pub async fn clear_torrent_session_cache(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> Result<TorrentSessionCacheClearResult, String> {
    let prepared = state
        .prepare_torrent_session_cache_clear()
        .await
        .map_err(|error| error.message)?;

    if let Err(error) = forget_known_torrent_sessions(&prepared.torrents).await {
        let _ = state
            .record_diagnostic_event(
                DiagnosticLevel::Warning,
                "torrent",
                format!(
                    "Could not forget in-memory torrent sessions before cache cleanup: {error}"
                ),
                None,
            )
            .await;
    }

    if let Err(error) = clear_in_memory_torrent_engine_if_idle(state.inner()).await {
        let _ = state
            .record_diagnostic_event(
                DiagnosticLevel::Warning,
                "torrent",
                format!("Could not reset in-memory torrent engine during cache cleanup: {error}"),
                None,
            )
            .await;
    }

    let result = clear_torrent_session_cache_directory(&state.app_data_dir())?;
    emit_snapshot(&app, &prepared.snapshot);
    Ok(result)
}

#[tauri::command]
pub async fn browse_torrent_file() -> Result<Option<String>, String> {
    let selected = tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .add_filter("Torrent or magnet", &["torrent", "magnet", "txt"])
            .pick_file()
    })
    .await
    .map_err(|error| format!("Could not open torrent picker: {error}"))?;

    selected
        .as_deref()
        .map(torrent_import_value_from_path)
        .transpose()
}

#[tauri::command]
pub async fn get_current_download_prompt(
    prompts: State<'_, PromptRegistry>,
) -> Result<Option<DownloadPrompt>, String> {
    Ok(prompts.active_prompt().await)
}

#[tauri::command]
pub async fn confirm_download_prompt(
    app: AppHandle,
    prompts: State<'_, PromptRegistry>,
    id: String,
    directory_override: Option<String>,
    allow_duplicate: Option<bool>,
    duplicate_action: Option<PromptDuplicateAction>,
    renamed_filename: Option<String>,
) -> Result<(), String> {
    let duplicate_action = duplicate_action.unwrap_or_else(|| {
        if allow_duplicate.unwrap_or(false) {
            PromptDuplicateAction::DownloadAnyway
        } else {
            PromptDuplicateAction::ReturnExisting
        }
    });
    complete_prompt_action(
        &app,
        prompts.inner().clone(),
        &id,
        PromptDecision::Download {
            directory_override,
            duplicate_action,
            renamed_filename,
        },
    )
    .await
}

fn prompt_enqueue_details(
    default_filename: String,
    duplicate_action: PromptDuplicateAction,
    renamed_filename: Option<String>,
) -> Result<(String, DuplicatePolicy), String> {
    match duplicate_action {
        PromptDuplicateAction::ReturnExisting => {
            Ok((default_filename, DuplicatePolicy::ReturnExisting))
        }
        PromptDuplicateAction::DownloadAnyway => Ok((default_filename, DuplicatePolicy::Allow)),
        PromptDuplicateAction::Overwrite => {
            Ok((default_filename, DuplicatePolicy::ReplaceExisting))
        }
        PromptDuplicateAction::Rename => {
            let filename = renamed_filename.unwrap_or_default();
            if filename.trim().is_empty() {
                return Err("Filename cannot be empty.".into());
            }
            Ok((filename, DuplicatePolicy::Allow))
        }
    }
}

#[tauri::command]
pub async fn show_existing_download_prompt(
    app: AppHandle,
    prompts: State<'_, PromptRegistry>,
    id: String,
) -> Result<(), String> {
    let active_prompt = prompts.active_prompt().await;
    let existing_job_id = active_prompt
        .as_ref()
        .and_then(|prompt| prompt.duplicate_job.as_ref())
        .map(|job| job.id.clone());

    complete_prompt_action(
        &app,
        prompts.inner().clone(),
        &id,
        PromptDecision::ShowExisting,
    )
    .await?;

    if let Some(job_id) = existing_job_id {
        focus_job_in_main_window_async(&app, &job_id).await;
    }

    Ok(())
}

#[tauri::command]
pub async fn cancel_download_prompt(
    app: AppHandle,
    prompts: State<'_, PromptRegistry>,
    id: String,
) -> Result<(), String> {
    complete_prompt_action(&app, prompts.inner().clone(), &id, PromptDecision::Cancel).await
}

#[tauri::command]
pub async fn open_progress_window(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    let transfer_kind = state
        .job_snapshot(&id)
        .await
        .map(|job| job.transfer_kind)
        .ok_or_else(|| "Download job is no longer available.".to_string())?;
    show_progress_window_for_transfer_kind(&app, &id, transfer_kind).await
}

#[tauri::command]
pub async fn open_batch_progress_window(
    app: AppHandle,
    registry: State<'_, ProgressBatchRegistry>,
    context: ProgressBatchContext,
) -> Result<(), String> {
    let batch_id = context.batch_id.clone();
    registry.store(context);
    show_batch_progress_window(&app, &batch_id).await
}

#[tauri::command]
pub async fn get_progress_batch_context(
    registry: State<'_, ProgressBatchRegistry>,
    batch_id: String,
) -> Result<Option<ProgressBatchContext>, String> {
    Ok(registry.get(&batch_id))
}

#[tauri::command]
pub async fn open_job_file(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<ExternalUseResult, String> {
    let preparation = state
        .prepare_job_for_external_use(&id)
        .await
        .map_err(|error| error.message)?;
    if let Some(snapshot) = &preparation.snapshot {
        emit_snapshot(&app, snapshot);
    }

    let path = state
        .resolve_openable_path(&id)
        .await
        .map_err(|error| error.message)?;

    let auto_reseed_retry_seconds = if preparation.paused_torrent {
        schedule_external_reseed(app.clone(), state.inner().clone(), id.clone()).await;
        Some(EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS)
    } else {
        None
    };

    tauri::async_runtime::spawn_blocking(move || open_path(&path))
        .await
        .map_err(|error| format!("Could not open file: {error}"))??;

    Ok(ExternalUseResult {
        paused_torrent: preparation.paused_torrent,
        auto_reseed_retry_seconds,
    })
}

#[tauri::command]
pub async fn reveal_job_in_folder(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<ExternalUseResult, String> {
    let preparation = state
        .prepare_job_for_external_use(&id)
        .await
        .map_err(|error| error.message)?;
    if let Some(snapshot) = &preparation.snapshot {
        emit_snapshot(&app, snapshot);
    }

    let path = state
        .resolve_revealable_path(&id)
        .await
        .map_err(|error| error.message)?;

    let auto_reseed_retry_seconds = if preparation.paused_torrent {
        schedule_external_reseed(app.clone(), state.inner().clone(), id.clone()).await;
        Some(EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS)
    } else {
        None
    };

    tauri::async_runtime::spawn_blocking(move || reveal_path(&path))
        .await
        .map_err(|error| format!("Could not reveal file: {error}"))??;

    Ok(ExternalUseResult {
        paused_torrent: preparation.paused_torrent,
        auto_reseed_retry_seconds,
    })
}

#[tauri::command]
pub async fn open_bulk_archive(
    state: State<'_, SharedState>,
    archive_id: String,
) -> Result<(), String> {
    let path = state
        .resolve_bulk_archive_openable_path(&archive_id)
        .await
        .map_err(|error| error.message)?;

    tauri::async_runtime::spawn_blocking(move || open_path(&path))
        .await
        .map_err(|error| format!("Could not open archive: {error}"))?
}

#[tauri::command]
pub async fn reveal_bulk_archive(
    state: State<'_, SharedState>,
    archive_id: String,
) -> Result<(), String> {
    let path = state
        .resolve_bulk_archive_revealable_path(&archive_id)
        .await
        .map_err(|error| error.message)?;

    tauri::async_runtime::spawn_blocking(move || reveal_path(&path))
        .await
        .map_err(|error| format!("Could not reveal archive: {error}"))?
}

#[tauri::command]
pub async fn retry_bulk_archive(
    app: AppHandle,
    state: State<'_, SharedState>,
    archive_id: String,
) -> Result<(), String> {
    crate::download::retry_bulk_archive(&app, state.inner(), &archive_id).await
}

#[tauri::command]
pub async fn retry_bulk_members(
    app: AppHandle,
    state: State<'_, SharedState>,
    archive_id: String,
) -> Result<BulkMemberRetryResult, String> {
    crate::download::reset_bulk_failure_sound(&archive_id);
    let candidates = state
        .bulk_member_retry_candidates(&archive_id)
        .await
        .map_err(|error| error.to_string())?;
    let mut queued_count = 0;
    let failed_items = Vec::new();
    let mut snapshot = None;

    for candidate in candidates {
        let resolved_url = candidate.source_url.clone();

        snapshot = Some(
            state
                .retry_bulk_member(&candidate.id, resolved_url)
                .await
                .map_err(|error| error.to_string())?,
        );
        queued_count += 1;
    }

    if let Some(snapshot) = snapshot {
        emit_snapshot(&app, &snapshot);
    }
    if queued_count > 0 {
        schedule_downloads(app, state.inner().clone());
    }

    Ok(BulkMemberRetryResult {
        queued_count,
        failed_items,
    })
}

#[tauri::command]
pub async fn open_install_docs() -> Result<(), String> {
    let docs_path = resolve_install_resource_path("install.md")?;

    tauri::async_runtime::spawn_blocking(move || open_path(&docs_path))
        .await
        .map_err(|error| format!("Could not open install docs: {error}"))??;

    Ok(())
}

#[tauri::command]
pub async fn run_host_registration_fix() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(register_native_host)
        .await
        .map_err(|error| format!("Could not start host registration: {error}"))??;

    Ok(())
}

pub fn initialize_native_host_registration() {
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn(async move {
            let result = tauri::async_runtime::spawn_blocking(ensure_native_host_registration)
                .await
                .map_err(|error| format!("Could not start native host registration: {error}"))
                .and_then(|result| result);

            if let Err(error) = result {
                eprintln!("native host auto-registration failed: {error}");
            }
        });
    }
}

#[tauri::command]
pub async fn test_extension_handoff(
    app: AppHandle,
    state: State<'_, SharedState>,
    prompts: State<'_, PromptRegistry>,
) -> Result<(), String> {
    let request_id = format!("test_handoff_{}", unix_timestamp_millis());
    let prompt = state
        .prepare_download_prompt(
            request_id,
            "https://example.com/simple-download-manager-test.bin",
            Some(DownloadSource {
                entry_point: "browser_download".into(),
                browser: "chrome".into(),
                extension_version: "settings-test".into(),
                page_url: Some("https://example.com/downloads".into()),
                page_title: Some("Simple Download Manager handoff test".into()),
                referrer: Some("https://example.com/downloads".into()),
                incognito: Some(false),
            }),
            Some("simple-download-manager-test.bin".into()),
            Some(1_048_576),
        )
        .await
        .map_err(|error| error.message)?;

    let receiver = prompts.enqueue(prompt.clone()).await;
    show_download_prompt_window(&app).await?;
    if let Some(active_prompt) = prompts.active_prompt().await {
        app.emit_to(DOWNLOAD_PROMPT_WINDOW, PROMPT_CHANGED_EVENT, active_prompt)
            .map_err(|error| error.to_string())?;
    }

    let worker_app = app.clone();
    let worker_state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let decision = receiver.await.unwrap_or(PromptDecision::Cancel);
        match decision {
            PromptDecision::Cancel => {}
            PromptDecision::ShowExisting => {
                if let Some(job) = prompt.duplicate_job {
                    focus_job_in_main_window_async(&worker_app, &job.id).await;
                }
            }
            PromptDecision::Download {
                directory_override,
                duplicate_action,
                renamed_filename,
            } => {
                let (filename_hint, duplicate_policy) = match prompt_enqueue_details(
                    prompt.filename.clone(),
                    duplicate_action,
                    renamed_filename,
                ) {
                    Ok(details) => details,
                    Err(message) => {
                        eprintln!("test extension handoff failed: {message}");
                        return;
                    }
                };
                let result = worker_state
                    .enqueue_download_with_options(
                        prompt.url,
                        crate::state::EnqueueOptions {
                            source: prompt.source,
                            directory_override,
                            filename_hint: Some(filename_hint),
                            duplicate_policy,
                            ..Default::default()
                        },
                    )
                    .await;

                match result {
                    Ok(result) => {
                        let show_progress = worker_state.show_progress_after_handoff().await;
                        emit_snapshot(&worker_app, &result.snapshot);
                        if result.status == EnqueueStatus::Queued {
                            if show_progress {
                                let transfer_kind = result
                                    .snapshot
                                    .jobs
                                    .iter()
                                    .find(|job| job.id == result.job_id)
                                    .map(|job| job.transfer_kind)
                                    .unwrap_or_default();
                                let _ = show_progress_window_for_transfer_kind(
                                    &worker_app,
                                    &result.job_id,
                                    transfer_kind,
                                )
                                .await;
                            }
                            schedule_downloads(worker_app, worker_state);
                        }
                    }
                    Err(error) => {
                        eprintln!("test extension handoff failed: {}", error.message);
                    }
                }
            }
        }
    });

    Ok(())
}

fn unix_timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn failed_browser_download_url(job: &DownloadJob) -> Result<&str, String> {
    if job.state != JobState::Failed
        || job.transfer_kind != TransferKind::Http
        || job
            .source
            .as_ref()
            .map(|source| source.entry_point.as_str())
            != Some("browser_download")
    {
        return Err("Only failed browser downloads can be swapped back to the browser.".into());
    }

    let parsed = url::Url::parse(&job.url)
        .map_err(|_| "The download URL is not valid for browser swap.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only http and https downloads can be swapped back to the browser.".into());
    }

    Ok(job.url.as_str())
}

fn torrent_import_value_from_path(path: &Path) -> Result<String, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "torrent" => Ok(path.display().to_string()),
        "magnet" | "txt" => {
            let content = std::fs::read_to_string(path)
                .map_err(|error| format!("Could not read torrent import file: {error}"))?;
            torrent_import_value_from_text(&content)
        }
        _ => Err("Choose a .torrent file or a text file containing a magnet link.".into()),
    }
}

fn torrent_import_value_from_text(content: &str) -> Result<String, String> {
    let value = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| "The selected import file is empty.".to_string())?;

    if value.starts_with("magnet:?")
        || (value.starts_with("https://") || value.starts_with("http://"))
            && value.to_ascii_lowercase().contains(".torrent")
    {
        Ok(value.to_string())
    } else {
        Err("The selected import file must contain a magnet link or HTTP(S) .torrent URL.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::native_host::should_register_native_host;
    use crate::storage::{
        DownloadJob, DownloadSource, HostRegistrationStatus, JobState, ResumeSupport, TransferKind,
    };
    #[cfg(windows)]
    use std::path::Path;

    #[test]
    fn native_host_registration_runs_for_missing_or_broken_entries() {
        assert!(!should_register_native_host(
            HostRegistrationStatus::Configured
        ));
        assert!(should_register_native_host(HostRegistrationStatus::Missing));
        assert!(should_register_native_host(HostRegistrationStatus::Broken));
    }

    #[test]
    fn progress_batch_registry_stores_and_retrieves_context_by_batch_id() {
        let registry = super::ProgressBatchRegistry::default();
        let context = super::ProgressBatchContext {
            batch_id: "batch_123".into(),
            kind: super::ProgressBatchKind::Multi,
            job_ids: vec!["job_1".into(), "job_2".into()],
            title: "Bulk download progress".into(),
            archive_name: None,
            failed_items: vec![super::FailedBatchItem {
                url: "https://datanodes.to/61nni6me5p0n/protected.rar".into(),
                message: "DataNodes captcha-protected downloads are not supported.".into(),
            }],
        };

        registry.store(context.clone());

        assert_eq!(registry.get("batch_123"), Some(context));
        assert_eq!(registry.get("missing"), None);
    }

    #[test]
    fn add_jobs_result_carries_failed_items_without_fake_job_ids() {
        let result = super::add_jobs_result_from_parts(
            Vec::new(),
            vec![super::FailedBatchItem {
                url: "https://datanodes.to/61nni6me5p0n/protected.rar".into(),
                message: "DataNodes captcha-protected downloads are not supported.".into(),
            }],
        );

        assert_eq!(result.queued_count, 0);
        assert_eq!(result.duplicate_count, 0);
        assert!(result.results.is_empty());
        assert_eq!(result.failed_items.len(), 1);
        assert_eq!(
            result.failed_items[0].url,
            "https://datanodes.to/61nni6me5p0n/protected.rar"
        );
    }

    #[test]
    fn add_jobs_hoster_resolution_queues_sources_for_background_preflight() {
        let source = include_str!("mod.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("commands source should contain production code");

        assert!(production_source.contains("HosterPreflightStatus::Checking"));
        assert!(production_source.contains("spawn_hoster_preflight_checks"));
        assert!(production_source.contains("preflight_hoster_source"));
        assert!(!production_source.contains("resolve_hoster_links_partial(urls)"));
    }

    #[test]
    fn bulk_hoster_preflight_uses_resolver_concurrency() {
        let source = include_str!("mod.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("commands source should contain production code");

        assert!(production_source.contains("const HOSTER_PREFLIGHT_CONCURRENCY: usize = 6"));
        assert!(production_source.contains(".for_each_concurrent(HOSTER_PREFLIGHT_CONCURRENCY"));
    }

    #[test]
    fn scoped_batch_job_commands_are_registered_for_single_ipc_actions() {
        let source = include_str!("mod.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("commands source should contain production code");

        for command in ["pause_jobs", "resume_jobs", "cancel_jobs", "delete_jobs"] {
            assert!(
                production_source.contains(&format!("pub async fn {command}(")),
                "{command} should be exposed as a scoped batch command"
            );
        }
    }

    #[test]
    fn bulk_member_retry_command_preserves_source_urls_for_worker_refresh() {
        let source = include_str!("mod.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("commands source should contain production code");

        assert!(production_source.contains("pub async fn retry_bulk_members("));
        assert!(production_source.contains("let resolved_url = candidate.source_url.clone();"));
        assert!(production_source.contains(".retry_bulk_member(&candidate.id, resolved_url)"));
        assert!(
            !production_source.contains("resolve_hoster_links(vec![candidate.source_url.clone()])")
        );
    }

    #[test]
    fn external_use_commands_schedule_auto_reseed() {
        let source = include_str!("mod.rs");
        let events_source = include_str!("events.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("commands source should contain production code");

        assert!(events_source.contains("pub auto_reseed_retry_seconds: Option<u64>"));
        assert_eq!(
            production_source
                .matches("schedule_external_reseed(app.clone(), state.inner().clone(), id.clone()).await")
                .count(),
            2,
            "open file and open folder should both schedule auto-reseed after pausing a torrent"
        );
        assert!(production_source.contains("Some(EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS)"));
    }

    #[test]
    fn save_settings_applies_torrent_runtime_settings_before_rescheduling() {
        let source = include_str!("mod.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("commands source should contain production code");
        let save_settings = production_source
            .find("pub async fn save_settings(")
            .expect("save_settings command should exist");
        let runtime_apply = production_source[save_settings..]
            .find("apply_torrent_runtime_settings(state.inner()).await")
            .expect("save_settings should apply torrent runtime settings")
            + save_settings;
        let schedule = production_source[save_settings..]
            .find("schedule_downloads(app, state.inner().clone());")
            .expect("save_settings should reschedule downloads")
            + save_settings;

        assert!(
            runtime_apply < schedule,
            "torrent upload limits should be applied to the live session before downloads are rescheduled"
        );
    }

    #[test]
    fn targeted_read_commands_do_not_clone_full_snapshots() {
        let source = include_str!("mod.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("commands source should contain production code");

        for (function_name, expected_selector) in [
            ("get_progress_job_snapshot", "progress_job_snapshot_parts"),
            (
                "get_batch_progress_snapshot",
                "batch_progress_snapshot_parts",
            ),
            ("open_progress_window", "job_snapshot"),
            ("swap_failed_download_to_browser", "job_snapshot"),
        ] {
            let function_start = production_source
                .find(&format!("pub async fn {function_name}("))
                .unwrap_or_else(|| panic!("{function_name} command should exist"));
            let function_body = &production_source[function_start..];
            let function_end = function_body
                .find("\n#[tauri::command]")
                .unwrap_or(function_body.len());
            let function_body = &function_body[..function_end];

            assert!(
                !function_body.contains("state.snapshot().await"),
                "{function_name} should avoid full DesktopSnapshot clones for targeted reads"
            );
            assert!(
                function_body.contains(expected_selector),
                "{function_name} should use {expected_selector}"
            );
        }
    }

    #[test]
    fn torrent_remove_and_delete_prepare_engine_cleanup_before_state_mutation() {
        let source = include_str!("mod.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("commands source should contain production code");

        for function_name in ["remove_job", "delete_job"] {
            let function_start = production_source
                .find(&format!("pub async fn {function_name}("))
                .unwrap_or_else(|| panic!("{function_name} command should exist"));
            let function_body = &production_source[function_start..];
            let cleanup = function_body
                .find("prepare_torrent_removal(&state, &id).await?;")
                .unwrap_or_else(|| panic!("{function_name} should prepare torrent engine cleanup"));
            let state_mutation = function_body
                .find(&format!(".{function_name}(&id"))
                .unwrap_or_else(|| panic!("{function_name} should call state.{function_name}"));

            assert!(
                cleanup < state_mutation,
                "{function_name} should forget rqbit session metadata before removing persisted job state"
            );
        }
    }

    #[test]
    fn failed_browser_download_url_accepts_only_failed_browser_http_jobs() {
        let mut job = failed_browser_download_job();

        assert_eq!(
            super::failed_browser_download_url(&job).unwrap(),
            "https://example.com/file.zip"
        );

        job.state = JobState::Downloading;
        assert!(super::failed_browser_download_url(&job).is_err());

        job = failed_browser_download_job();
        job.source = None;
        assert!(super::failed_browser_download_url(&job).is_err());

        job = failed_browser_download_job();
        job.url = "magnet:?xt=urn:btih:example".into();
        job.transfer_kind = TransferKind::Torrent;
        assert!(super::failed_browser_download_url(&job).is_err());
    }

    #[test]
    fn torrent_import_value_accepts_torrent_files() {
        let dir = test_runtime_dir("torrent-import");
        let path = dir.join("sample.torrent");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"d4:infode").unwrap();

        assert_eq!(
            super::torrent_import_value_from_path(&path).unwrap(),
            path.display().to_string()
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn torrent_import_value_reads_magnet_files() {
        let dir = test_runtime_dir("magnet-import");
        let path = dir.join("sample.magnet");
        let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Sample";
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, format!("  {magnet}\n")).unwrap();

        assert_eq!(
            super::torrent_import_value_from_path(&path).unwrap(),
            magnet
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    fn failed_browser_download_job() -> DownloadJob {
        DownloadJob {
            id: "job_1".into(),
            url: "https://example.com/file.zip".into(),
            filename: "file.zip".into(),
            source: Some(DownloadSource {
                entry_point: "browser_download".into(),
                browser: "chrome".into(),
                extension_version: "0.3.51".into(),
                page_url: None,
                page_title: None,
                referrer: None,
                incognito: Some(false),
            }),
            transfer_kind: TransferKind::Http,
            integrity_check: None,
            torrent: None,
            state: JobState::Failed,
            removal_state: None,
            created_at: 0,
            progress: 25.0,
            total_bytes: 100,
            downloaded_bytes: 25,
            speed: 0,
            eta: 0,
            active_segments: None,
            planned_segments: None,
            error: Some("network error".into()),
            failure_category: None,
            resume_support: ResumeSupport::Supported,
            retry_attempts: 1,
            auto_restart_attempts: 0,
            resolved_from_url: None,
            hoster_preflight: None,
            target_path: "C:/Downloads/file.zip".into(),
            temp_path: "C:/Downloads/file.zip.part".into(),
            artifact_exists: None,
            bulk_archive: None,
        }
    }

    fn test_runtime_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::current_dir()
            .unwrap()
            .join("test-runtime")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(windows)]
    #[test]
    fn native_host_manifest_uses_native_path_and_browser_allowlist() {
        let manifest = super::native_host::native_host_manifest_json(
            Path::new(
                r"C:\Program Files\Simple Download Manager\simple-download-manager-native-host.exe",
            ),
            "allowed_origins",
            serde_json::json!(["chrome-extension://extension-id/"]),
        );

        assert_eq!(
            manifest.get("path").and_then(|value| value.as_str()),
            Some(
                r"C:\Program Files\Simple Download Manager\simple-download-manager-native-host.exe"
            )
        );
        assert_eq!(
            manifest
                .get("allowed_origins")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("chrome-extension://extension-id/")
        );
    }
}
