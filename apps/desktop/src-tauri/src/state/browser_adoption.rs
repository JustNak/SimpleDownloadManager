use super::*;

impl SharedState {
    pub async fn adopt_browser_download(
        &self,
        url: String,
        source: DownloadSource,
        local_path: String,
        filename_hint: Option<String>,
        total_bytes: Option<u64>,
        mime_type: Option<String>,
    ) -> Result<EnqueueResult, BackendError> {
        let source_path = PathBuf::from(local_path.trim());
        let metadata = std::fs::metadata(&source_path).map_err(|error| BackendError {
            code: "DESTINATION_INVALID",
            message: format!("Could not inspect completed browser download: {error}"),
        })?;
        if !metadata.is_file() {
            return Err(BackendError {
                code: "DESTINATION_INVALID",
                message: "Completed browser download path is not a file.".into(),
            });
        }

        let filename = source_path
            .file_name()
            .and_then(|value| value.to_str())
            .map(sanitize_filename)
            .filter(|value| value != "download.bin")
            .or_else(|| {
                filename_hint
                    .as_deref()
                    .map(sanitize_filename)
                    .filter(|value| value != "download.bin")
            })
            .unwrap_or_else(|| browser_adopted_filename(None, mime_type.as_deref()));
        let downloaded_bytes = metadata.len();
        let total = total_bytes
            .filter(|value| *value >= downloaded_bytes)
            .unwrap_or(downloaded_bytes);
        let source_path_key = source_path.display().to_string();

        let (result, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            if let Some(existing) = state.jobs.iter().find(|job| {
                Path::new(&job.target_path) == source_path.as_path()
                    && matches!(job.state, JobState::Completed)
            }) {
                let result = EnqueueResult {
                    snapshot: state.snapshot(),
                    job_id: existing.id.clone(),
                    filename: existing.filename.clone(),
                    status: EnqueueStatus::DuplicateExistingJob,
                };
                return Ok(result);
            }

            let job_id = format!("job_{}", state.next_job_number);
            state.next_job_number += 1;
            let now = current_unix_timestamp_millis();
            state.push_job(DownloadJob {
                id: job_id.clone(),
                url,
                filename: filename.clone(),
                source: Some(source),
                transfer_kind: TransferKind::BrowserAdopted,
                integrity_check: None,
                torrent: None,
                state: JobState::Completed,
                removal_state: None,
                created_at: now,
                progress: 100.0,
                total_bytes: total,
                downloaded_bytes,
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
                target_path: source_path_key.clone(),
                temp_path: format!("{source_path_key}.part"),
                artifact_exists: Some(true),
                bulk_archive: None,
            });
            state.push_diagnostic_event(
                DiagnosticLevel::Info,
                "download".into(),
                format!("Adopted completed browser download {filename}"),
                Some(job_id.clone()),
            );
            let result = EnqueueResult {
                snapshot: state.snapshot(),
                job_id,
                filename,
                status: EnqueueStatus::Queued,
            };
            let persisted = state.persisted();
            let diagnostic_events = state.take_pending_diagnostic_events();
            (result, persisted, diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(result)
    }
}

fn browser_adopted_filename(filename_hint: Option<&str>, mime_type: Option<&str>) -> String {
    if let Some(filename_hint) = filename_hint {
        let filename = sanitize_filename(filename_hint);
        if filename != "download.bin" {
            return filename;
        }
    }

    let extension = browser_adopted_mime_extension(mime_type).unwrap_or("bin");
    format!("download.{extension}")
}

fn browser_adopted_mime_extension(mime_type: Option<&str>) -> Option<&'static str> {
    let mime_type = mime_type?.split(';').next()?.trim().to_ascii_lowercase();
    match mime_type.as_str() {
        "application/pdf" => Some("pdf"),
        "application/zip" | "application/x-zip-compressed" => Some("zip"),
        "application/vnd.rar" | "application/x-rar-compressed" => Some("rar"),
        "application/x-7z-compressed" => Some("7z"),
        "application/gzip" => Some("gz"),
        "audio/mpeg" => Some("mp3"),
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "text/plain" => Some("txt"),
        "video/mp4" => Some("mp4"),
        "video/quicktime" => Some("mov"),
        "video/webm" => Some("webm"),
        "video/x-matroska" => Some("mkv"),
        _ => None,
    }
}
