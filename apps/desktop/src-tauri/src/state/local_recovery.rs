use super::*;
use sha2::{Digest, Sha256};

const LOCAL_RECOVERY_ENTRY_POINT: &str = "local_recovery";
const LOCAL_RECOVERY_URL_PREFIX: &str = "recovered://local-file/";
const LOCAL_RECOVERY_DISABLED_ACTION: &str =
    "Recovered local files cannot be retried or restarted because the original download URL was not recoverable.";

impl SharedState {
    pub async fn preview_local_recovery(
        &self,
        root: Option<String>,
    ) -> Result<LocalRecoveryPreview, BackendError> {
        let (root_path, existing_targets) = {
            let state = self.inner.read().await;
            let root_path = root
                .as_deref()
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(&state.settings.download_directory));
            let existing_targets = state
                .jobs
                .iter()
                .map(|job| normalize_recovery_path_key(Path::new(&job.target_path)))
                .collect::<HashSet<_>>();
            (root_path, existing_targets)
        };

        preview_local_recovery_for_root(root_path, existing_targets)
            .await
            .map_err(internal_error)
    }

    pub async fn import_local_recovery(
        &self,
        candidate_ids: Vec<String>,
    ) -> Result<DesktopSnapshot, BackendError> {
        let selected_ids = candidate_ids
            .into_iter()
            .filter(|id| !id.trim().is_empty())
            .collect::<HashSet<_>>();
        if selected_ids.is_empty() {
            return Err(BackendError {
                code: "NO_LOCAL_RECOVERY_SELECTION",
                message: "Select at least one local file to recover.".into(),
            });
        }

        let preview = self.preview_local_recovery(None).await?;
        let selected_candidates = preview
            .candidates
            .into_iter()
            .filter(|candidate| selected_ids.contains(&candidate.id))
            .collect::<Vec<_>>();
        if selected_candidates.is_empty() {
            return Err(BackendError {
                code: "LOCAL_RECOVERY_NOT_FOUND",
                message: "Selected local recovery files are no longer available.".into(),
            });
        }

        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            for candidate in selected_candidates {
                if state.jobs.iter().any(|job| {
                    normalize_recovery_path_key(Path::new(&job.target_path))
                        == normalize_recovery_path_key(Path::new(&candidate.path))
                }) {
                    continue;
                }

                let job_id = format!("job_{}", state.next_job_number);
                state.next_job_number += 1;
                state.push_job(local_recovery_job(&job_id, candidate));
            }
            state.startup_recovery = None;
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }
}

pub(super) fn is_recovered_local_job(job: &DownloadJob) -> bool {
    job.url.starts_with(LOCAL_RECOVERY_URL_PREFIX)
        || job
            .source
            .as_ref()
            .is_some_and(|source| source.entry_point == LOCAL_RECOVERY_ENTRY_POINT)
}

pub(super) fn recovered_local_action_error() -> BackendError {
    BackendError {
        code: "UNSUPPORTED_RECOVERY_ACTION",
        message: LOCAL_RECOVERY_DISABLED_ACTION.into(),
    }
}

async fn preview_local_recovery_for_root(
    root_path: PathBuf,
    existing_targets: HashSet<String>,
) -> Result<LocalRecoveryPreview, String> {
    tokio::task::spawn_blocking(move || {
        let root_path = canonical_or_original(root_path);
        let mut candidates = Vec::new();
        let mut skipped_count = 0usize;
        scan_local_recovery_directory(
            &root_path,
            &existing_targets,
            &mut candidates,
            &mut skipped_count,
        )?;
        candidates.sort_by(|left, right| {
            left.path
                .to_ascii_lowercase()
                .cmp(&right.path.to_ascii_lowercase())
                .then_with(|| left.path.cmp(&right.path))
        });

        Ok(LocalRecoveryPreview {
            root: user_visible_recovery_path(&root_path),
            candidates,
            skipped_count,
        })
    })
    .await
    .map_err(|error| format!("Local recovery scanner failed: {error}"))?
}

