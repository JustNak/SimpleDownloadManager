use super::*;

pub(super) fn validate_handoff_auth_headers(auth: &HandoffAuth) -> Result<(), BackendError> {
    if auth.headers.is_empty() || auth.headers.len() > MAX_HANDOFF_AUTH_HEADERS {
        return Err(BackendError {
            code: "INVALID_PAYLOAD",
            message: "Authenticated handoff header count is not supported.".into(),
        });
    }

    for header in &auth.headers {
        validate_handoff_auth_header(header)?;
    }

    Ok(())
}

pub(super) fn validate_handoff_auth_header(header: &HandoffAuthHeader) -> Result<(), BackendError> {
    let name = header.name.trim();
    if name.is_empty()
        || name.len() > MAX_HANDOFF_AUTH_HEADER_NAME_LENGTH
        || header.value.len() > MAX_HANDOFF_AUTH_HEADER_VALUE_LENGTH
        || name.contains(':')
        || name.contains('\r')
        || name.contains('\n')
        || header.value.contains('\r')
        || header.value.contains('\n')
        || !is_allowed_handoff_auth_header(name)
    {
        return Err(BackendError {
            code: "INVALID_PAYLOAD",
            message: "Authenticated handoff header is not allowed.".into(),
        });
    }

    Ok(())
}

pub(super) fn is_allowed_handoff_auth_header(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    matches!(
        name.as_str(),
        "cookie"
            | "authorization"
            | "referer"
            | "origin"
            | "user-agent"
            | "accept"
            | "accept-language"
    ) || name.starts_with("sec-fetch-")
        || name.starts_with("sec-ch-ua")
}

pub(super) fn normalize_expected_sha256(
    value: Option<String>,
) -> Result<Option<String>, BackendError> {
    let Some(value) = value else {
        return Ok(None);
    };

    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(None);
    }

    if normalized.len() != SHA256_HEX_LENGTH
        || !normalized
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(BackendError {
            code: "INVALID_INTEGRITY_HASH",
            message: "SHA-256 checksum must be 64 hexadecimal characters.".into(),
        });
    }

    Ok(Some(normalized))
}

pub(super) fn normalize_download_url(raw_url: &str) -> Result<String, BackendError> {
    let trimmed_url = raw_url.trim();
    if trimmed_url.len() > MAX_URL_LENGTH {
        return Err(BackendError {
            code: "URL_TOO_LONG",
            message: format!("URL exceeds {MAX_URL_LENGTH} characters."),
        });
    }

    let parsed = Url::parse(trimmed_url).map_err(|_| BackendError {
        code: "INVALID_URL",
        message: "URL is not valid.".into(),
    })?;

    match parsed.scheme() {
        "http" | "https" | "magnet" => Ok(parsed.to_string()),
        _ => Err(BackendError {
            code: "UNSUPPORTED_SCHEME",
            message: "Only http, https, magnet, and HTTP(S) .torrent URLs are supported.".into(),
        }),
    }
}

pub(super) fn normalize_download_input(
    raw_input: &str,
    explicit_transfer_kind: Option<TransferKind>,
) -> Result<String, BackendError> {
    match normalize_download_url(raw_input) {
        Ok(url) => Ok(url),
        Err(url_error) if explicit_transfer_kind == Some(TransferKind::Torrent) => {
            normalize_local_torrent_file(raw_input).map_err(|_| url_error)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn normalize_local_torrent_file(raw_path: &str) -> Result<String, BackendError> {
    let trimmed_path = raw_path.trim();
    if trimmed_path.is_empty() {
        return Err(BackendError {
            code: "INVALID_URL",
            message: "Torrent file path is empty.".into(),
        });
    }

    let path = PathBuf::from(trimmed_path);
    let is_torrent_file = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("torrent"));

    if !is_torrent_file || !path.is_file() {
        return Err(BackendError {
            code: "INVALID_TRANSFER_KIND",
            message: "Choose an existing .torrent file.".into(),
        });
    }

    Ok(path.display().to_string())
}

pub(super) fn transfer_kind_for_url(url: &str) -> TransferKind {
    if path_has_torrent_extension(Path::new(url)) {
        return TransferKind::Torrent;
    }

    let Ok(parsed) = Url::parse(url) else {
        return TransferKind::Http;
    };

    if parsed.scheme() == "magnet" || url_path_has_torrent_extension(&parsed) {
        TransferKind::Torrent
    } else {
        TransferKind::Http
    }
}

pub(super) fn url_path_has_torrent_extension(url: &Url) -> bool {
    url.path_segments()
        .and_then(|mut segments| segments.next_back())
        .map(|segment| segment.to_ascii_lowercase().ends_with(".torrent"))
        .unwrap_or(false)
}

pub(super) fn path_has_torrent_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("torrent"))
}

