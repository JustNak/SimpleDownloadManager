use super::*;
use tokio::io::AsyncWriteExt;

impl SharedState {
    pub async fn begin_browser_blob_download(
        &self,
        stream_id: String,
        source: DownloadSource,
        filename_hint: Option<String>,
        total_bytes: Option<u64>,
        mime_type: Option<String>,
    ) -> Result<EnqueueResult, BackendError> {
        validate_browser_blob_stream_id(&stream_id)?;

        let mut streams = self.browser_blob_streams.write().await;
        if streams.contains_key(&stream_id) {
            return Err(BackendError {
                code: "INVALID_PAYLOAD",
                message: "Browser blob stream is already active.".into(),
            });
        }

        let (result, persisted, diagnostic_events, stream) = {
            let mut state = self.inner.write().await;
            let directory = state.settings.download_directory.trim();
            if directory.is_empty() {
                return Err(BackendError {
                    code: "DESTINATION_NOT_CONFIGURED",
                    message: "Configure a download directory before adding downloads.".into(),
                });
            }

            let download_dir = PathBuf::from(directory);
            std::fs::create_dir_all(&download_dir).map_err(|error| BackendError {
                code: "DESTINATION_INVALID",
                message: format!("Could not create the download directory: {error}"),
            })?;

            let filename = browser_blob_filename(filename_hint.as_deref(), mime_type.as_deref());
            let target_dir = prepare_category_download_directory(&download_dir, &filename)?;
            verify_download_directory_writable(&target_dir)?;
            let (target_path, temp_path) =
                allocate_target_paths(&target_dir, &filename, &state.jobs);
            std::fs::File::create(&temp_path).map_err(|error| BackendError {
                code: "DESTINATION_INVALID",
                message: format!("Could not create partial browser blob download: {error}"),
            })?;

            let job_id = format!("job_{}", state.next_job_number);
            state.next_job_number += 1;
            let now = current_unix_timestamp_millis();
            let total = total_bytes.unwrap_or(0);
            state.push_job(DownloadJob {
                id: job_id.clone(),
                url: browser_blob_url(&stream_id),
                filename: filename.clone(),
                source: Some(source),
                transfer_kind: TransferKind::BrowserBlob,
                integrity_check: None,
                torrent: None,
                state: JobState::Downloading,
                removal_state: None,
                created_at: now,
                progress: 0.0,
                total_bytes: total,
                downloaded_bytes: 0,
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
                target_path: target_path.display().to_string(),
                temp_path: temp_path.display().to_string(),
                artifact_exists: None,
                bulk_archive: None,
            });
            state.push_diagnostic_event(
                DiagnosticLevel::Info,
                "download".into(),
                format!("Receiving browser blob {filename}"),
                Some(job_id.clone()),
            );
            let result = EnqueueResult {
                snapshot: state.snapshot(),
                job_id: job_id.clone(),
                filename,
                status: EnqueueStatus::Queued,
            };
            let stream = BrowserBlobStream {
                job_id,
                downloaded_bytes: 0,
                total_bytes,
                target_path,
                temp_path,
            };
            let persisted = state.persisted();
            let diagnostic_events = state.take_pending_diagnostic_events();
            (result, persisted, diagnostic_events, stream)
        };

        streams.insert(stream_id, stream);
        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(result)
    }

