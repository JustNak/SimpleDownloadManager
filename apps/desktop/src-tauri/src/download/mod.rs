use crate::commands::{emit_download_update, emit_snapshot};
use crate::state::{
    should_stop_seeding, BulkArchiveReady, BulkMemberAutoRestartMode, BulkMemberSlowRecoveryState,
    ExternalReseedAttempt, SharedState, TorrentRuntimePhase, TorrentRuntimeSnapshot, WorkerControl,
};
use crate::storage::{
    default_torrent_download_directory_for, BulkArchiveStatus, BulkFinalizeMode,
    BulkHosterAccelerationMode, DiagnosticLevel, DownloadPerformanceMode, FailureCategory,
    HandoffAuth, JobState, ResumeSupport, Settings, TorrentInfo, TorrentPeerConnectionWatchdogMode,
    TransferKind,
};
use crate::torrent::{
    cached_torrent_metadata_source, pending_torrent_cleanup_info_hash, prepare_torrent_source,
    PreparedTorrentSource, TorrentAddSessionOutcome, TorrentEngine, TorrentSourceKind,
    TrackerFirstMetadataOutcome,
};
use futures_util::StreamExt;
use percent_encoding::percent_decode_str;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT_ENCODING, ACCEPT_RANGES, CONTENT_DISPOSITION,
    CONTENT_RANGE, CONTENT_TYPE, ETAG, IF_RANGE, LAST_MODIFIED, LOCATION, RANGE, RETRY_AFTER,
};
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::plugin::PermissionState;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, Mutex};

mod bulk_finalize;
mod client;
mod filesystem;
mod http;
mod notifications;
mod policy;
mod segmented;
mod stream;
mod torrent;
mod types;

use bulk_finalize::*;
use client::*;
use filesystem::*;
use http::*;
use notifications::*;
use policy::*;
use segmented::*;
use stream::*;
use torrent::*;
use types::*;

pub(crate) use notifications::reset_bulk_failure_sound;

#[cfg(test)]
mod tests;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(120);
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(8);
const PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_millis(750);
const PROGRESS_PERSIST_INTERVAL: Duration = Duration::from_secs(5);
const THROTTLE_CONTROL_INTERVAL: Duration = Duration::from_millis(250);
const TORRENT_LOW_THROUGHPUT_LIVE_PEER_THRESHOLD: u32 = 10;
const TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND: u64 = 256 * 1024;
const TORRENT_LOW_THROUGHPUT_REPORT_WINDOW: Duration = Duration::from_secs(30);
const TORRENT_LOW_THROUGHPUT_REPORT_INTERVAL: Duration = Duration::from_secs(60);
const TORRENT_RESTORE_RECHECK_IDLE_WINDOW: Duration = Duration::from_secs(45);
const TORRENT_RESTORE_STALLED_IDLE_WINDOW: Duration = Duration::from_secs(90);
const TORRENT_PEER_WATCHDOG_WINDOW: Duration = Duration::from_secs(60);
const TORRENT_METADATA_TIMEOUT: Duration = Duration::from_secs(60);
const TORRENT_METADATA_CONTROL_INTERVAL: Duration = Duration::from_millis(250);
pub const EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS: u64 = 60;
const DOWNLOAD_BUFFER_SIZE: usize = 512 * 1024;
const BALANCED_MIN_SEGMENTED_SIZE: u64 = 32 * 1024 * 1024;
const BALANCED_TARGET_SEGMENT_SIZE: u64 = 64 * 1024 * 1024;
const FAST_MIN_SEGMENTED_SIZE: u64 = 16 * 1024 * 1024;
const FAST_TARGET_SEGMENT_SIZE: u64 = 32 * 1024 * 1024;
const RANGE_BACKOFF_DURATION: Duration = Duration::from_secs(10 * 60);
const DIRECT_BULK_TOTAL_SEGMENT_CONNECTION_BUDGET: usize = 16;
const DIRECT_BULK_ORIGIN_SEGMENT_CONNECTION_BUDGET: usize = 8;
const HOSTER_BULK_BALANCED_TOTAL_SEGMENT_CONNECTION_BUDGET: usize = 24;
const HOSTER_BULK_BALANCED_ORIGIN_SEGMENT_CONNECTION_BUDGET: usize = 16;
const HOSTER_BULK_FAST_TOTAL_SEGMENT_CONNECTION_BUDGET: usize = 48;
const HOSTER_BULK_FAST_ORIGIN_SEGMENT_CONNECTION_BUDGET: usize = 24;
const MAX_RETRY_AFTER_DELAY: Duration = Duration::from_secs(60);
const MAX_RETRY_JITTER: Duration = Duration::from_millis(250);
const SEGMENT_WORKER_STOP_GRACE: Duration = Duration::from_millis(100);
const REQUEST_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
];
pub const PROTECTED_DOWNLOAD_AUTH_REQUIRED_CODE: &str = "PROTECTED_DOWNLOAD_AUTH_REQUIRED";
pub const PROTECTED_DOWNLOAD_AUTH_REQUIRED_MESSAGE: &str =
    "This site requires your browser session. Enable Protected Downloads or let the browser handle this download.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserHandoffAccessProbe {
    pub status: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserHandoffAccessError {
    pub code: &'static str,
    pub message: String,
    pub status: Option<u16>,
}