pub(super) fn torrent_filename_from_url(raw_url: &str, filename_hint: Option<&str>) -> String {
    if let Some(hint) = filename_hint {
        let filename = sanitize_filename(hint);
        if filename != "download.bin" {
            return filename;
        }
    }

    if let Some(filename) = torrent_filename_from_path(raw_url) {
        return filename;
    }

    let Ok(parsed) = Url::parse(raw_url) else {
        return "torrent".into();
    };

    if parsed.scheme() == "magnet" {
        if let Some(display_name) = parsed
            .query_pairs()
            .find_map(|(key, value)| (key == "dn").then(|| sanitize_filename(&value)))
            .filter(|value| value != "download.bin")
        {
            return display_name;
        }

        if let Some(hash) = parsed
            .query_pairs()
            .find_map(|(key, value)| (key == "xt").then_some(value.into_owned()))
            .and_then(|value| value.rsplit(':').next().map(str::to_string))
            .filter(|value| !value.is_empty())
        {
            let prefix = hash.chars().take(8).collect::<String>();
            return format!("torrent-{prefix}");
        }

        return "torrent".into();
    }

    filename_from_hint(filename_hint, raw_url)
}

pub(super) fn torrent_filename_from_path(raw_path: &str) -> Option<String> {
    let path = Path::new(raw_path.trim());
    if !path_has_torrent_extension(path) {
        return None;
    }

    path.file_stem()
        .and_then(|value| value.to_str())
        .map(sanitize_filename)
        .filter(|value| value != "download.bin")
}

pub(super) fn torrent_state_path_for_job(download_dir: &Path, job_id: &str) -> PathBuf {
    download_dir.join(".torrent-state").join(job_id)
}

pub(super) fn verify_download_directory_writable(download_dir: &Path) -> Result<(), BackendError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let probe_name = format!(
        ".simple-download-manager-write-test-{}-{timestamp}",
        std::process::id()
    );

    verify_download_directory_writable_with_probe_name(download_dir, &probe_name)
}

pub(super) fn verify_download_directory_writable_with_probe_name(
    download_dir: &Path,
    probe_name: &str,
) -> Result<(), BackendError> {
    let probe_path = download_dir.join(probe_name);
    let probe_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe_path)
        .map_err(destination_write_error)?;
    drop(probe_file);

    std::fs::remove_file(&probe_path).map_err(destination_write_error)?;
    Ok(())
}

pub(super) fn destination_write_error(error: std::io::Error) -> BackendError {
    let code = if error.kind() == std::io::ErrorKind::PermissionDenied {
        "PERMISSION_DENIED"
    } else {
        "DESTINATION_INVALID"
    };

    BackendError {
        code,
        message: format!("Download directory is not writable: {error}"),
    }
}

pub(super) fn prepare_category_download_directory(
    download_dir: &Path,
    filename: &str,
) -> Result<PathBuf, BackendError> {
    ensure_download_category_directories(download_dir).map_err(|error| BackendError {
        code: "DESTINATION_INVALID",
        message: error,
    })?;
    Ok(category_download_directory(download_dir, filename))
}

pub(super) fn ensure_download_category_directories(download_dir: &Path) -> Result<(), String> {
    for folder in DOWNLOAD_CATEGORY_FOLDERS {
        let category_dir = download_dir.join(folder);
        std::fs::create_dir_all(&category_dir).map_err(|error| {
            format!("Could not create {folder} download category directory: {error}")
        })?;
    }

    Ok(())
}

pub(super) fn category_download_directory(download_dir: &Path, filename: &str) -> PathBuf {
    download_dir.join(category_folder_for_filename(filename))
}

