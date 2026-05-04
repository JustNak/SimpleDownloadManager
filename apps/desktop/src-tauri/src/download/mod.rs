use crate::commands::{emit_download_update, emit_snapshot};
use crate::state::{
    should_stop_seeding, BulkArchiveReady, ExternalReseedAttempt, SharedState, TorrentRuntimePhase,
    TorrentRuntimeSnapshot, WorkerControl,
};
use crate::storage::{
    default_torrent_download_directory_for, BulkArchiveOutputKind, BulkArchiveStatus,
    DiagnosticLevel, DownloadPerformanceMode, FailureCategory, HandoffAuth, JobState,
    ResumeSupport, TorrentInfo, TorrentPeerConnectionWatchdogMode, TorrentSettings, TransferKind,
};
use crate::torrent::{
    cached_torrent_metadata_source, pending_torrent_cleanup_info_hash, prepare_torrent_source,
    PreparedTorrentSource, TorrentAddSessionOutcome, TorrentEngine, TorrentSourceKind,
    TrackerFirstMetadataOutcome,
};
use futures_util::StreamExt;
use percent_encoding::percent_decode_str;
use reqwest::header::{
    HeaderName, HeaderValue, ACCEPT_ENCODING, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_RANGE,
    LOCATION, RANGE,
};
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::plugin::PermissionState;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, Mutex, OnceCell};

mod archive;
mod client;
mod filesystem;
mod http;
mod notifications;
mod segmented;
mod torrent;
mod types;

use archive::*;
use client::*;
use filesystem::*;
use http::*;
use notifications::*;
use segmented::*;
use torrent::*;
use types::*;

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
            }
            Err(error) => eprintln!("failed to claim queued jobs: {error}"),
        }
    });
}

pub fn apply_torrent_runtime_settings(settings: &TorrentSettings) {
    if let Some(engine) = TORRENT_ENGINE.get() {
        engine.set_upload_limit(settings.upload_limit_kib_per_second);
    }
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
    let Some(engine) = TORRENT_ENGINE.get() else {
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
                } else if let Ok(snapshot) = state
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

        state.clear_handoff_auth(&task.id).await;
        schedule_downloads(app, state);
    });
}

async fn run_download(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
) -> Result<DownloadOutcome, DownloadError> {
    let max_retry_attempts = state.auto_retry_attempts().await;
    let mut retry_attempts = 0;

    loop {
        match run_transfer_attempt(app, state, task).await {
            Ok(outcome) => return Ok(outcome),
            Err(error) if error.retryable && retry_attempts < max_retry_attempts => {
                retry_attempts += 1;
                let snapshot = state.record_retry_attempt(&task.id, retry_attempts).await?;
                emit_snapshot(app, &snapshot);
                tokio::time::sleep(retry_delay_for_attempt((retry_attempts - 1) as usize)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

static CLIENT: OnceLock<Client> = OnceLock::new();
static TORRENT_ENGINE: OnceCell<Arc<TorrentEngine>> = OnceCell::const_new();
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