pub fn schedule_downloads(app: AppHandle, state: SharedState) {
    tauri::async_runtime::spawn(async move {
        match state.claim_schedulable_jobs().await {
            Ok((snapshot, tasks)) => {
                if !tasks.is_empty() {
                    emit_snapshot(&app, &snapshot);
                }

                for task in tasks {
                    start_download_worker(app.clone(), state.clone(), task);
                }

                let warmup_candidates = state.datanodes_hoster_warmup_candidates().await;
                if !warmup_candidates.is_empty() {
                    spawn_datanodes_hoster_warmups(app.clone(), state.clone(), warmup_candidates);
                }
            }
            Err(error) => eprintln!("failed to claim queued jobs: {error}"),
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TorrentEngineConfig {
    default_output_folder: PathBuf,
    data_dir: PathBuf,
    port_forwarding_enabled: bool,
    port_forwarding_port: u32,
}

impl TorrentEngineConfig {
    fn from_settings(settings: &Settings, data_dir: PathBuf) -> Self {
        let default_output_folder = if settings.torrent.download_directory.trim().is_empty() {
            PathBuf::from(default_torrent_download_directory_for(
                &settings.download_directory,
            ))
        } else {
            PathBuf::from(&settings.torrent.download_directory)
        };

        Self {
            default_output_folder,
            data_dir,
            port_forwarding_enabled: settings.torrent.port_forwarding_enabled,
            port_forwarding_port: settings.torrent.port_forwarding_port,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TorrentEngineRefreshAction {
    Create,
    Reuse,
    Recreate,
    Defer,
}

fn torrent_engine_refresh_action(
    current: Option<&TorrentEngineConfig>,
    desired: &TorrentEngineConfig,
    has_blocking_torrent_work: bool,
) -> TorrentEngineRefreshAction {
    match current {
        None => TorrentEngineRefreshAction::Create,
        Some(current) if current == desired => TorrentEngineRefreshAction::Reuse,
        Some(_) if has_blocking_torrent_work => TorrentEngineRefreshAction::Defer,
        Some(_) => TorrentEngineRefreshAction::Recreate,
    }
}

struct TorrentEngineSlot {
    config: TorrentEngineConfig,
    engine: Arc<TorrentEngine>,
}

#[derive(Default)]
struct TorrentEngineManager {
    slot: Mutex<Option<TorrentEngineSlot>>,
}

impl TorrentEngineManager {
    async fn get_or_create(&self, state: &SharedState) -> Result<Arc<TorrentEngine>, String> {
        let settings = state.settings().await;
        let config = TorrentEngineConfig::from_settings(&settings, state.app_data_dir());
        let has_blocking_torrent_work = state.has_torrent_engine_blocking_work().await;
        let mut slot = self.slot.lock().await;

        match torrent_engine_refresh_action(
            slot.as_ref().map(|slot| &slot.config),
            &config,
            has_blocking_torrent_work,
        ) {
            TorrentEngineRefreshAction::Reuse | TorrentEngineRefreshAction::Defer => {
                if let Some(slot) = slot.as_ref() {
                    slot.engine
                        .set_upload_limit(settings.torrent.upload_limit_kib_per_second);
                    return Ok(slot.engine.clone());
                }
            }
            TorrentEngineRefreshAction::Create | TorrentEngineRefreshAction::Recreate => {}
        }

        let engine = TorrentEngine::new(
            config.default_output_folder.clone(),
            config.data_dir.clone(),
            settings.torrent.clone(),
        )
        .await
        .map(Arc::new)?;
        engine.set_upload_limit(settings.torrent.upload_limit_kib_per_second);
        *slot = Some(TorrentEngineSlot {
            config,
            engine: engine.clone(),
        });
        Ok(engine)
    }

    async fn refresh_runtime_settings(&self, state: &SharedState) -> Result<(), String> {
        let settings = state.settings().await;
        let config = TorrentEngineConfig::from_settings(&settings, state.app_data_dir());
        let has_blocking_torrent_work = state.has_torrent_engine_blocking_work().await;
        let mut record_deferred_warning = false;

        {
            let mut slot = self.slot.lock().await;
            let Some(current_slot) = slot.as_ref() else {
                return Ok(());
            };

            current_slot
                .engine
                .set_upload_limit(settings.torrent.upload_limit_kib_per_second);

            match torrent_engine_refresh_action(
                Some(&current_slot.config),
                &config,
                has_blocking_torrent_work,
            ) {
                TorrentEngineRefreshAction::Reuse | TorrentEngineRefreshAction::Create => {}
                TorrentEngineRefreshAction::Recreate => {
                    *slot = None;
                }
                TorrentEngineRefreshAction::Defer => {
                    record_deferred_warning = true;
                }
            }
        }

        if record_deferred_warning {
            record_deferred_torrent_engine_settings_refresh(state).await?;
        }

        Ok(())
    }

    async fn clear_if_idle(&self, state: &SharedState) -> Result<(), String> {
        if state.has_torrent_engine_blocking_work().await {
            return Err(
                "Pause active, queued, or seeding torrents before resetting the torrent engine."
                    .into(),
            );
        }

        let mut slot = self.slot.lock().await;
        *slot = None;
        Ok(())
    }

    async fn current_engine(&self) -> Option<Arc<TorrentEngine>> {
        self.slot
            .lock()
            .await
            .as_ref()
            .map(|slot| slot.engine.clone())
    }

    #[cfg(test)]
    async fn current_config(&self) -> Option<TorrentEngineConfig> {
        self.slot
            .lock()
            .await
            .as_ref()
            .map(|slot| slot.config.clone())
    }
}

fn torrent_engine_manager() -> &'static TorrentEngineManager {
    TORRENT_ENGINE_MANAGER.get_or_init(TorrentEngineManager::default)
}

pub async fn apply_torrent_runtime_settings(state: &SharedState) -> Result<(), String> {
    torrent_engine_manager()
        .refresh_runtime_settings(state)
        .await
}

pub async fn clear_in_memory_torrent_engine_if_idle(state: &SharedState) -> Result<(), String> {
    torrent_engine_manager().clear_if_idle(state).await
}

async fn record_deferred_torrent_engine_settings_refresh(
    state: &SharedState,
) -> Result<(), String> {
    state
        .record_diagnostic_event(
            DiagnosticLevel::Warning,
            "torrent",
            "Torrent listener or port-forwarding changes will apply after active torrents stop or after restart.",
            None,
        )
        .await
}

async fn current_torrent_engine() -> Option<Arc<TorrentEngine>> {
    torrent_engine_manager().current_engine().await
}

async fn managed_torrent_engine(state: &SharedState) -> Result<Arc<TorrentEngine>, String> {
    torrent_engine_manager().get_or_create(state).await
}

pub async fn forget_torrent_session_for_restart(
    state: &SharedState,
    torrent: &TorrentInfo,
) -> Result<(), String> {
    if torrent.engine_id.is_none() && torrent.info_hash.is_none() {
        return Ok(());
    }

    let engine = torrent_engine(state).await?;
    engine
        .forget_existing(torrent.engine_id, torrent.info_hash.as_deref())
        .await?;
    Ok(())
}

pub async fn forget_known_torrent_sessions(torrents: &[TorrentInfo]) -> Result<(), String> {
    let Some(engine) = current_torrent_engine().await else {
        return Ok(());
    };

    for torrent in torrents {
        if torrent.engine_id.is_none() && torrent.info_hash.is_none() {
            continue;
        }

        engine
            .forget_existing(torrent.engine_id, torrent.info_hash.as_deref())
            .await?;
    }

    Ok(())
}

pub async fn retry_bulk_archive(
    app: &AppHandle,
    state: &SharedState,
    archive_id: &str,
) -> Result<(), String> {
    retry_bulk_archive_creation(app, state, archive_id).await
}

pub async fn schedule_external_reseed(app: AppHandle, state: SharedState, id: String) {
    state.begin_external_reseed(&id).await;

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS)).await;

            match state.queue_external_reseed_attempt(&id).await {
                Ok(ExternalReseedAttempt::Queued(snapshot)) => {
                    emit_snapshot(&app, &snapshot);
                    schedule_downloads(app.clone(), state.clone());
                }
                Ok(ExternalReseedAttempt::Pending) => {}
                Ok(ExternalReseedAttempt::Stop) => break,
                Err(error) => {
                    eprintln!("failed to queue automatic torrent reseed: {error}");
                    break;
                }
            }
        }
    });
}

fn start_download_worker(app: AppHandle, state: SharedState, task: crate::state::DownloadTask) {
    tauri::async_runtime::spawn(async move {
        let cleanup_temp_on_exit = matches!(
            state.worker_control(&task.id).await,
            WorkerControl::Canceled
        );

        let mut deferred_delay = None;
        let mut clear_handoff_auth = true;
        match run_download(&app, &state, &task).await {
            Ok(DownloadOutcome::Completed) => {}
            Ok(DownloadOutcome::Paused) | Ok(DownloadOutcome::Canceled) => {
                if let Ok(snapshot) = state.finish_interrupted_job(&task.id).await {
                    emit_snapshot(&app, &snapshot);
                }

                if cleanup_temp_on_exit
                    || matches!(
                        state.worker_control(&task.id).await,
                        WorkerControl::Canceled | WorkerControl::Missing
                    )
                {
                    cleanup_partial_artifacts(&task.temp_path).await;
                }
            }
            Ok(DownloadOutcome::Deferred(delay)) => {
                deferred_delay = Some(delay);
                clear_handoff_auth = false;
            }
            Err(error) => {
                if let Ok(Some(snapshot)) = state
                    .handle_external_reseed_failure(&task.id, error.message.clone(), error.category)
                    .await
                {
                    emit_snapshot(&app, &snapshot);
                } else if is_torrent_seeding_restore_task(&task) {
                    match state
                        .handle_torrent_seeding_restore_failure(
                            &task.id,
                            error.message.clone(),
                            error.category,
                        )
                        .await
                    {
                        Ok(Some(handled)) => {
                            emit_snapshot(&app, &handled.snapshot);
                            if handled.retry_reseed {
                                schedule_external_reseed(
                                    app.clone(),
                                    state.clone(),
                                    task.id.clone(),
                                )
                                .await;
                            }
                        }
                        Ok(None) => {
                            if let Ok(snapshot) = state
                                .fail_job(&task.id, error.message.clone(), error.category)
                                .await
                            {
                                emit_snapshot(&app, &snapshot);
                                notify_download_failure(
                                    &app,
                                    &state,
                                    &task,
                                    snapshot
                                        .jobs
                                        .iter()
                                        .find(|job| job.id == task.id)
                                        .and_then(|job| job.error.as_deref()),
                                )
                                .await;
                            }
                        }
                        Err(handle_error) => {
                            eprintln!(
                                "failed to pause torrent seeding restore after error: {handle_error}"
                            );
                            if let Ok(snapshot) = state
                                .fail_job(&task.id, error.message.clone(), error.category)
                                .await
                            {
                                emit_snapshot(&app, &snapshot);
                                notify_download_failure(
                                    &app,
                                    &state,
                                    &task,
                                    snapshot
                                        .jobs
                                        .iter()
                                        .find(|job| job.id == task.id)
                                        .and_then(|job| job.error.as_deref()),
                                )
                                .await;
                            }
                        }
                    }
                } else {
                    match try_auto_restart_failed_bulk_member(&app, &state, &task, &error).await {
                        BulkMemberAutoRestartOutcome::Restarted => {}
                        BulkMemberAutoRestartOutcome::NotEligible => {
                            fail_job_and_notify(&app, &state, &task, &error).await;
                        }
                        BulkMemberAutoRestartOutcome::Failed(recovery_error) => {
                            fail_job_and_notify(&app, &state, &task, &recovery_error).await;
                        }
                    }
                }
            }
        }

        if clear_handoff_auth {
            state.clear_handoff_auth(&task.id).await;
        }
        if let Some(delay) = deferred_delay {
            let deferred_app = app.clone();
            let deferred_state = state.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(delay).await;
                schedule_downloads(deferred_app, deferred_state);
            });
        }
        schedule_downloads(app, state);
    });
}