pub(super) fn category_folder_for_filename(filename: &str) -> &'static str {
    let Some(extension) = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
    else {
        return "Other";
    };

    match extension.as_str() {
        ext if DOCUMENT_EXTENSIONS.contains(&ext) => "Document",
        ext if PROGRAM_EXTENSIONS.contains(&ext) => "Program",
        ext if PICTURE_EXTENSIONS.contains(&ext) => "Picture",
        ext if VIDEO_EXTENSIONS.contains(&ext) => "Video",
        ext if COMPRESSED_EXTENSIONS.contains(&ext) => "Compressed",
        ext if MUSIC_EXTENSIONS.contains(&ext) => "Music",
        _ => "Other",
    }
}

pub(super) fn allocate_target_paths(
    download_dir: &Path,
    filename: &str,
    jobs: &[DownloadJob],
) -> (PathBuf, PathBuf) {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();

    let occupied_targets = jobs
        .iter()
        .map(|job| job.target_path.clone())
        .collect::<HashSet<_>>();
    let occupied_temps = jobs
        .iter()
        .map(|job| job.temp_path.clone())
        .collect::<HashSet<_>>();

    for index in 0..10_000 {
        let candidate = if index == 0 {
            format!("{stem}{extension}")
        } else {
            format!("{stem} ({index}){extension}")
        };
        let target_path = download_dir.join(&candidate);
        let temp_path = download_dir.join(format!("{candidate}.part"));
        let target_key = target_path.display().to_string();
        let temp_key = temp_path.display().to_string();

        if occupied_targets.contains(&target_key) || occupied_temps.contains(&temp_key) {
            continue;
        }

        if target_path.exists() || temp_path.exists() {
            continue;
        }

        return (target_path, temp_path);
    }

    let fallback_target = download_dir.join(filename);
    let fallback_temp = download_dir.join(format!("{filename}.part"));
    (fallback_target, fallback_temp)
}

pub(super) fn candidate_target_paths(download_dir: &Path, filename: &str) -> (PathBuf, PathBuf) {
    let target_path = download_dir.join(filename);
    let temp_path = download_dir.join(format!("{filename}.part"));
    (target_path, temp_path)
}

pub(super) fn unique_target_path(
    download_dir: &Path,
    filename: &str,
    jobs: &[DownloadJob],
) -> PathBuf {
    let (target_path, _) = allocate_target_paths(download_dir, filename, jobs);
    target_path
}

pub(super) fn derive_filename(raw_url: &str) -> String {
    let fallback = "download.bin".to_string();
    let Ok(url) = Url::parse(raw_url) else {
        return fallback;
    };

    let candidate = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .unwrap_or("download.bin");

    let decoded = percent_decode_str(candidate).decode_utf8_lossy();
    sanitize_filename(&decoded)
}

pub(super) fn filename_from_hint(filename_hint: Option<&str>, raw_url: &str) -> String {
    filename_hint
        .map(|hint| {
            let decoded = percent_decode_str(hint).decode_utf8_lossy();
            sanitize_filename(&decoded)
        })
        .filter(|filename| !filename.trim().is_empty())
        .unwrap_or_else(|| derive_filename(raw_url))
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

    if sanitized.trim().is_empty() {
        "download.bin".into()
    } else {
        if is_windows_reserved_filename(&sanitized) {
            sanitized.push('_');
        }
        sanitized
    }
}

pub(super) fn normalize_archive_filename(input: &str) -> String {
    let mut filename = sanitize_filename(input);
    if !filename.to_ascii_lowercase().ends_with(".zip") {
        filename.push_str(".zip");
    }
    filename
}

pub(super) fn unique_archive_entry_name(
    filename: &str,
    used_names: &mut HashSet<String>,
) -> String {
    let sanitized = sanitize_filename(filename);
    if used_names.insert(sanitized.clone()) {
        return sanitized;
    }

    let stem = Path::new(&sanitized)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = Path::new(&sanitized)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();

    for index in 1..10_000 {
        let candidate = format!("{stem} ({index}){extension}");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
    }

    sanitized
}

pub(super) fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not remove partial download file: {error}")),
    }
}

pub(super) fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
            .map_err(|error| format!("Could not remove download directory: {error}"))
    } else {
        remove_file_if_exists(path)
    }
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
