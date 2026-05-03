use super::*;

pub(super) async fn ensure_parent_directory(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Download path has no parent directory.".to_string())?;

    fs::create_dir_all(parent)
        .await
        .map_err(|error| format!("Could not create download directory: {error}"))
}

pub(super) async fn truncate_file(path: &Path) -> Result<(), String> {
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

pub(super) async fn metadata_len(path: &Path) -> Option<u64> {
    fs::metadata(path).await.ok().map(|metadata| metadata.len())
}

pub(super) async fn compute_sha256(path: &Path) -> Result<String, String> {
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

pub(super) async fn move_to_final_path(
    temp_path: &Path,
    target_path: &Path,
) -> Result<std::path::PathBuf, String> {
    let final_path = allocate_final_path(target_path).await?;

    fs::rename(temp_path, &final_path)
        .await
        .map_err(|error| format!("Could not finalize downloaded file: {error}"))?;

    Ok(final_path)
}

pub(super) async fn allocate_final_path(target_path: &Path) -> Result<std::path::PathBuf, String> {
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

pub(super) fn extract_filename(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_disposition_filename)
}

pub(super) fn parse_content_disposition_filename(header_value: &str) -> Option<String> {
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

pub(super) fn decode_content_disposition_filename(value: &str) -> String {
    let value = value.trim().trim_matches('"').trim();
    let encoded = value.split("''").nth(1).unwrap_or(value);
    let decoded = percent_decode_str(encoded).decode_utf8_lossy();
    sanitize_filename(decoded.trim())
}

pub(super) fn derive_target_path(
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

pub(super) fn fallback_filename(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("download.bin")
        .to_string()
}

pub(super) fn derive_filename_from_url(raw_url: &str) -> Option<String> {
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

pub(super) fn parse_content_range_total(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.parse::<u64>().ok()
}

pub(super) fn content_range_matches(
    value: &str,
    expected_range: ByteRange,
    expected_total: u64,
) -> bool {
    let Some((range, total)) = parse_content_range(value) else {
        return false;
    };

    range == expected_range && total == expected_total
}

pub(super) fn parse_content_range(value: &str) -> Option<(ByteRange, u64)> {
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

pub(super) fn sanitize_filename(input: &str) -> String {
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

pub(super) fn is_windows_reserved_filename(filename: &str) -> bool {
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

pub(super) fn should_retry_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    ) || status.is_server_error()
}

pub(super) fn should_retry_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request() || error.is_body()
}

pub(super) fn download_error(
    category: FailureCategory,
    message: String,
    retryable: bool,
) -> DownloadError {
    DownloadError {
        category,
        message,
        retryable,
    }
}

pub(super) fn error_for_http_status(
    status: StatusCode,
    authenticated_handoff: bool,
) -> DownloadError {
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

pub(super) fn request_error(error: reqwest::Error) -> DownloadError {
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

pub(super) fn download_stream_error(error: reqwest::Error) -> DownloadError {
    let retryable = should_retry_error(&error);
    let category = if retryable {
        FailureCategory::Network
    } else {
        FailureCategory::Internal
    };

    download_error(category, format!("Download failed: {error}"), retryable)
}

pub(super) fn disk_error(message: String) -> DownloadError {
    download_error(FailureCategory::Disk, message, false)
}

pub(super) fn retry_delay_for_attempt(attempt: usize) -> Duration {
    REQUEST_RETRY_DELAYS
        .get(attempt)
        .copied()
        .unwrap_or_else(|| *REQUEST_RETRY_DELAYS.last().unwrap())
}

pub(super) fn throttle_delay_for_limit(
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

pub(super) async fn throttle_download(
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