enum BulkMemberAutoRestartOutcome {
    Restarted,
    NotEligible,
    Failed(DownloadError),
}

async fn try_auto_restart_failed_bulk_member(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
    error: &DownloadError,
) -> BulkMemberAutoRestartOutcome {
    let candidate = match state
        .bulk_member_auto_restart_candidate(
            &task.id,
            error.category,
            &error.message,
            error.retryable,
        )
        .await
    {
        Ok(Some(candidate)) => candidate,
        Ok(None) => return BulkMemberAutoRestartOutcome::NotEligible,
        Err(message) => {
            return BulkMemberAutoRestartOutcome::Failed(download_error(
                FailureCategory::Internal,
                format!("Could not prepare bulk member auto-restart: {message}"),
                false,
            ))
        }
    };

    let resolved_url = if let Some(source_url) = candidate.resolved_from_url.as_deref() {
        source_url.to_string()
    } else {
        task.url.clone()
    };

    if candidate.mode == BulkMemberAutoRestartMode::ResetPartial {
        cleanup_partial_artifacts(&task.temp_path).await;
    }

    match state
        .auto_restart_bulk_member(
            &task.id,
            resolved_url,
            candidate.mode,
            candidate.attempt,
            candidate.max_attempts,
            error.category,
            &error.message,
        )
        .await
    {
        Ok(snapshot) => {
            emit_snapshot(app, &snapshot);
            BulkMemberAutoRestartOutcome::Restarted
        }
        Err(message) => BulkMemberAutoRestartOutcome::Failed(download_error(
            auto_restart_reset_failure_category(&message),
            format!("Could not auto-restart bulk member: {message}"),
            false,
        )),
    }
}

