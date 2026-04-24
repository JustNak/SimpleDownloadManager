use crate::commands::emit_snapshot;
use crate::state::{SharedState, WorkerControl};
use futures_util::StreamExt;
use percent_encoding::percent_decode_str;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_RANGE, RANGE};
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
const PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_millis(750);
const PROGRESS_PERSIST_INTERVAL: Duration = Duration::from_secs(5);
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
        let cleanup_temp_on_exit = matches!(state.worker_control(&task.id).await, WorkerControl::Canceled);

        match run_download(&app, &state, &task).await {
            Ok(DownloadOutcome::Completed) => {}
            Ok(DownloadOutcome::Paused) | Ok(DownloadOutcome::Canceled) => {
                if let Ok(snapshot) = state.finish_interrupted_job(&task.id).await {
                    emit_snapshot(&app, &snapshot);
                }

                if cleanup_temp_on_exit
                    || matches!(state.worker_control(&task.id).await, WorkerControl::Canceled | WorkerControl::Missing)
                {
                    let _ = fs::remove_file(&task.temp_path).await;
                }
            }
            Err(error) => {
                if let Ok(snapshot) = state.fail_job(&task.id, error).await {
                    emit_snapshot(&app, &snapshot);
                    notify_download_failure(&app, &state, &task, snapshot.jobs.iter().find(|job| job.id == task.id).and_then(|job| job.error.as_deref())).await;
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

async fn run_download(app: &AppHandle, state: &SharedState, task: &crate::state::DownloadTask) -> Result<DownloadOutcome, String> {
    ensure_parent_directory(&task.target_path).await?;

    let mut existing_bytes = metadata_len(&task.temp_path).await.unwrap_or(0);
    if existing_bytes > 0 {
        let snapshot = state.sync_downloaded_bytes(&task.id, existing_bytes).await?;
        emit_snapshot(app, &snapshot);
    }

    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .user_agent("SimpleDownloadManager/0.1")
        .build()
        .map_err(|error| format!("Could not create download client: {error}"))?;

    let mut response = send_request(&client, &task.url, existing_bytes).await?;
    let supports_resume = response.status() == StatusCode::PARTIAL_CONTENT;

    if existing_bytes > 0 && !supports_resume {
        truncate_file(&task.temp_path).await?;
        existing_bytes = 0;
        let snapshot = state.mark_job_downloading(&task.id, 0, response.content_length()).await?;
        emit_snapshot(app, &snapshot);
        response = send_request(&client, &task.url, 0).await?;
    }

    let total_bytes = derive_total_bytes(&response, existing_bytes);
    let target_path = derive_target_path(&task.target_path, &response);
    let snapshot = state
        .mark_job_downloading(&task.id, existing_bytes, total_bytes)
        .await?;
    emit_snapshot(app, &snapshot);

    let mut file = if existing_bytes > 0 {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&task.temp_path)
            .await
            .map_err(|error| format!("Could not open partial download file: {error}"))?
    } else {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&task.temp_path)
            .await
            .map_err(|error| format!("Could not create download file: {error}"))?
    };

    let mut stream = response.bytes_stream();
    let mut downloaded_bytes = existing_bytes;
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

        let chunk = chunk_result.map_err(|error| format!("Download failed: {error}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("Could not write download chunk: {error}"))?;

        downloaded_bytes = downloaded_bytes.saturating_add(chunk.len() as u64);
        sample_bytes = sample_bytes.saturating_add(chunk.len() as u64);
        let elapsed = sample_started.elapsed();
        let speed = if elapsed.as_secs_f64() > 0.0 {
            (sample_bytes as f64 / elapsed.as_secs_f64()) as u64
        } else {
            0
        };

        if elapsed >= PROGRESS_UPDATE_INTERVAL {
            let should_persist = last_persisted_at.elapsed() >= PROGRESS_PERSIST_INTERVAL;
            let snapshot = state
                .update_job_progress(&task.id, downloaded_bytes, total_bytes, speed, should_persist)
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
        .map_err(|error| format!("Could not flush download file: {error}"))?;
    file.sync_all()
        .await
        .map_err(|error| format!("Could not sync download file: {error}"))?;

    if let Some(total_bytes) = total_bytes {
        if downloaded_bytes < total_bytes {
            return Err(format!(
                "Download ended early. Received {downloaded_bytes} of {total_bytes} bytes."
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

    let final_path = move_to_final_path(&task.temp_path, &target_path).await?;
    let snapshot = state
        .complete_job(&task.id, downloaded_bytes, &final_path)
        .await?;
    emit_snapshot(app, &snapshot);
    notify_download_completed(app, state, &final_path).await;
    Ok(DownloadOutcome::Completed)
}

async fn send_request(client: &Client, url: &str, existing_bytes: u64) -> Result<reqwest::Response, String> {
    let mut next_retry = 0;

    loop {
        let mut request = client.get(url);
        if existing_bytes > 0 {
            request = request.header(RANGE, format!("bytes={existing_bytes}-"));
        }

        match request.send().await {
            Ok(response) => {
                if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
                    return Err("The remote server rejected the resume request.".into());
                }

                if response.status().is_success() {
                    return Ok(response);
                }

                let status = response.status();
                let message = format!("Download request failed with HTTP {status}.");

                if should_retry_status(status) && next_retry < REQUEST_RETRY_DELAYS.len() {
                    tokio::time::sleep(REQUEST_RETRY_DELAYS[next_retry]).await;
                    next_retry += 1;
                    continue;
                }

                return Err(message);
            }
            Err(error) => {
                if should_retry_error(&error) && next_retry < REQUEST_RETRY_DELAYS.len() {
                    tokio::time::sleep(REQUEST_RETRY_DELAYS[next_retry]).await;
                    next_retry += 1;
                    continue;
                }

                return Err(format!("Could not start download: {error}"));
            }
        }
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

async fn move_to_final_path(temp_path: &Path, target_path: &Path) -> Result<std::path::PathBuf, String> {
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

fn derive_target_path(current_target_path: &Path, response: &reqwest::Response) -> std::path::PathBuf {
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

async fn notify_download_failure(app: &AppHandle, state: &SharedState, task: &crate::state::DownloadTask, error: Option<&str>) {
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

    if !matches!(notification.permission_state(), Ok(PermissionState::Granted)) {
        return;
    }

    let _ = notification.builder().title(title).body(body).show();
}
