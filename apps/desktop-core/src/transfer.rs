use crate::contracts::{
    BrowserDownloadAccessError as BrowserHandoffAccessError,
    BrowserDownloadAccessProbe as BrowserHandoffAccessProbe, DesktopEvent, ShellServices,
};
use crate::state::{BulkArchiveReady, DownloadTask, SharedState, WorkerControl};
use crate::storage::{
    BulkArchiveStatus, DiagnosticLevel, DownloadPerformanceMode, FailureCategory, HandoffAuth,
    ResumeSupport,
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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::Mutex;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(120);
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(8);
const PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_millis(750);
const PROGRESS_PERSIST_INTERVAL: Duration = Duration::from_secs(5);
const THROTTLE_CONTROL_INTERVAL: Duration = Duration::from_millis(250);
const DOWNLOAD_BUFFER_SIZE: usize = 512 * 1024;
const SEGMENT_COMBINE_BUFFER_SIZE: usize = 1024 * 1024;
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

#[derive(Clone)]
pub struct TransferShell {
    shell: Arc<dyn ShellServices>,
}

impl TransferShell {
    pub fn new<S>(shell: S) -> Self
    where
        S: ShellServices + 'static,
    {
        Self {
            shell: Arc::new(shell),
        }
    }
}

type AppHandle = TransferShell;

pub async fn run_http_download(
    app: &AppHandle,
    state: &SharedState,
    task: &DownloadTask,
) -> Result<DownloadOutcome, DownloadError> {
    let max_retry_attempts = state.auto_retry_attempts().await;
    let mut retry_attempts = 0;

    loop {
        match run_http_download_attempt(app, state, task).await {
            Ok(outcome) => return Ok(outcome),
            Err(error) if error.retryable && retry_attempts < max_retry_attempts => {
                retry_attempts += 1;
                let snapshot = state.record_retry_attempt(&task.id, retry_attempts).await?;
                emit_snapshot(app, &snapshot).await;
                tokio::time::sleep(retry_delay_for_attempt((retry_attempts - 1) as usize)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn emit_snapshot(app: &AppHandle, snapshot: &crate::storage::DesktopSnapshot) {
    let _ = app
        .shell
        .emit_event(DesktopEvent::StateChanged(Box::new(snapshot.clone())))
        .await;
}

pub fn plan_segmented_ranges_for_mode(
    total_bytes: u64,
    resume_support: ResumeSupport,
    speed_limit: Option<u64>,
    mode: DownloadPerformanceMode,
) -> Option<RangePlan> {
    plan_segmented_ranges(
        total_bytes,
        resume_support,
        speed_limit,
        performance_profile(mode),
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadOutcome {
    Completed,
    Paused,
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    fn len(self) -> u64 {
        self.end.saturating_sub(self.start).saturating_add(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangePlan {
    pub total_bytes: u64,
    pub segments: Vec<ByteRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SegmentProgress {
    index: usize,
    range: ByteRange,
    completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SegmentedDownloadState {
    pub total_bytes: u64,
    segments: Vec<SegmentProgress>,
}

struct SegmentedProgressCounters {
    segment_bytes: Vec<AtomicU64>,
    sample_bytes: AtomicU64,
}

impl SegmentedProgressCounters {
    fn new(segment_bytes: Vec<u64>) -> Self {
        Self {
            segment_bytes: segment_bytes.into_iter().map(AtomicU64::new).collect(),
            sample_bytes: AtomicU64::new(0),
        }
    }

    fn store_segment_bytes(&self, segment_index: usize, bytes: u64) {
        if let Some(segment_bytes) = self.segment_bytes.get(segment_index) {
            segment_bytes.store(bytes, Ordering::Relaxed);
        }
    }

    fn add_sample_bytes(&self, bytes: u64) {
        self.sample_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    fn drain_sample_bytes(&self) -> u64 {
        self.sample_bytes.swap(0, Ordering::Relaxed)
    }

    fn total_downloaded(&self) -> u64 {
        self.segment_bytes
            .iter()
            .map(|bytes| bytes.load(Ordering::Relaxed))
            .sum()
    }
}

#[derive(Clone)]
struct SegmentWorkerContext {
    state: SharedState,
    client: Client,
    job_id: String,
    url: String,
    handoff_auth: Option<HandoffAuth>,
    temp_path: PathBuf,
    pub total_bytes: u64,
    profile: DownloadPerformanceProfile,
    progress: Arc<SegmentedProgressCounters>,
    metadata: Arc<Mutex<SegmentedDownloadState>>,
}

#[derive(Debug, Clone, Copy)]
struct DownloadPerformanceProfile {
    max_segments: usize,
    min_segmented_size: u64,
    target_segment_size: u64,
    low_speed_threshold_bytes_per_second: u64,
    low_speed_window: Duration,
    max_low_speed_retries: u32,
    speed_smoothing_alpha: f64,
}

#[derive(Debug)]
struct RollingSpeed {
    smoothed_bytes_per_second: Option<f64>,
    alpha: f64,
}

impl Default for RollingSpeed {
    fn default() -> Self {
        Self {
            smoothed_bytes_per_second: None,
            alpha: 0.25,
        }
    }
}

impl RollingSpeed {
    fn with_alpha(alpha: f64) -> Self {
        Self {
            smoothed_bytes_per_second: None,
            alpha: alpha.clamp(0.05, 1.0),
        }
    }
}

impl RollingSpeed {
    fn record_sample(&mut self, bytes: u64, elapsed: Duration) -> u64 {
        if elapsed.is_zero() {
            return self
                .smoothed_bytes_per_second
                .map(|value| value as u64)
                .unwrap_or(0);
        }

        let sample = bytes as f64 / elapsed.as_secs_f64();
        let next = match self.smoothed_bytes_per_second {
            Some(previous) => previous.mul_add(1.0 - self.alpha, sample * self.alpha),
            None => sample,
        };
        self.smoothed_bytes_per_second = Some(next);
        next.max(0.0) as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LowSpeedDecision {
    Continue,
    Retry,
}

#[derive(Debug)]
struct LowSpeedMonitor {
    threshold_bytes_per_second: u64,
    window: Duration,
    max_retries: u32,
    retries: u32,
}

impl LowSpeedMonitor {
    fn new(profile: DownloadPerformanceProfile) -> Self {
        Self {
            threshold_bytes_per_second: profile.low_speed_threshold_bytes_per_second,
            window: profile.low_speed_window,
            max_retries: profile.max_low_speed_retries,
            retries: 0,
        }
    }

    fn observe(&mut self, bytes: u64, elapsed: Duration, speed_limited: bool) -> LowSpeedDecision {
        if speed_limited
            || elapsed < self.window
            || self.threshold_bytes_per_second == 0
            || self.retries >= self.max_retries
        {
            return LowSpeedDecision::Continue;
        }

        let speed = if elapsed.is_zero() {
            0
        } else {
            (bytes as f64 / elapsed.as_secs_f64()) as u64
        };

        if speed < self.threshold_bytes_per_second {
            self.retries += 1;
            LowSpeedDecision::Retry
        } else {
            LowSpeedDecision::Continue
        }
    }
}

#[derive(Debug)]
pub struct DownloadError {
    pub category: FailureCategory,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreflightMetadata {
    total_bytes: Option<u64>,
    resume_support: ResumeSupport,
    filename: Option<String>,
}

impl From<String> for DownloadError {
    fn from(message: String) -> Self {
        download_error(FailureCategory::Internal, message, false)
    }
}

fn download_client() -> Result<Client, DownloadError> {
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }

    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .pool_idle_timeout(Some(Duration::from_secs(120)))
        .pool_max_idle_per_host(16)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .http2_adaptive_window(true)
        .redirect(Policy::none())
        .user_agent("SimpleDownloadManager/0.2")
        .build()
        .map_err(|error| format!("Could not create download client: {error}"))?;

    let _ = CLIENT.set(client);
    CLIENT.get().cloned().ok_or_else(|| {
        "Could not initialize shared download client."
            .to_string()
            .into()
    })
}

static CLIENT: OnceLock<Client> = OnceLock::new();
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

fn access_probe_download_error(error: DownloadError) -> BrowserHandoffAccessError {
    BrowserHandoffAccessError {
        code: "DOWNLOAD_FAILED",
        message: error.message,
        status: None,
    }
}

#[derive(Default)]
struct RangeBackoffRegistry {
    rejected_hosts: StdMutex<HashMap<String, Instant>>,
}

impl RangeBackoffRegistry {
    fn record_rejection(&self, url: &str, now: Instant) {
        let Some(key) = range_backoff_key(url) else {
            return;
        };

        if let Ok(mut rejected_hosts) = self.rejected_hosts.lock() {
            rejected_hosts.insert(key, now);
        }
    }

    fn is_backed_off(&self, url: &str, now: Instant) -> bool {
        let Some(key) = range_backoff_key(url) else {
            return false;
        };

        let Ok(mut rejected_hosts) = self.rejected_hosts.lock() else {
            return false;
        };

        let Some(rejected_at) = rejected_hosts.get(&key).copied() else {
            return false;
        };

        if now.duration_since(rejected_at) < RANGE_BACKOFF_DURATION {
            return true;
        }

        rejected_hosts.remove(&key);
        false
    }
}

fn range_backoffs() -> &'static RangeBackoffRegistry {
    RANGE_BACKOFFS.get_or_init(RangeBackoffRegistry::default)
}

fn range_backoff_key(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    Some(format!(
        "{}://{}:{}",
        parsed.scheme(),
        host.to_ascii_lowercase(),
        parsed.port_or_known_default().unwrap_or(0)
    ))
}

pub async fn run_http_download_attempt(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
) -> Result<DownloadOutcome, DownloadError> {
    ensure_parent_directory(&task.target_path)
        .await
        .map_err(disk_error)?;

    let mut existing_bytes = metadata_len(&task.temp_path).await.unwrap_or(0);
    let client = download_client()?;
    let speed_limit = state.speed_limit_bytes_per_second().await;
    let profile = performance_profile(state.download_performance_mode().await);

    let mut preflight_metadata =
        preflight_download(&client, &task.url, task.handoff_auth.as_ref()).await;

    if existing_bytes == 0
        && speed_limit.is_none()
        && profile.max_segments >= 2
        && !range_backoffs().is_backed_off(&task.url, Instant::now())
    {
        let probe_metadata =
            probe_range_metadata(&client, &task.url, task.handoff_auth.as_ref()).await;
        match probe_metadata {
            Some(metadata) => {
                preflight_metadata = Some(merge_preflight_metadata(preflight_metadata, metadata));
            }
            None => {
                range_backoffs().record_rejection(&task.url, Instant::now());
            }
        }

        if let Some(metadata) = preflight_metadata.as_ref() {
            if let Some(total_bytes) = metadata.total_bytes {
                if let Some(plan) = plan_segmented_ranges(
                    total_bytes,
                    metadata.resume_support,
                    speed_limit,
                    profile,
                ) {
                    return run_segmented_download_attempt(app, state, task, client, plan, profile)
                        .await;
                }
            }
        }
    }

    let mut response = send_request(
        &client,
        &task.url,
        existing_bytes,
        task.handoff_auth.as_ref(),
    )
    .await?;
    let supports_resume = response.status() == StatusCode::PARTIAL_CONTENT;

    if existing_bytes > 0 && !supports_resume {
        truncate_file(&task.temp_path).await.map_err(disk_error)?;
        existing_bytes = 0;
        let snapshot = state
            .mark_job_downloading(
                &task.id,
                0,
                response.content_length(),
                ResumeSupport::Unsupported,
                extract_filename(&response),
            )
            .await?;
        emit_snapshot(app, &snapshot).await;
        response = send_request(&client, &task.url, 0, task.handoff_auth.as_ref()).await?;
    }

    let total_bytes = derive_total_bytes(&response, existing_bytes).or_else(|| {
        preflight_metadata
            .as_ref()
            .and_then(|metadata| metadata.total_bytes)
    });
    let resume_support = derive_resume_support(&response, existing_bytes);
    let display_filename = extract_filename(&response)
        .or_else(|| {
            preflight_metadata
                .as_ref()
                .and_then(|metadata| metadata.filename.clone())
        })
        .or_else(|| derive_filename_from_url(response.url().as_str()));
    let target_path = derive_target_path(&task.target_path, &response);
    let snapshot = state
        .mark_job_downloading(
            &task.id,
            existing_bytes,
            total_bytes,
            resume_support,
            display_filename,
        )
        .await?;
    emit_snapshot(app, &snapshot).await;

    let file = if existing_bytes > 0 {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&task.temp_path)
            .await
            .map_err(|error| disk_error(format!("Could not open partial download file: {error}")))?
    } else {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&task.temp_path)
            .await
            .map_err(|error| disk_error(format!("Could not create download file: {error}")))?
    };
    let mut file = BufWriter::with_capacity(DOWNLOAD_BUFFER_SIZE, file);

    let mut stream = response.bytes_stream();
    let mut downloaded_bytes = existing_bytes;
    let attempt_started = Instant::now();
    let mut attempt_transferred_bytes = 0_u64;
    let mut sample_bytes = 0_u64;
    let mut sample_started = Instant::now();
    let mut displayed_speed = RollingSpeed::with_alpha(profile.speed_smoothing_alpha);
    let mut low_speed_monitor = LowSpeedMonitor::new(profile);
    let mut low_speed_bytes = 0_u64;
    let mut low_speed_started = Instant::now();
    let mut last_emitted_bytes = existing_bytes;
    let mut last_persisted_at = Instant::now();

    while let Some(chunk_result) = stream.next().await {
        match state.worker_control(&task.id).await {
            WorkerControl::Paused => {
                file.flush().await.ok();
                return Ok(DownloadOutcome::Paused);
            }
            WorkerControl::Canceled => {
                file.flush().await.ok();
                return Ok(DownloadOutcome::Canceled);
            }
            WorkerControl::Missing => {
                file.flush().await.ok();
                return Ok(DownloadOutcome::Canceled);
            }
            WorkerControl::Continue => {}
        }

        let chunk = chunk_result.map_err(download_stream_error)?;
        let chunk_len = chunk.len() as u64;
        file.write_all(&chunk)
            .await
            .map_err(|error| disk_error(format!("Could not write download chunk: {error}")))?;

        downloaded_bytes = downloaded_bytes.saturating_add(chunk_len);
        attempt_transferred_bytes = attempt_transferred_bytes.saturating_add(chunk_len);
        sample_bytes = sample_bytes.saturating_add(chunk_len);

        if let Some(limit) = speed_limit {
            match throttle_download(
                state,
                &task.id,
                limit,
                attempt_transferred_bytes,
                attempt_started,
            )
            .await
            {
                WorkerControl::Paused => {
                    file.flush().await.ok();
                    return Ok(DownloadOutcome::Paused);
                }
                WorkerControl::Canceled | WorkerControl::Missing => {
                    file.flush().await.ok();
                    return Ok(DownloadOutcome::Canceled);
                }
                WorkerControl::Continue => {}
            }
        }

        let elapsed = sample_started.elapsed();

        low_speed_bytes = low_speed_bytes.saturating_add(chunk_len);
        if low_speed_started.elapsed() >= profile.low_speed_window {
            if low_speed_monitor.observe(
                low_speed_bytes,
                low_speed_started.elapsed(),
                speed_limit.is_some(),
            ) == LowSpeedDecision::Retry
            {
                file.flush().await.ok();
                return Err(download_error(
                    FailureCategory::Network,
                    "Download speed stayed below the recovery threshold; retrying the stream."
                        .into(),
                    true,
                ));
            }
            low_speed_bytes = 0;
            low_speed_started = Instant::now();
        }

        if elapsed >= PROGRESS_UPDATE_INTERVAL {
            let speed = displayed_speed.record_sample(sample_bytes, elapsed);
            let should_persist = last_persisted_at.elapsed() >= PROGRESS_PERSIST_INTERVAL;
            let snapshot = state
                .update_job_progress(
                    &task.id,
                    downloaded_bytes,
                    total_bytes,
                    speed,
                    should_persist,
                )
                .await?;
            emit_snapshot(app, &snapshot).await;
            last_emitted_bytes = downloaded_bytes;
            if should_persist {
                last_persisted_at = Instant::now();
            }
            sample_bytes = 0;
            sample_started = Instant::now();
        }
    }

    file.flush()
        .await
        .map_err(|error| disk_error(format!("Could not flush download file: {error}")))?;
    file.get_mut()
        .sync_all()
        .await
        .map_err(|error| disk_error(format!("Could not sync download file: {error}")))?;

    if let Some(total_bytes) = total_bytes {
        if downloaded_bytes < total_bytes {
            return Err(download_error(
                FailureCategory::Network,
                format!(
                    "Download ended early. Received {downloaded_bytes} of {total_bytes} bytes."
                ),
                true,
            ));
        }
    }

    if downloaded_bytes != last_emitted_bytes {
        let should_persist = last_persisted_at.elapsed() >= PROGRESS_PERSIST_INTERVAL;
        let snapshot = state
            .update_job_progress(&task.id, downloaded_bytes, total_bytes, 0, should_persist)
            .await?;
        emit_snapshot(app, &snapshot).await;
    }

    let final_path = move_to_final_path(&task.temp_path, &target_path)
        .await
        .map_err(disk_error)?;
    complete_http_download(app, state, task, downloaded_bytes, &final_path).await?;
    Ok(DownloadOutcome::Completed)
}

async fn complete_http_download(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
    total_bytes: u64,
    final_path: &Path,
) -> Result<(), DownloadError> {
    let actual_sha256 = if state.job_requires_sha256(&task.id).await {
        Some(compute_sha256(final_path).await.map_err(disk_error)?)
    } else {
        None
    };
    let snapshot = state
        .complete_job_with_integrity(&task.id, total_bytes, final_path, actual_sha256)
        .await?;
    let failed_integrity = snapshot.jobs.iter().any(|job| {
        job.id == task.id
            && job.state == crate::storage::JobState::Failed
            && job.failure_category == Some(FailureCategory::Integrity)
    });
    emit_snapshot(app, &snapshot).await;

    if failed_integrity {
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
        return Ok(());
    }

    handle_bulk_archive_after_completion(app, state, &task.id).await?;
    notify_download_completed(app, state, final_path).await;
    Ok(())
}

async fn handle_bulk_archive_after_completion(
    app: &AppHandle,
    state: &SharedState,
    job_id: &str,
) -> Result<(), String> {
    if let Some(archive) = state.bulk_archive_ready_for_job(job_id).await? {
        let archive_id = archive.archive_id.clone();
        let archive_output_path = archive.output_path.display().to_string();
        let snapshot = state
            .mark_bulk_archive_status(
                &archive_id,
                BulkArchiveStatus::Compressing,
                Some(archive_output_path.clone()),
                None,
            )
            .await?;
        emit_snapshot(app, &snapshot).await;

        match create_bulk_archive(archive).await {
            Ok(path) => {
                let snapshot = state
                    .mark_bulk_archive_status(
                        &archive_id,
                        BulkArchiveStatus::Completed,
                        Some(path.display().to_string()),
                        None,
                    )
                    .await?;
                emit_snapshot(app, &snapshot).await;
                notify_bulk_archive_completed(app, state, &path).await;
            }
            Err(error) => {
                let _ = state
                    .record_diagnostic_event(
                        DiagnosticLevel::Error,
                        "bulk_archive",
                        format!("Bulk archive failed: {error}"),
                        Some(job_id.into()),
                    )
                    .await;
                let snapshot = state
                    .mark_bulk_archive_status(
                        &archive_id,
                        BulkArchiveStatus::Failed,
                        Some(archive_output_path),
                        Some(error.clone()),
                    )
                    .await?;
                emit_snapshot(app, &snapshot).await;
                eprintln!("failed to create bulk archive: {error}");
            }
        }
    }

    Ok(())
}

fn performance_profile(mode: DownloadPerformanceMode) -> DownloadPerformanceProfile {
    match mode {
        DownloadPerformanceMode::Stable => DownloadPerformanceProfile {
            max_segments: 1,
            min_segmented_size: u64::MAX,
            target_segment_size: u64::MAX,
            low_speed_threshold_bytes_per_second: 4 * 1024,
            low_speed_window: Duration::from_secs(30),
            max_low_speed_retries: 2,
            speed_smoothing_alpha: 0.25,
        },
        DownloadPerformanceMode::Balanced => DownloadPerformanceProfile {
            max_segments: 6,
            min_segmented_size: BALANCED_MIN_SEGMENTED_SIZE,
            target_segment_size: BALANCED_TARGET_SEGMENT_SIZE,
            low_speed_threshold_bytes_per_second: 8 * 1024,
            low_speed_window: Duration::from_secs(20),
            max_low_speed_retries: 2,
            speed_smoothing_alpha: 0.25,
        },
        DownloadPerformanceMode::Fast => DownloadPerformanceProfile {
            max_segments: 12,
            min_segmented_size: FAST_MIN_SEGMENTED_SIZE,
            target_segment_size: FAST_TARGET_SEGMENT_SIZE,
            low_speed_threshold_bytes_per_second: 16 * 1024,
            low_speed_window: Duration::from_secs(15),
            max_low_speed_retries: 3,
            speed_smoothing_alpha: 0.25,
        },
    }
}

fn plan_segmented_ranges(
    total_bytes: u64,
    resume_support: ResumeSupport,
    speed_limit: Option<u64>,
    profile: DownloadPerformanceProfile,
) -> Option<RangePlan> {
    if speed_limit.is_some()
        || resume_support != ResumeSupport::Supported
        || total_bytes < profile.min_segmented_size
        || profile.max_segments < 2
    {
        return None;
    }

    let target_segment_size = profile.target_segment_size.max(1);
    let segment_count = profile
        .max_segments
        .min(total_bytes.div_ceil(target_segment_size).max(2) as usize)
        .max(2);
    let segment_size = total_bytes / segment_count as u64;
    let mut segments = Vec::with_capacity(segment_count);

    for index in 0..segment_count {
        let start = index as u64 * segment_size;
        let end = if index == segment_count - 1 {
            total_bytes - 1
        } else {
            ((index as u64 + 1) * segment_size).saturating_sub(1)
        };
        segments.push(ByteRange { start, end });
    }

    Some(RangePlan {
        total_bytes,
        segments,
    })
}

async fn probe_range_metadata(
    client: &Client,
    url: &str,
    handoff_auth: Option<&HandoffAuth>,
) -> Option<PreflightMetadata> {
    let response = send_range_request(client, url, ByteRange { start: 0, end: 0 }, handoff_auth)
        .await
        .ok()?;

    if response.status() != StatusCode::PARTIAL_CONTENT {
        return None;
    }

    let (range, total_bytes) = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range)?;

    if range != (ByteRange { start: 0, end: 0 }) || total_bytes == 0 {
        return None;
    }

    Some(PreflightMetadata {
        total_bytes: Some(total_bytes),
        resume_support: ResumeSupport::Supported,
        filename: extract_filename(&response)
            .or_else(|| derive_filename_from_url(response.url().as_str())),
    })
}

fn merge_preflight_metadata(
    existing: Option<PreflightMetadata>,
    probed: PreflightMetadata,
) -> PreflightMetadata {
    let Some(existing) = existing else {
        return probed;
    };

    PreflightMetadata {
        total_bytes: probed.total_bytes.or(existing.total_bytes),
        resume_support: if probed.resume_support == ResumeSupport::Supported {
            ResumeSupport::Supported
        } else {
            existing.resume_support
        },
        filename: existing.filename.or(probed.filename),
    }
}

async fn run_segmented_download_attempt(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
    client: Client,
    plan: RangePlan,
    profile: DownloadPerformanceProfile,
) -> Result<DownloadOutcome, DownloadError> {
    let mut segment_state = load_or_create_segment_state(&task.temp_path, &plan).await?;
    refresh_segment_completion_from_disk(&task.temp_path, &mut segment_state).await;
    persist_segment_state(&task.temp_path, &segment_state).await?;

    let initial_segment_bytes = segment_state
        .segments
        .iter()
        .map(|segment| segment_existing_len(&task.temp_path, segment))
        .collect::<Vec<_>>();
    let initial_downloaded = initial_segment_bytes.iter().sum::<u64>();

    let snapshot = state
        .mark_job_downloading(
            &task.id,
            initial_downloaded,
            Some(plan.total_bytes),
            ResumeSupport::Supported,
            None,
        )
        .await?;
    emit_snapshot(app, &snapshot).await;

    let progress = Arc::new(SegmentedProgressCounters::new(
        initial_segment_bytes.clone(),
    ));
    let reporter_stop = Arc::new(AtomicBool::new(false));
    let reporter_handle = tokio::spawn(report_segmented_progress(
        app.clone(),
        state.clone(),
        task.id.clone(),
        plan.total_bytes,
        profile,
        progress.clone(),
        reporter_stop.clone(),
    ));
    let metadata = Arc::new(Mutex::new(segment_state));
    let worker_context = SegmentWorkerContext {
        state: state.clone(),
        client: client.clone(),
        job_id: task.id.clone(),
        url: task.url.clone(),
        handoff_auth: task.handoff_auth.clone(),
        temp_path: task.temp_path.clone(),
        total_bytes: plan.total_bytes,
        profile,
        progress: progress.clone(),
        metadata: metadata.clone(),
    };

    let mut handles = Vec::new();
    for segment in plan.segments.iter().copied().enumerate() {
        let (index, range) = segment;
        if initial_segment_bytes[index] >= range.len() {
            continue;
        }

        handles.push(tokio::spawn(download_segment_worker(
            worker_context.clone(),
            SegmentProgress {
                index,
                range,
                completed: false,
            },
        )));
    }

    let mut worker_outcome = DownloadOutcome::Completed;
    let mut worker_error = None::<DownloadError>;
    while let Some(handle) = handles.pop() {
        match handle.await {
            Ok(Ok(DownloadOutcome::Completed)) => {}
            Ok(Ok(outcome @ (DownloadOutcome::Paused | DownloadOutcome::Canceled))) => {
                worker_outcome = outcome;
                for handle in handles {
                    handle.abort();
                }
                break;
            }
            Ok(Err(error)) => {
                worker_error = Some(error);
                for handle in handles {
                    handle.abort();
                }
                break;
            }
            Err(error) => {
                worker_error = Some(download_error(
                    FailureCategory::Internal,
                    format!("Segment worker failed: {error}"),
                    true,
                ));
                for handle in handles {
                    handle.abort();
                }
                break;
            }
        }
    }

    reporter_stop.store(true, Ordering::Relaxed);
    match reporter_handle.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) if worker_error.is_none() => worker_error = Some(error),
        Ok(Err(_)) => {}
        Err(error) if worker_error.is_none() => {
            worker_error = Some(download_error(
                FailureCategory::Internal,
                format!("Segment progress reporter failed: {error}"),
                true,
            ));
        }
        Err(_) => {}
    }

    if let Some(error) = worker_error {
        return Err(error);
    }

    if worker_outcome != DownloadOutcome::Completed {
        return Ok(worker_outcome);
    }

    let final_state = metadata.lock().await.clone();
    if !final_state.segments.iter().all(|segment| segment.completed) {
        return Err(download_error(
            FailureCategory::Network,
            "Segmented download ended before every segment completed.".into(),
            true,
        ));
    }

    combine_segment_files(&task.temp_path, &final_state).await?;
    cleanup_segment_artifacts(&task.temp_path, final_state.segments.len()).await;

    let snapshot = state
        .update_job_progress(&task.id, plan.total_bytes, Some(plan.total_bytes), 0, true)
        .await?;
    emit_snapshot(app, &snapshot).await;

    let final_path = move_to_final_path(&task.temp_path, &task.target_path)
        .await
        .map_err(disk_error)?;
    complete_http_download(app, state, task, plan.total_bytes, &final_path).await?;
    Ok(DownloadOutcome::Completed)
}

async fn download_segment_worker(
    context: SegmentWorkerContext,
    segment: SegmentProgress,
) -> Result<DownloadOutcome, DownloadError> {
    let segment_path = segment_path(&context.temp_path, segment.index);
    let mut current_len = metadata_len(&segment_path)
        .await
        .unwrap_or(0)
        .min(segment.range.len());
    let mut low_speed_monitor = LowSpeedMonitor::new(context.profile);

    while current_len < segment.range.len() {
        match context.state.worker_control(&context.job_id).await {
            WorkerControl::Paused => return Ok(DownloadOutcome::Paused),
            WorkerControl::Canceled | WorkerControl::Missing => {
                return Ok(DownloadOutcome::Canceled)
            }
            WorkerControl::Continue => {}
        }

        let requested = ByteRange {
            start: segment.range.start + current_len,
            end: segment.range.end,
        };
        let response = match send_range_request(
            &context.client,
            &context.url,
            requested,
            context.handoff_auth.as_ref(),
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                if error.category == FailureCategory::Resume {
                    range_backoffs().record_rejection(&context.url, Instant::now());
                }
                return Err(error);
            }
        };

        if response.status() != StatusCode::PARTIAL_CONTENT {
            range_backoffs().record_rejection(&context.url, Instant::now());
            return Err(download_error(
                FailureCategory::Resume,
                "The server did not honor a segmented range request.".into(),
                false,
            ));
        }

        let range_ok = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .map(|value| content_range_matches(value, requested, context.total_bytes))
            .unwrap_or(false);

        if !range_ok {
            range_backoffs().record_rejection(&context.url, Instant::now());
            return Err(download_error(
                FailureCategory::Resume,
                "The server returned an unexpected Content-Range for a segment.".into(),
                false,
            ));
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&segment_path)
            .await
            .map_err(|error| disk_error(format!("Could not open segment file: {error}")))?;
        let mut writer = BufWriter::with_capacity(DOWNLOAD_BUFFER_SIZE, file);
        let mut stream = response.bytes_stream();
        let mut low_speed_bytes = 0_u64;
        let mut low_speed_started = Instant::now();

        while let Some(chunk_result) = stream.next().await {
            match context.state.worker_control(&context.job_id).await {
                WorkerControl::Paused => {
                    writer.flush().await.ok();
                    return Ok(DownloadOutcome::Paused);
                }
                WorkerControl::Canceled | WorkerControl::Missing => {
                    writer.flush().await.ok();
                    return Ok(DownloadOutcome::Canceled);
                }
                WorkerControl::Continue => {}
            }

            let chunk = chunk_result.map_err(download_stream_error)?;
            let chunk_len = chunk.len() as u64;
            writer
                .write_all(&chunk)
                .await
                .map_err(|error| disk_error(format!("Could not write segment chunk: {error}")))?;

            current_len = current_len
                .saturating_add(chunk_len)
                .min(segment.range.len());
            low_speed_bytes = low_speed_bytes.saturating_add(chunk_len);
            context
                .progress
                .store_segment_bytes(segment.index, current_len);
            context.progress.add_sample_bytes(chunk_len);

            if low_speed_started.elapsed() >= context.profile.low_speed_window {
                if low_speed_monitor.observe(low_speed_bytes, low_speed_started.elapsed(), false)
                    == LowSpeedDecision::Retry
                {
                    writer.flush().await.ok();
                    tokio::time::sleep(retry_delay_for_attempt(
                        low_speed_monitor.retries.saturating_sub(1) as usize,
                    ))
                    .await;
                    break;
                }
                low_speed_bytes = 0;
                low_speed_started = Instant::now();
            }
        }

        writer
            .flush()
            .await
            .map_err(|error| disk_error(format!("Could not flush segment file: {error}")))?;

        if current_len >= segment.range.len() {
            mark_segment_completed(&context.temp_path, &context.metadata, segment.index).await?;
            return Ok(DownloadOutcome::Completed);
        }

        if low_speed_monitor.retries >= context.profile.max_low_speed_retries {
            return Err(download_error(
                FailureCategory::Network,
                "A segment stayed below the recovery speed threshold.".into(),
                true,
            ));
        }
    }

    mark_segment_completed(&context.temp_path, &context.metadata, segment.index).await?;
    Ok(DownloadOutcome::Completed)
}

async fn report_segmented_progress(
    app: AppHandle,
    state: SharedState,
    job_id: String,
    total_bytes: u64,
    profile: DownloadPerformanceProfile,
    progress: Arc<SegmentedProgressCounters>,
    stop: Arc<AtomicBool>,
) -> Result<(), DownloadError> {
    let mut rolling_speed = RollingSpeed::with_alpha(profile.speed_smoothing_alpha);
    let mut sample_started = Instant::now();
    let mut last_persisted_at = Instant::now();
    let mut interval = tokio::time::interval(PROGRESS_UPDATE_INTERVAL);

    loop {
        interval.tick().await;

        let stopping = stop.load(Ordering::Relaxed);
        let sample_bytes = progress.drain_sample_bytes();
        if sample_bytes == 0 && !stopping {
            continue;
        }

        let elapsed = sample_started.elapsed();
        let speed = if elapsed.as_secs_f64() > 0.0 {
            rolling_speed.record_sample(sample_bytes, elapsed)
        } else {
            0
        };
        sample_started = Instant::now();

        let downloaded_bytes = progress.total_downloaded();
        let should_persist = stopping || last_persisted_at.elapsed() >= PROGRESS_PERSIST_INTERVAL;
        if should_persist {
            last_persisted_at = Instant::now();
        }

        let snapshot = match state.worker_control(&job_id).await {
            WorkerControl::Continue => {
                state
                    .update_job_progress(
                        &job_id,
                        downloaded_bytes,
                        Some(total_bytes),
                        speed,
                        should_persist,
                    )
                    .await?
            }
            WorkerControl::Paused | WorkerControl::Canceled | WorkerControl::Missing => {
                state
                    .sync_downloaded_bytes(&job_id, downloaded_bytes)
                    .await?
            }
        };
        emit_snapshot(&app, &snapshot).await;

        if stopping {
            break;
        }
    }

    Ok(())
}

async fn load_or_create_segment_state(
    temp_path: &Path,
    plan: &RangePlan,
) -> Result<SegmentedDownloadState, DownloadError> {
    let meta_path = segment_meta_path(temp_path);
    if let Ok(raw) = fs::read_to_string(&meta_path).await {
        if let Ok(state) = serde_json::from_str::<SegmentedDownloadState>(&raw) {
            let same_plan = state.total_bytes == plan.total_bytes
                && state.segments.len() == plan.segments.len()
                && state
                    .segments
                    .iter()
                    .zip(plan.segments.iter())
                    .all(|(stored, planned)| stored.range == *planned);
            if same_plan {
                return Ok(state);
            }
        }
    }

    cleanup_segment_artifacts(temp_path, plan.segments.len()).await;
    Ok(SegmentedDownloadState {
        total_bytes: plan.total_bytes,
        segments: plan
            .segments
            .iter()
            .copied()
            .enumerate()
            .map(|(index, range)| SegmentProgress {
                index,
                range,
                completed: false,
            })
            .collect(),
    })
}

async fn refresh_segment_completion_from_disk(
    temp_path: &Path,
    state: &mut SegmentedDownloadState,
) {
    for segment in &mut state.segments {
        let len = metadata_len(&segment_path(temp_path, segment.index))
            .await
            .unwrap_or(0);
        segment.completed = len == segment.range.len();
        if len > segment.range.len() {
            let _ = fs::remove_file(segment_path(temp_path, segment.index)).await;
            segment.completed = false;
        }
    }
}

fn segment_existing_len(temp_path: &Path, segment: &SegmentProgress) -> u64 {
    let path = segment_path(temp_path, segment.index);
    std::fs::metadata(path)
        .map(|metadata| metadata.len().min(segment.range.len()))
        .unwrap_or(0)
}

async fn mark_segment_completed(
    temp_path: &Path,
    metadata: &Arc<Mutex<SegmentedDownloadState>>,
    segment_index: usize,
) -> Result<(), DownloadError> {
    let state = {
        let mut metadata = metadata.lock().await;
        if let Some(segment) = metadata
            .segments
            .iter_mut()
            .find(|segment| segment.index == segment_index)
        {
            segment.completed = true;
        }
        metadata.clone()
    };

    persist_segment_state(temp_path, &state).await
}

async fn persist_segment_state(
    temp_path: &Path,
    state: &SegmentedDownloadState,
) -> Result<(), DownloadError> {
    let serialized = serde_json::to_string_pretty(state)
        .map_err(|error| format!("Could not serialize segment metadata: {error}"))?;
    fs::write(segment_meta_path(temp_path), serialized)
        .await
        .map_err(|error| disk_error(format!("Could not write segment metadata: {error}")))
}

async fn combine_segment_files(
    temp_path: &Path,
    state: &SegmentedDownloadState,
) -> Result<(), DownloadError> {
    let output = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(temp_path)
        .await
        .map_err(|error| disk_error(format!("Could not create combined partial file: {error}")))?;
    let mut writer = BufWriter::with_capacity(SEGMENT_COMBINE_BUFFER_SIZE, output);

    for segment in &state.segments {
        let path = segment_path(temp_path, segment.index);
        let actual_len = metadata_len(&path).await.unwrap_or(0);
        if actual_len != segment.range.len() {
            return Err(download_error(
                FailureCategory::Disk,
                format!(
                    "Segment {} has {} bytes, expected {} bytes.",
                    segment.index,
                    actual_len,
                    segment.range.len()
                ),
                true,
            ));
        }

        let input = fs::File::open(&path)
            .await
            .map_err(|error| disk_error(format!("Could not read segment file: {error}")))?;
        let mut input = BufReader::with_capacity(SEGMENT_COMBINE_BUFFER_SIZE, input);
        tokio::io::copy(&mut input, &mut writer)
            .await
            .map_err(|error| disk_error(format!("Could not combine segment file: {error}")))?;
    }

    writer
        .flush()
        .await
        .map_err(|error| disk_error(format!("Could not flush combined file: {error}")))?;
    writer
        .get_mut()
        .sync_all()
        .await
        .map_err(|error| disk_error(format!("Could not sync combined file: {error}")))?;
    Ok(())
}

async fn cleanup_segment_artifacts(temp_path: &Path, segment_count: usize) {
    let _ = fs::remove_file(segment_meta_path(temp_path)).await;
    for index in 0..segment_count {
        let _ = fs::remove_file(segment_path(temp_path, index)).await;
    }
}

pub async fn cleanup_partial_artifacts(temp_path: &Path) {
    let _ = fs::remove_file(temp_path).await;
    let _ = fs::remove_file(segment_meta_path(temp_path)).await;

    let Some(parent) = temp_path.parent() else {
        return;
    };
    let Some(file_name) = temp_path.file_name().and_then(|value| value.to_str()) else {
        return;
    };
    let segment_prefix = format!("{file_name}.seg");

    let Ok(mut entries) = fs::read_dir(parent).await else {
        return;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let should_remove = entry
            .file_name()
            .to_str()
            .map(|name| name.starts_with(&segment_prefix))
            .unwrap_or(false);
        if should_remove {
            let _ = fs::remove_file(entry.path()).await;
        }
    }
}

fn segment_meta_path(temp_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.meta", temp_path.display()))
}

fn segment_path(temp_path: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.seg{index}", temp_path.display()))
}

async fn send_request(
    client: &Client,
    url: &str,
    existing_bytes: u64,
    handoff_auth: Option<&HandoffAuth>,
) -> Result<reqwest::Response, DownloadError> {
    let range_header = if existing_bytes > 0 {
        Some(format!("bytes={existing_bytes}-"))
    } else {
        None
    };

    send_download_request(client, url, range_header, handoff_auth).await
}

async fn send_range_request(
    client: &Client,
    url: &str,
    range: ByteRange,
    handoff_auth: Option<&HandoffAuth>,
) -> Result<reqwest::Response, DownloadError> {
    send_download_request(
        client,
        url,
        Some(format!("bytes={}-{}", range.start, range.end)),
        handoff_auth,
    )
    .await
}

async fn send_download_request(
    client: &Client,
    url: &str,
    range_header: Option<String>,
    handoff_auth: Option<&HandoffAuth>,
) -> Result<reqwest::Response, DownloadError> {
    let mut next_retry = 0;
    let mut current_url = url.to_string();
    let mut redirects = 0;

    loop {
        let mut request = client.get(&current_url).header(ACCEPT_ENCODING, "identity");
        if let Some(range_header) = range_header.as_deref() {
            request = request.header(RANGE, range_header);
        }
        request = apply_handoff_auth_headers(request, handoff_auth)?;

        match request.send().await {
            Ok(response) => {
                if response.status().is_redirection() {
                    let next_url = redirect_location(response.url().as_str(), &response)?;
                    if handoff_auth.is_some()
                        && !redirect_keeps_origin(response.url().as_str(), &next_url)
                    {
                        return Err(download_error(
                            FailureCategory::Http,
                            "Authenticated download redirected to another origin; refusing to forward browser credentials."
                                .into(),
                            false,
                        ));
                    }

                    redirects += 1;
                    if redirects > 10 {
                        return Err(download_error(
                            FailureCategory::Http,
                            "Download redirected too many times.".into(),
                            false,
                        ));
                    }

                    current_url = next_url;
                    next_retry = 0;
                    continue;
                }

                if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
                    return Err(download_error(
                        FailureCategory::Resume,
                        "The remote server rejected the resume request.".into(),
                        false,
                    ));
                }

                if response.status().is_success() {
                    return Ok(response);
                }

                let status = response.status();

                if should_retry_status(status) && next_retry < REQUEST_RETRY_DELAYS.len() {
                    tokio::time::sleep(REQUEST_RETRY_DELAYS[next_retry]).await;
                    next_retry += 1;
                    continue;
                }

                return Err(error_for_http_status(status, handoff_auth.is_some()));
            }
            Err(error) => {
                if should_retry_error(&error) && next_retry < REQUEST_RETRY_DELAYS.len() {
                    tokio::time::sleep(REQUEST_RETRY_DELAYS[next_retry]).await;
                    next_retry += 1;
                    continue;
                }

                return Err(request_error(error));
            }
        }
    }
}

fn apply_handoff_auth_headers(
    mut request: reqwest::RequestBuilder,
    handoff_auth: Option<&HandoffAuth>,
) -> Result<reqwest::RequestBuilder, DownloadError> {
    let Some(auth) = handoff_auth else {
        return Ok(request);
    };

    for header in &auth.headers {
        let name = HeaderName::from_bytes(header.name.as_bytes()).map_err(|_| {
            download_error(
                FailureCategory::Internal,
                "Authenticated handoff header name is invalid.".into(),
                false,
            )
        })?;
        let value = HeaderValue::from_str(&header.value).map_err(|_| {
            download_error(
                FailureCategory::Internal,
                "Authenticated handoff header value is invalid.".into(),
                false,
            )
        })?;
        request = request.header(name, value);
    }

    Ok(request)
}

fn redirect_location(
    current_url: &str,
    response: &reqwest::Response,
) -> Result<String, DownloadError> {
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            download_error(
                FailureCategory::Http,
                "Download redirected without a Location header.".into(),
                false,
            )
        })?;
    let base = reqwest::Url::parse(current_url).map_err(|_| {
        download_error(
            FailureCategory::Http,
            "Download redirected from an invalid URL.".into(),
            false,
        )
    })?;
    let next_url = base.join(location).map_err(|_| {
        download_error(
            FailureCategory::Http,
            "Download redirected to an invalid URL.".into(),
            false,
        )
    })?;

    match next_url.scheme() {
        "http" | "https" => Ok(next_url.to_string()),
        _ => Err(download_error(
            FailureCategory::Http,
            "Download redirected to an unsupported URL scheme.".into(),
            false,
        )),
    }
}

fn redirect_keeps_origin(current_url: &str, next_url: &str) -> bool {
    let Ok(current) = reqwest::Url::parse(current_url) else {
        return false;
    };
    let Ok(next) = reqwest::Url::parse(next_url) else {
        return false;
    };

    current.scheme() == next.scheme()
        && current.host_str().map(str::to_ascii_lowercase)
            == next.host_str().map(str::to_ascii_lowercase)
        && current.port_or_known_default() == next.port_or_known_default()
}

async fn preflight_download(
    client: &Client,
    url: &str,
    handoff_auth: Option<&HandoffAuth>,
) -> Option<PreflightMetadata> {
    let mut current_url = url.to_string();
    let mut redirects = 0;
    let response = loop {
        let request = client
            .head(&current_url)
            .timeout(PREFLIGHT_TIMEOUT)
            .header(ACCEPT_ENCODING, "identity");
        let request = apply_handoff_auth_headers(request, handoff_auth).ok()?;
        let response = request.send().await.ok()?;
        if !response.status().is_redirection() {
            break response;
        }

        let next_url = redirect_location(response.url().as_str(), &response).ok()?;
        if handoff_auth.is_some() && !redirect_keeps_origin(response.url().as_str(), &next_url) {
            return None;
        }
        redirects += 1;
        if redirects > 10 {
            return None;
        }
        current_url = next_url;
    };
    if !response.status().is_success() {
        return None;
    }

    let accept_ranges = response
        .headers()
        .get(ACCEPT_RANGES)
        .and_then(|value| value.to_str().ok());
    let content_disposition = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok());

    Some(derive_preflight_metadata_from_parts(
        response.content_length(),
        accept_ranges,
        content_disposition,
        response.url().as_str(),
    ))
}

fn derive_preflight_metadata_from_parts(
    total_bytes: Option<u64>,
    accept_ranges: Option<&str>,
    content_disposition: Option<&str>,
    final_url: &str,
) -> PreflightMetadata {
    PreflightMetadata {
        total_bytes,
        resume_support: derive_resume_support_from_parts(StatusCode::OK, 0, accept_ranges),
        filename: content_disposition
            .and_then(parse_content_disposition_filename)
            .or_else(|| derive_filename_from_url(final_url)),
    }
}

fn derive_total_bytes(response: &reqwest::Response, existing_bytes: u64) -> Option<u64> {
    if response.status() == StatusCode::PARTIAL_CONTENT {
        if let Some(total_bytes) = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_range_total)
        {
            return Some(total_bytes);
        }

        return response
            .content_length()
            .map(|length| existing_bytes.saturating_add(length));
    }

    response.content_length()
}

fn derive_resume_support(response: &reqwest::Response, existing_bytes: u64) -> ResumeSupport {
    let accept_ranges = response
        .headers()
        .get(ACCEPT_RANGES)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    derive_resume_support_from_parts(response.status(), existing_bytes, accept_ranges.as_deref())
}

fn derive_resume_support_from_parts(
    status: StatusCode,
    existing_bytes: u64,
    accept_ranges: Option<&str>,
) -> ResumeSupport {
    if status == StatusCode::PARTIAL_CONTENT {
        return ResumeSupport::Supported;
    }

    if existing_bytes > 0 {
        return ResumeSupport::Unsupported;
    }

    accept_ranges
        .map(|value| {
            if value.to_ascii_lowercase().contains("bytes") {
                ResumeSupport::Supported
            } else {
                ResumeSupport::Unsupported
            }
        })
        .unwrap_or(ResumeSupport::Unknown)
}

async fn ensure_parent_directory(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Download path has no parent directory.".to_string())?;

    fs::create_dir_all(parent)
        .await
        .map_err(|error| format!("Could not create download directory: {error}"))
}

async fn truncate_file(path: &Path) -> Result<(), String> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .await
        .map_err(|error| format!("Could not reset partial download file: {error}"))?;

    file.set_len(0)
        .await
        .map_err(|error| format!("Could not truncate partial download file: {error}"))
}

async fn metadata_len(path: &Path) -> Option<u64> {
    fs::metadata(path).await.ok().map(|metadata| metadata.len())
}

pub async fn compute_sha256(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .await
        .map_err(|error| format!("Could not open file for SHA-256 verification: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| format!("Could not read file for SHA-256 verification: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

async fn move_to_final_path(
    temp_path: &Path,
    target_path: &Path,
) -> Result<std::path::PathBuf, String> {
    let final_path = allocate_final_path(target_path).await?;

    fs::rename(temp_path, &final_path)
        .await
        .map_err(|error| format!("Could not finalize downloaded file: {error}"))?;

    Ok(final_path)
}

async fn allocate_final_path(target_path: &Path) -> Result<std::path::PathBuf, String> {
    if !target_path.exists() {
        return Ok(target_path.to_path_buf());
    }

    let stem = target_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = target_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    let parent = target_path
        .parent()
        .ok_or_else(|| "Download path has no parent directory.".to_string())?;

    for index in 1..10_000 {
        let candidate = parent.join(format!("{stem} ({index}){extension}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err("Could not allocate a unique final download path.".into())
}

fn extract_filename(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_disposition_filename)
}

fn parse_content_disposition_filename(header_value: &str) -> Option<String> {
    if let Some(encoded) = header_value
        .split(';')
        .find_map(|segment| segment.trim().strip_prefix("filename*="))
    {
        let sanitized = decode_content_disposition_filename(encoded);
        if !sanitized.is_empty() {
            return Some(sanitized);
        }
    }

    header_value
        .split(';')
        .find_map(|segment| segment.trim().strip_prefix("filename="))
        .map(decode_content_disposition_filename)
        .filter(|value| !value.is_empty())
}

fn decode_content_disposition_filename(value: &str) -> String {
    let value = value.trim().trim_matches('"').trim();
    let encoded = value.split("''").nth(1).unwrap_or(value);
    let decoded = percent_decode_str(encoded).decode_utf8_lossy();
    sanitize_filename(decoded.trim())
}

fn derive_target_path(
    current_target_path: &Path,
    response: &reqwest::Response,
) -> std::path::PathBuf {
    let filename = extract_filename(response)
        .or_else(|| derive_filename_from_url(response.url().as_str()))
        .unwrap_or_else(|| fallback_filename(current_target_path));

    current_target_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(filename)
}

fn fallback_filename(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("download.bin")
        .to_string()
}

fn derive_filename_from_url(raw_url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(raw_url).ok()?;
    let candidate = parsed
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())?;
    let decoded = percent_decode_str(candidate).decode_utf8_lossy();
    let sanitized = sanitize_filename(&decoded);
    if sanitized.is_empty() || sanitized == "download.bin" {
        None
    } else {
        Some(sanitized)
    }
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.parse::<u64>().ok()
}

fn content_range_matches(value: &str, expected_range: ByteRange, expected_total: u64) -> bool {
    let Some((range, total)) = parse_content_range(value) else {
        return false;
    };

    range == expected_range && total == expected_total
}

fn parse_content_range(value: &str) -> Option<(ByteRange, u64)> {
    let value = value.trim();
    let range_and_total = value.strip_prefix("bytes ")?;
    let (range, total) = range_and_total.split_once('/')?;
    let (start, end) = range.split_once('-')?;

    Some((
        ByteRange {
            start: start.parse().ok()?,
            end: end.parse().ok()?,
        },
        total.parse().ok()?,
    ))
}

fn sanitize_filename(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            character if character.is_control() => '_',
            _ => character,
        })
        .collect();

    let mut sanitized = sanitized.trim().trim_matches('.').trim().to_string();
    if is_windows_reserved_filename(&sanitized) {
        sanitized.push('_');
    }
    sanitized
}

fn is_windows_reserved_filename(filename: &str) -> bool {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename)
        .to_ascii_uppercase();

    matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn should_retry_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    ) || status.is_server_error()
}

fn should_retry_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request() || error.is_body()
}

fn download_error(category: FailureCategory, message: String, retryable: bool) -> DownloadError {
    DownloadError {
        category,
        message,
        retryable,
    }
}

fn error_for_http_status(status: StatusCode, authenticated_handoff: bool) -> DownloadError {
    let retryable = should_retry_status(status);
    let category = if retryable {
        FailureCategory::Server
    } else {
        FailureCategory::Http
    };
    let auth_context = if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        if authenticated_handoff {
            " Authenticated handoff was rejected; the browser session may have expired."
        } else {
            " This server may require browser session authentication."
        }
    } else {
        ""
    };

    download_error(
        category,
        format!("Download request failed with HTTP {status}.{auth_context}"),
        retryable,
    )
}

fn request_error(error: reqwest::Error) -> DownloadError {
    let retryable = should_retry_error(&error);
    let category = if retryable {
        FailureCategory::Network
    } else {
        FailureCategory::Internal
    };

    download_error(
        category,
        format!("Could not start download: {error}"),
        retryable,
    )
}

fn download_stream_error(error: reqwest::Error) -> DownloadError {
    let retryable = should_retry_error(&error);
    let category = if retryable {
        FailureCategory::Network
    } else {
        FailureCategory::Internal
    };

    download_error(category, format!("Download failed: {error}"), retryable)
}

fn disk_error(message: String) -> DownloadError {
    download_error(FailureCategory::Disk, message, false)
}

fn retry_delay_for_attempt(attempt: usize) -> Duration {
    REQUEST_RETRY_DELAYS
        .get(attempt)
        .copied()
        .unwrap_or_else(|| *REQUEST_RETRY_DELAYS.last().unwrap())
}

fn throttle_delay_for_limit(
    bytes_per_second: u64,
    transferred_bytes: u64,
    elapsed: Duration,
) -> Option<Duration> {
    if bytes_per_second == 0 || transferred_bytes == 0 {
        return None;
    }

    let expected_elapsed =
        Duration::from_secs_f64(transferred_bytes as f64 / bytes_per_second as f64);
    let delay = expected_elapsed.checked_sub(elapsed)?;
    if delay.is_zero() {
        None
    } else {
        Some(delay)
    }
}

async fn throttle_download(
    state: &SharedState,
    job_id: &str,
    bytes_per_second: u64,
    transferred_bytes: u64,
    started: Instant,
) -> WorkerControl {
    while let Some(delay) =
        throttle_delay_for_limit(bytes_per_second, transferred_bytes, started.elapsed())
    {
        tokio::time::sleep(std::cmp::min(delay, THROTTLE_CONTROL_INTERVAL)).await;
        let control = state.worker_control(job_id).await;
        if control != WorkerControl::Continue {
            return control;
        }
    }

    WorkerControl::Continue
}

async fn notify_download_completed(app: &AppHandle, state: &SharedState, final_path: &Path) {
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

async fn notify_bulk_archive_completed(app: &AppHandle, state: &SharedState, final_path: &Path) {
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

async fn create_bulk_archive(archive: BulkArchiveReady) -> Result<PathBuf, String> {
    tokio::task::spawn_blocking(move || create_bulk_archive_sync(archive))
        .await
        .map_err(|error| format!("Could not create bulk archive task: {error}"))?
}

fn create_bulk_archive_sync(archive: BulkArchiveReady) -> Result<PathBuf, String> {
    if archive.output_path.exists() {
        return Ok(archive.output_path);
    }

    let staging_dir = archive
        .output_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(
            ".sdm-bulk-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));

    std::fs::create_dir_all(&staging_dir)
        .map_err(|error| format!("Could not create archive staging directory: {error}"))?;

    let mut staged_paths = Vec::with_capacity(archive.entries.len());
    for entry in &archive.entries {
        let staged_path = staging_dir.join(&entry.archive_name);
        std::fs::copy(&entry.source_path, &staged_path).map_err(|error| {
            format!(
                "Could not stage {} for archiving: {error}",
                entry.source_path.display()
            )
        })?;
        staged_paths.push(staged_path);
    }

    let script = r#"
$ErrorActionPreference = 'Stop'
$destination = $args[0]
$paths = @()
for ($index = 1; $index -lt $args.Length; $index++) { $paths += $args[$index] }
Compress-Archive -LiteralPath $paths -DestinationPath $destination -Force
"#;

    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .arg(&archive.output_path)
        .args(&staged_paths)
        .status()
        .map_err(|error| format!("Could not run Compress-Archive: {error}"))?;

    let _ = std::fs::remove_dir_all(&staging_dir);

    if !status.success() {
        return Err(format!("Compress-Archive failed with status {status}."));
    }

    Ok(archive.output_path)
}

async fn notify_download_failure(
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

async fn notify(app: &AppHandle, state: &SharedState, title: &str, body: &str) {
    if !state.notifications_enabled().await {
        return;
    }

    let _ = app.shell.notify(title.to_string(), body.to_string()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::ShellServices;
    use crate::storage::{
        DownloadJob, HandoffAuthHeader, IntegrityAlgorithm, IntegrityCheck, IntegrityStatus,
        JobState, TransferKind,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[derive(Clone)]
    struct TestShell;

    impl ShellServices for TestShell {}

    #[test]
    fn http_status_errors_are_classified_by_recoverability() {
        let unavailable = error_for_http_status(StatusCode::SERVICE_UNAVAILABLE, false);
        assert_eq!(unavailable.category, FailureCategory::Server);
        assert!(unavailable.retryable);

        let not_found = error_for_http_status(StatusCode::NOT_FOUND, false);
        assert_eq!(not_found.category, FailureCategory::Http);
        assert!(!not_found.retryable);
    }

    #[test]
    fn probed_range_metadata_wins_when_head_size_disagrees() {
        let merged = merge_preflight_metadata(
            Some(PreflightMetadata {
                total_bytes: Some(64),
                resume_support: ResumeSupport::Supported,
                filename: Some("head.bin".into()),
            }),
            PreflightMetadata {
                total_bytes: Some(128),
                resume_support: ResumeSupport::Supported,
                filename: Some("probe.bin".into()),
            },
        );

        assert_eq!(merged.total_bytes, Some(128));
        assert_eq!(merged.filename.as_deref(), Some("head.bin"));
    }

    #[test]
    fn low_speed_recovery_retries_only_after_sustained_unlimited_slowdown() {
        let profile = performance_profile(DownloadPerformanceMode::Balanced);
        let mut monitor = LowSpeedMonitor::new(profile);

        assert_eq!(
            monitor.observe(4 * 1024, Duration::from_secs(10), false),
            LowSpeedDecision::Continue
        );
        assert_eq!(
            monitor.observe(4 * 1024, Duration::from_secs(21), false),
            LowSpeedDecision::Retry
        );
        assert_eq!(
            monitor.observe(0, Duration::from_secs(30), true),
            LowSpeedDecision::Continue
        );
    }

    #[tokio::test]
    async fn range_probe_metadata_uses_partial_content_total_and_identity_header() {
        let response = concat!(
            "HTTP/1.1 206 Partial Content\r\n",
            "Content-Range: bytes 0-0/33554432\r\n",
            "Content-Length: 1\r\n",
            "Content-Disposition: attachment; filename=\"probe.bin\"\r\n",
            "\r\n",
            "x"
        );
        let (url, request_handle) = spawn_one_response_server(response).await;
        let client = download_client().unwrap();

        let metadata = probe_range_metadata(&client, &url, None)
            .await
            .expect("range probe should derive metadata from partial content");
        let request = request_handle.await.unwrap();
        let request_lower = request.to_ascii_lowercase();

        assert!(request_lower.contains("range: bytes=0-0"));
        assert!(request_lower.contains("accept-encoding: identity"));
        assert_eq!(metadata.total_bytes, Some(33_554_432));
        assert_eq!(metadata.resume_support, ResumeSupport::Supported);
        assert_eq!(metadata.filename.as_deref(), Some("probe.bin"));
    }

    #[tokio::test]
    async fn send_request_applies_authenticated_handoff_headers() {
        let (url, request_handle) = spawn_cookie_required_server().await;
        let client = download_client().unwrap();
        let auth = HandoffAuth {
            headers: vec![HandoffAuthHeader {
                name: "Cookie".into(),
                value: "session=abc".into(),
            }],
        };

        let response = send_request(&client, &url, 0, Some(&auth)).await.unwrap();
        let request = request_handle.await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(request.to_ascii_lowercase().contains("cookie: session=abc"));
        assert!(request
            .to_ascii_lowercase()
            .contains("accept-encoding: identity"));
    }

    #[tokio::test]
    async fn protected_handoff_access_probe_rejects_missing_browser_auth() {
        let (url, request_handle) = spawn_cookie_required_server().await;

        let error = probe_browser_handoff_access(&url, None)
            .await
            .expect_err("missing browser auth should reject protected downloads before queuing");
        let request = request_handle.await.unwrap();

        assert_eq!(error.code, PROTECTED_DOWNLOAD_AUTH_REQUIRED_CODE);
        assert_eq!(error.status, Some(403));
        assert!(request.to_ascii_lowercase().contains("range: bytes=0-0"));
        assert!(request
            .to_ascii_lowercase()
            .contains("accept-encoding: identity"));
    }

    #[tokio::test]
    async fn segmented_sidecar_resumes_only_valid_completed_ranges() {
        let root = test_runtime_dir("sdm-core-segment-test");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let temp_path = root.join("download.bin.part");
        let plan = RangePlan {
            total_bytes: 12,
            segments: vec![
                ByteRange { start: 0, end: 3 },
                ByteRange { start: 4, end: 7 },
                ByteRange { start: 8, end: 11 },
            ],
        };

        let mut state = load_or_create_segment_state(&temp_path, &plan)
            .await
            .unwrap();
        tokio::fs::write(segment_path(&temp_path, 0), vec![1_u8; 4])
            .await
            .unwrap();
        tokio::fs::write(segment_path(&temp_path, 1), vec![2_u8; 2])
            .await
            .unwrap();
        tokio::fs::write(segment_path(&temp_path, 2), vec![3_u8; 5])
            .await
            .unwrap();

        refresh_segment_completion_from_disk(&temp_path, &mut state).await;

        assert!(state.segments[0].completed);
        assert!(!state.segments[1].completed);
        assert!(!state.segments[2].completed);
        assert_eq!(segment_existing_len(&temp_path, &state.segments[1]), 2);
        assert_eq!(metadata_len(&segment_path(&temp_path, 2)).await, None);

        persist_segment_state(&temp_path, &state).await.unwrap();
        let reloaded = load_or_create_segment_state(&temp_path, &plan)
            .await
            .unwrap();
        assert!(reloaded.segments[0].completed);
        assert!(!reloaded.segments[1].completed);

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn http_completion_verifies_sha256_and_marks_integrity_failure() {
        let root = test_runtime_dir("sdm-core-sha-complete-test");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let storage_path = root.join("state.json");
        let final_path = root.join("download.bin");
        tokio::fs::write(&final_path, b"wrong").await.unwrap();
        let temp_path = root.join("download.bin.part");
        let job = DownloadJob {
            id: "job_1".into(),
            url: "https://example.com/download.bin".into(),
            filename: "download.bin".into(),
            source: None,
            transfer_kind: TransferKind::Http,
            integrity_check: Some(IntegrityCheck {
                algorithm: IntegrityAlgorithm::Sha256,
                expected: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into(),
                actual: None,
                status: IntegrityStatus::Pending,
            }),
            torrent: None,
            state: JobState::Downloading,
            created_at: 0,
            progress: 0.0,
            total_bytes: 0,
            downloaded_bytes: 0,
            speed: 0,
            eta: 0,
            error: None,
            failure_category: None,
            resume_support: ResumeSupport::Unknown,
            retry_attempts: 0,
            target_path: final_path.display().to_string(),
            temp_path: temp_path.display().to_string(),
            artifact_exists: None,
            bulk_archive: None,
        };
        let state = SharedState::for_tests(storage_path, vec![job]);
        let shell = TransferShell::new(TestShell);
        let task = DownloadTask {
            id: "job_1".into(),
            url: "https://example.com/download.bin".into(),
            filename: "download.bin".into(),
            transfer_kind: TransferKind::Http,
            torrent: None,
            handoff_auth: None,
            target_path: final_path.clone(),
            temp_path,
        };

        complete_http_download(&shell, &state, &task, 5, &final_path)
            .await
            .unwrap();

        let snapshot = state.snapshot().await;
        let completed = snapshot
            .jobs
            .iter()
            .find(|job| job.id == "job_1")
            .expect("job should remain in snapshot");
        assert_eq!(completed.state, JobState::Failed);
        assert_eq!(completed.failure_category, Some(FailureCategory::Integrity));

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    async fn spawn_one_response_server(
        response: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0_u8; 4096];
            let read = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            socket.write_all(response.as_bytes()).await.unwrap();
            request
        });

        (format!("http://{address}/download.bin"), handle)
    }

    async fn spawn_cookie_required_server() -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0_u8; 4096];
            let read = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let response = if request.to_ascii_lowercase().contains("cookie: session=abc") {
                "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
            } else {
                "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n"
            };
            socket.write_all(response.as_bytes()).await.unwrap();
            request
        });

        (format!("http://{address}/download.bin"), handle)
    }

    fn test_runtime_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