fn auto_restart_reset_failure_category(message: &str) -> FailureCategory {
    if message.contains("partial download file") || message.contains("download directory") {
        FailureCategory::Disk
    } else {
        FailureCategory::Internal
    }
}

async fn fail_job_and_notify(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
    error: &DownloadError,
) {
    if let Ok(snapshot) = state
        .fail_job(&task.id, error.message.clone(), error.category)
        .await
    {
        emit_snapshot(app, &snapshot);
        notify_download_failure(
            app,
            state,
            task,
            snapshot
                .jobs
                .iter()
                .find(|job| job.id == task.id)
                .and_then(|job| job.error.as_deref()),
        )
        .await;
    }
}

async fn run_download(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
) -> Result<DownloadOutcome, DownloadError> {
    let max_retry_attempts = state.auto_retry_attempts_for_job(&task.id).await;
    let mut retry_attempts = task.retry_attempts.min(max_retry_attempts);

    loop {
        match run_transfer_attempt(app, state, task).await {
            Ok(outcome) => return Ok(outcome),
            Err(error) => {
                let has_valid_partial = state.has_recoverable_partial_download(&task.id).await;
                match retry_decision_for_attempt_error(
                    &error,
                    retry_attempts,
                    max_retry_attempts,
                    has_valid_partial,
                ) {
                    RetryDecision::Retry => {
                        retry_attempts += 1;
                        let snapshot = state.record_retry_attempt(&task.id, retry_attempts).await?;
                        emit_snapshot(app, &snapshot);
                        tokio::time::sleep(retry_delay_for_attempt_with_jitter(
                            (retry_attempts - 1) as usize,
                            &task.id,
                            &task.url,
                        ))
                        .await;
                    }
                    RetryDecision::PauseRecoverably => {
                        let message = recoverable_retry_pause_message(&error, retry_attempts);
                        let snapshot = state
                            .pause_job_after_retry_exhaustion(&task.id, message, error.category)
                            .await?;
                        emit_snapshot(app, &snapshot);
                        return Ok(DownloadOutcome::Paused);
                    }
                    RetryDecision::Fail => return Err(error),
                }
            }
        }
    }
}

