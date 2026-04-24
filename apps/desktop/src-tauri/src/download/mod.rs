use crate::commands::emit_snapshot;
use crate::state::{SharedState, WorkerControl};
use crate::storage::{FailureCategory, ResumeSupport};
use futures_util::StreamExt;
use percent_encoding::percent_decode_str;
use reqwest::header::{ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_RANGE, RANGE};
use reqwest::{Client, StatusCode};
use std::path::Path;
use std::time::{Duration, Instant};
use tauri::plugin::PermissionState;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(120);
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(8);
const PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_millis(750);
const PROGRESS_PERSIST_INTERVAL: Duration = Duration::from_secs(5);
const THROTTLE_CONTROL_INTERVAL: Duration = Duration::from_millis(250);
const REQUEST_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
];

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
                    let _ = fs::remove_file(&task.temp_path).await;
                }
            }
            Err(error) => {
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

        schedule_downloads(app, state);
    });
}

enum DownloadOutcome {
    Completed,
    Paused,
    Canceled,
}

#[derive(Debug, Clone)]
struct DownloadError {
    category: FailureCategory,
    message: String,
    retryable: bool,
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

async fn run_download(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
) -> Result<DownloadOutcome, DownloadError> {
    let max_retry_attempts = state.auto_retry_attempts().await;
    let mut retry_attempts = 0;

    loop {
        match run_download_attempt(app, state, task).await {
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

async fn run_download_attempt(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
) -> Result<DownloadOutcome, DownloadError> {
    ensure_parent_directory(&task.target_path)
        .await
        .map_err(disk_error)?;

    let mut existing_bytes = metadata_len(&task.temp_path).await.unwrap_or(0);
    if existing_bytes > 0 {
        let snapshot = state
            .sync_downloaded_bytes(&task.id, existing_bytes)
            .await?;
        emit_snapshot(app, &snapshot);
    }

    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .user_agent("SimpleDownloadManager/0.1")
        .build()
        .map_err(|error| format!("Could not create download client: {error}"))?;

    if let Some(metadata) = preflight_download(&client, &task.url).await {
        let snapshot = state
            .apply_preflight_metadata(
                &task.id,
                metadata.total_bytes,
                metadata.resume_support,
                metadata.filename,
            )
            .await?;
        emit_snapshot(app, &snapshot);
    }

    let mut response = send_request(&client, &task.url, existing_bytes).await?;
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
        emit_snapshot(app, &snapshot);
        response = send_request(&client, &task.url, 0).await?;
    }

    let total_bytes = derive_total_bytes(&response, existing_bytes);
    let resume_support = derive_resume_support(&response, existing_bytes);
    let display_filename = extract_filename(&response)
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
    emit_snapshot(app, &snapshot);

    let mut file = if existing_bytes > 0 {
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

    let mut stream = response.bytes_stream();
    let mut downloaded_bytes = existing_bytes;
    let speed_limit = state.speed_limit_bytes_per_second().await;
    let attempt_started = Instant::now();
    let mut attempt_transferred_bytes = 0_u64;
    let mut sample_bytes = 0_u64;
    let mut sample_started = Instant::now();
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
        let speed = if elapsed.as_secs_f64() > 0.0 {
            (sample_bytes as f64 / elapsed.as_secs_f64()) as u64
        } else {
            0
        };

        if elapsed >= PROGRESS_UPDATE_INTERVAL {
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
            emit_snapshot(app, &snapshot);
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
    file.sync_all()
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
        emit_snapshot(app, &snapshot);
    }

    let final_path = move_to_final_path(&task.temp_path, &target_path)
        .await
        .map_err(disk_error)?;
    let snapshot = state
        .complete_job(&task.id, downloaded_bytes, &final_path)
        .await?;
    emit_snapshot(app, &snapshot);
    notify_download_completed(app, state, &final_path).await;
    Ok(DownloadOutcome::Completed)
}

async fn send_request(
    client: &Client,
    url: &str,
    existing_bytes: u64,
) -> Result<reqwest::Response, DownloadError> {
    let mut next_retry = 0;

    loop {
        let mut request = client.get(url);
        if existing_bytes > 0 {
            request = request.header(RANGE, format!("bytes={existing_bytes}-"));
        }

        match request.send().await {
            Ok(response) => {
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

                return Err(error_for_http_status(status));
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

async fn preflight_download(client: &Client, url: &str) -> Option<PreflightMetadata> {
    let response = client
        .head(url)
        .timeout(PREFLIGHT_TIMEOUT)
        .send()
        .await
        .ok()?;
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
        .and_then(|value| value.split("''").nth(1).or(Some(value)))
    {
        let decoded = percent_decode_str(encoded).decode_utf8_lossy();
        let sanitized = sanitize_filename(decoded.trim_matches('"').trim());
        if !sanitized.is_empty() {
            return Some(sanitized);
        }
    }

    header_value
        .split(';')
        .find_map(|segment| segment.trim().strip_prefix("filename="))
        .map(|value| sanitize_filename(value.trim_matches('"').trim()))
        .filter(|value| !value.is_empty())
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
        .and_then(|segments| segments.last())
        .filter(|segment| !segment.is_empty())?;
    let sanitized = sanitize_filename(candidate);
    if sanitized.is_empty() || sanitized == "download.bin" {
        None
    } else {
        Some(sanitized)
    }
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.parse::<u64>().ok()
}

fn sanitize_filename(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => character,
        })
        .collect();

    sanitized.trim().trim_matches('.').to_string()
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

fn error_for_http_status(status: StatusCode) -> DownloadError {
    let retryable = should_retry_status(status);
    let category = if retryable {
        FailureCategory::Server
    } else {
        FailureCategory::Http
    };

    download_error(
        category,
        format!("Download request failed with HTTP {status}."),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_errors_are_classified_by_recoverability() {
        let unavailable = error_for_http_status(StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(unavailable.category, FailureCategory::Server);
        assert!(unavailable.retryable);

        let not_found = error_for_http_status(StatusCode::NOT_FOUND);
        assert_eq!(not_found.category, FailureCategory::Http);
        assert!(!not_found.retryable);
    }

    #[test]
    fn retry_delay_caps_at_last_configured_delay() {
        assert_eq!(retry_delay_for_attempt(0), REQUEST_RETRY_DELAYS[0]);
        assert_eq!(
            retry_delay_for_attempt(99),
            *REQUEST_RETRY_DELAYS.last().unwrap()
        );
    }

    #[test]
    fn resume_support_uses_partial_content_before_header_hints() {
        assert_eq!(
            derive_resume_support_from_parts(StatusCode::PARTIAL_CONTENT, 10, None),
            ResumeSupport::Supported
        );
        assert_eq!(
            derive_resume_support_from_parts(StatusCode::OK, 10, Some("bytes")),
            ResumeSupport::Unsupported
        );
        assert_eq!(
            derive_resume_support_from_parts(StatusCode::OK, 0, Some("bytes")),
            ResumeSupport::Supported
        );
        assert_eq!(
            derive_resume_support_from_parts(StatusCode::OK, 0, None),
            ResumeSupport::Unknown
        );
    }

    #[test]
    fn preflight_metadata_uses_head_headers() {
        let metadata = derive_preflight_metadata_from_parts(
            Some(4_096),
            Some("bytes"),
            Some("attachment; filename=\"server-report.pdf\""),
            "https://example.com/download",
        );

        assert_eq!(metadata.total_bytes, Some(4_096));
        assert_eq!(metadata.resume_support, ResumeSupport::Supported);
        assert_eq!(metadata.filename.as_deref(), Some("server-report.pdf"));
    }

    #[test]
    fn speed_limit_throttle_calculates_remaining_delay() {
        assert_eq!(
            throttle_delay_for_limit(1024, 4096, Duration::from_secs(2)),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            throttle_delay_for_limit(1024, 4096, Duration::from_secs(4)),
            None
        );
        assert_eq!(
            throttle_delay_for_limit(0, 4096, Duration::from_secs(0)),
            None
        );
    }
}