fn scan_local_recovery_directory(
    directory: &Path,
    existing_targets: &HashSet<String>,
    candidates: &mut Vec<LocalRecoveryCandidate>,
    skipped_count: &mut usize,
) -> Result<(), String> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            return Err(format!(
                "Could not read local recovery directory {}: {error}",
                directory.display()
            ))
        }
    };

    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Could not inspect local recovery file: {error}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Could not inspect local recovery file type: {error}"))?;
        let file_name = entry.file_name().to_string_lossy().to_string();

        if should_skip_local_recovery_entry(&file_name, &path) || file_type.is_symlink() {
            *skipped_count += 1;
            continue;
        }

        if file_type.is_dir() {
            scan_local_recovery_directory(&path, existing_targets, candidates, skipped_count)?;
            continue;
        }

        if !file_type.is_file() || should_skip_local_recovery_file(&file_name) {
            *skipped_count += 1;
            continue;
        }

        let canonical_path = canonical_or_original(path);
        if existing_targets.contains(&normalize_recovery_path_key(&canonical_path)) {
            *skipped_count += 1;
            continue;
        }
        let metadata = std::fs::metadata(&canonical_path).map_err(|error| {
            format!(
                "Could not inspect local recovery file {}: {error}",
                canonical_path.display()
            )
        })?;
        candidates.push(LocalRecoveryCandidate {
            id: local_recovery_candidate_id(&canonical_path, &metadata),
            path: user_visible_recovery_path(&canonical_path),
            filename: canonical_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Recovered file")
                .to_string(),
            size_bytes: metadata.len(),
            modified_at: metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as u64),
        });
    }

    Ok(())
}

fn should_skip_local_recovery_entry(file_name: &str, path: &Path) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower == ".torrent-state"
        || lower == "state.backups"
        || lower == "desktop.ini"
        || lower == "thumbs.db"
        || lower == ".ds_store"
        || file_name.starts_with('.')
        || path
            .components()
            .any(|component| component.as_os_str().to_string_lossy() == ".torrent-state")
}

fn should_skip_local_recovery_file(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.ends_with(".part")
        || lower.ends_with(".crdownload")
        || lower.ends_with(".tmp")
        || lower.ends_with(".download")
        || lower == "state.json"
        || lower == "state.json.bak"
        || lower == "state.last-good.json"
        || lower == "diagnostic-events.jsonl"
}

fn local_recovery_candidate_id(path: &Path, metadata: &std::fs::Metadata) -> String {
    let mut hasher = Sha256::new();
    let path_key = normalize_recovery_path_key(path);
    hasher.update(path_key.as_bytes());
    hasher.update(metadata.len().to_le_bytes());
    if let Ok(modified) = metadata.modified() {
        if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
            hasher.update(duration.as_millis().to_le_bytes());
        }
    }
    let digest = hasher.finalize();
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn local_recovery_job(job_id: &str, candidate: LocalRecoveryCandidate) -> DownloadJob {
    let target_path = candidate.path;
    DownloadJob {
        id: job_id.into(),
        url: format!("{LOCAL_RECOVERY_URL_PREFIX}{}", candidate.id),
        filename: candidate.filename,
        source: Some(DownloadSource {
            entry_point: LOCAL_RECOVERY_ENTRY_POINT.into(),
            browser: "local".into(),
            extension_version: "recovery".into(),
            page_url: None,
            page_title: None,
            referrer: None,
            incognito: None,
        }),
        transfer_kind: TransferKind::Http,
        integrity_check: None,
        torrent: None,
        state: JobState::Completed,
        removal_state: None,
        created_at: current_unix_timestamp_millis(),
        progress: 100.0,
        total_bytes: candidate.size_bytes,
        downloaded_bytes: candidate.size_bytes,
        speed: 0,
        eta: 0,
        active_segments: None,
        planned_segments: None,
        error: None,
        failure_category: None,
        resume_support: ResumeSupport::Unsupported,
        retry_attempts: 0,
        auto_restart_attempts: 0,
        resolved_from_url: None,
        hoster_preflight: None,
        target_path: target_path.clone(),
        temp_path: target_path,
        artifact_exists: Some(true),
        bulk_archive: None,
    }
}

fn canonical_or_original(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn normalize_recovery_path_key(path: &Path) -> String {
    let value =
        user_visible_recovery_path(&canonical_or_original(path.to_path_buf())).replace('\\', "/");
    if cfg!(windows) {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

fn user_visible_recovery_path(path: &Path) -> String {
    let value = path.display().to_string();
    if let Some(stripped) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{stripped}")
    } else if let Some(stripped) = value.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        value
    }
}