static CLIENT: OnceLock<Client> = OnceLock::new();
static TORRENT_ENGINE_MANAGER: OnceLock<TorrentEngineManager> = OnceLock::new();
static RANGE_BACKOFFS: OnceLock<RangeBackoffRegistry> = OnceLock::new();

pub async fn probe_browser_handoff_access(
    url: &str,
    handoff_auth: Option<&HandoffAuth>,
) -> Result<BrowserHandoffAccessProbe, BrowserHandoffAccessError> {
    let client = download_client().map_err(|error| BrowserHandoffAccessError {
        code: "DOWNLOAD_FAILED",
        message: error.message,
        status: None,
    })?;
    let mut current_url = url.to_string();
    let mut redirects = 0;

    loop {
        let request = client
            .get(&current_url)
            .timeout(PREFLIGHT_TIMEOUT)
            .header(ACCEPT_ENCODING, "identity")
            .header(RANGE, "bytes=0-0");
        let request = apply_handoff_auth_headers(request, handoff_auth)
            .map_err(access_probe_download_error)?;

        let response = request
            .send()
            .await
            .map_err(|error| BrowserHandoffAccessError {
                code: "DOWNLOAD_FAILED",
                message: format!("Could not access protected browser download: {error}"),
                status: None,
            })?;

        if response.status().is_redirection() {
            let next_url = redirect_location(response.url().as_str(), &response)
                .map_err(access_probe_download_error)?;
            if handoff_auth.is_some() && !redirect_keeps_origin(response.url().as_str(), &next_url)
            {
                return Err(BrowserHandoffAccessError {
                    code: "DOWNLOAD_FAILED",
                    message: "Authenticated download redirected to another origin; refusing to forward browser credentials."
                        .into(),
                    status: Some(response.status().as_u16()),
                });
            }
            redirects += 1;
            if redirects > 10 {
                return Err(BrowserHandoffAccessError {
                    code: "DOWNLOAD_FAILED",
                    message: "Download access probe redirected too many times.".into(),
                    status: Some(response.status().as_u16()),
                });
            }
            current_url = next_url;
            continue;
        }

        let status = response.status();
        if status.is_success() {
            return Ok(BrowserHandoffAccessProbe {
                status: status.as_u16(),
            });
        }

        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err(BrowserHandoffAccessError {
                code: PROTECTED_DOWNLOAD_AUTH_REQUIRED_CODE,
                message: PROTECTED_DOWNLOAD_AUTH_REQUIRED_MESSAGE.into(),
                status: Some(status.as_u16()),
            });
        }

        return Err(BrowserHandoffAccessError {
            code: "DOWNLOAD_FAILED",
            message: format!("Download access probe failed with HTTP {status}."),
            status: Some(status.as_u16()),
        });
    }
}
async fn run_transfer_attempt(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
) -> Result<DownloadOutcome, DownloadError> {
    match transfer_dispatch_for_kind(task.transfer_kind) {
        Some(TransferDispatch::Http) => run_http_download_attempt(app, state, task).await,
        Some(TransferDispatch::Torrent) => run_torrent_download_attempt(app, state, task).await,
        None => Err(download_error(
            FailureCategory::Internal,
            "Unsupported transfer kind.".into(),
            false,
        )),
    }
}