    pub async fn append_browser_blob_download_chunk(
        &self,
        stream_id: &str,
        offset: u64,
        chunk: &[u8],
    ) -> Result<DesktopSnapshot, BackendError> {
        validate_browser_blob_stream_id(stream_id)?;
        if chunk.is_empty() {
            return Err(BackendError {
                code: "INVALID_PAYLOAD",
                message: "Browser blob chunk cannot be empty.".into(),
            });
        }

        let (snapshot, persisted) = {
            let mut streams = self.browser_blob_streams.write().await;
            let stream = streams.get_mut(stream_id).ok_or_else(|| BackendError {
                code: "INVALID_PAYLOAD",
                message: "Browser blob stream is not active.".into(),
            })?;
            if stream.downloaded_bytes != offset {
                return Err(BackendError {
                    code: "INVALID_PAYLOAD",
                    message: format!(
                        "Browser blob chunk offset {} did not match expected offset {}.",
                        offset, stream.downloaded_bytes
                    ),
                });
            }

            let mut file = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&stream.temp_path)
                .await
                .map_err(|error| BackendError {
                    code: "DESTINATION_INVALID",
                    message: format!("Could not open partial browser blob download: {error}"),
                })?;
            file.write_all(chunk).await.map_err(|error| BackendError {
                code: "DESTINATION_INVALID",
                message: format!("Could not write browser blob download: {error}"),
            })?;
            file.flush().await.map_err(|error| BackendError {
                code: "DESTINATION_INVALID",
                message: format!("Could not flush browser blob download: {error}"),
            })?;

            stream.downloaded_bytes = stream.downloaded_bytes.saturating_add(chunk.len() as u64);
            let job_id = stream.job_id.clone();
            let downloaded_bytes = stream.downloaded_bytes;
            let total_bytes = stream.total_bytes;
            let mut state = self.inner.write().await;
            if let Some(job) = state.job_mut(&job_id) {
                job.downloaded_bytes = downloaded_bytes;
                job.total_bytes = total_bytes.unwrap_or(downloaded_bytes);
                job.progress = progress_percent(downloaded_bytes, job.total_bytes);
                job.speed = 0;
                job.eta = 0;
            }
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn finish_browser_blob_download(
        &self,
        stream_id: &str,
    ) -> Result<DesktopSnapshot, BackendError> {
        validate_browser_blob_stream_id(stream_id)?;
        let stream = {
            let mut streams = self.browser_blob_streams.write().await;
            let stream = streams
                .get(stream_id)
                .cloned()
                .ok_or_else(|| BackendError {
                    code: "INVALID_PAYLOAD",
                    message: "Browser blob stream is not active.".into(),
                })?;
            if stream
                .total_bytes
                .is_some_and(|total_bytes| total_bytes != stream.downloaded_bytes)
            {
                return Err(BackendError {
                    code: "INVALID_PAYLOAD",
                    message: "Browser blob stream ended before all bytes were received.".into(),
                });
            }
            streams.remove(stream_id).expect("stream was checked above")
        };

        tokio::fs::rename(&stream.temp_path, &stream.target_path)
            .await
            .map_err(|error| BackendError {
                code: "DESTINATION_INVALID",
                message: format!("Could not finalize browser blob download: {error}"),
            })?;

        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            let job_id = stream.job_id.clone();
            let filename = if let Some(job) = state.job_mut(&job_id) {
                job.state = JobState::Completed;
                job.downloaded_bytes = stream.downloaded_bytes;
                job.total_bytes = stream.total_bytes.unwrap_or(stream.downloaded_bytes);
                job.progress = 100.0;
                job.speed = 0;
                job.eta = 0;
                job.error = None;
                job.failure_category = None;
                Some(job.filename.clone())
            } else {
                None
            };
            if let Some(filename) = filename {
                state.push_diagnostic_event(
                    DiagnosticLevel::Info,
                    "download".into(),
                    format!("Completed browser blob {filename}"),
                    Some(job_id),
                );
            }
            let snapshot = state.snapshot();
            let persisted = state.persisted();
            let diagnostic_events = state.take_pending_diagnostic_events();
            (snapshot, persisted, diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
    }

    pub async fn cancel_browser_blob_download(
        &self,
        stream_id: &str,
        reason: Option<String>,
    ) -> Result<DesktopSnapshot, BackendError> {
        validate_browser_blob_stream_id(stream_id)?;
        let stream = self.browser_blob_streams.write().await.remove(stream_id);
        let Some(stream) = stream else {
            return Ok(self.snapshot().await);
        };

        let _ = tokio::fs::remove_file(&stream.temp_path).await;
        let reason = reason
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Browser blob stream was canceled.".into());
        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            if let Some(job) = state.job_mut(&stream.job_id) {
                job.state = JobState::Failed;
                job.speed = 0;
                job.eta = 0;
                job.error = Some(reason.clone());
                job.failure_category = Some(FailureCategory::Network);
            }
            state.push_diagnostic_event(
                DiagnosticLevel::Warning,
                "download".into(),
                reason,
                Some(stream.job_id),
            );
            let snapshot = state.snapshot();
            let persisted = state.persisted();
            let diagnostic_events = state.take_pending_diagnostic_events();
            (snapshot, persisted, diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
    }
}

fn browser_blob_url(stream_id: &str) -> String {
    format!("browser-blob://{stream_id}")
}

fn validate_browser_blob_stream_id(stream_id: &str) -> Result<(), BackendError> {
    if stream_id.trim().is_empty()
        || stream_id.len() > 128
        || !stream_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':')
        })
    {
        return Err(BackendError {
            code: "INVALID_PAYLOAD",
            message: "Browser blob stream id is not supported.".into(),
        });
    }

    Ok(())
}

fn browser_blob_filename(filename_hint: Option<&str>, mime_type: Option<&str>) -> String {
    if let Some(filename_hint) = filename_hint {
        let filename = sanitize_filename(filename_hint);
        if filename != "download.bin" {
            return filename;
        }
    }

    let extension = browser_blob_mime_extension(mime_type).unwrap_or("bin");
    format!("download.{extension}")
}

fn browser_blob_mime_extension(mime_type: Option<&str>) -> Option<&'static str> {
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

fn progress_percent(downloaded_bytes: u64, total_bytes: u64) -> f64 {
    if total_bytes == 0 {
        return 0.0;
    }

    ((downloaded_bytes as f64 / total_bytes as f64) * 100.0).clamp(0.0, 100.0)
}
