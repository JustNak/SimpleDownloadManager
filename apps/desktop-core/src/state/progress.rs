use super::*;

impl SharedState {
    pub async fn sync_downloaded_bytes(
        &self,
        id: &str,
        downloaded_bytes: u64,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            job.downloaded_bytes = downloaded_bytes;
            if job.total_bytes > 0 {
                job.progress =
                    (downloaded_bytes as f64 / job.total_bytes as f64 * 100.0).clamp(0.0, 100.0);
            } else {
                job.progress = 0.0;
            }
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn mark_job_downloading(
        &self,
        id: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        resume_support: ResumeSupport,
        filename: Option<String>,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            job.state = JobState::Downloading;
            if let Some(filename) = filename {
                apply_download_filename(job, &filename);
            }
            job.downloaded_bytes = downloaded_bytes;
            if let Some(total_bytes) = total_bytes {
                job.total_bytes = total_bytes.max(downloaded_bytes);
            }
            job.progress = if job.total_bytes == 0 {
                0.0
            } else {
                (job.downloaded_bytes as f64 / job.total_bytes as f64 * 100.0).clamp(0.0, 100.0)
            };
            job.error = None;
            job.failure_category = None;
            job.resume_support = resume_support;
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn apply_preflight_metadata(
        &self,
        id: &str,
        total_bytes: Option<u64>,
        resume_support: ResumeSupport,
        filename: Option<String>,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            apply_preflight_metadata_to_job(job, total_bytes, resume_support, filename);
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn record_retry_attempt(
        &self,
        id: &str,
        retry_attempts: u32,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let event_message = {
                let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                    return Err("Job not found.".into());
                };

                job.retry_attempts = retry_attempts;
                job.speed = 0;
                job.eta = 0;
                format!("Retry attempt {retry_attempts} for {}", job.filename)
            };
            state.push_diagnostic_event(
                DiagnosticLevel::Warning,
                "download".into(),
                event_message,
                Some(id.into()),
            );
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn update_job_progress(
        &self,
        id: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        speed: u64,
        persist: bool,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            job.state = JobState::Downloading;
            job.downloaded_bytes = downloaded_bytes;
            if let Some(total_bytes) = total_bytes {
                job.total_bytes = total_bytes.max(downloaded_bytes);
            }
            job.speed = speed;

            if job.total_bytes > 0 {
                job.progress = (job.downloaded_bytes as f64 / job.total_bytes as f64 * 100.0)
                    .clamp(0.0, 100.0);
                let remaining = job.total_bytes.saturating_sub(job.downloaded_bytes);
                job.eta = if speed == 0 {
                    0
                } else {
                    ((remaining as f64) / (speed as f64)).ceil() as u64
                };
            } else {
                job.progress = 0.0;
                job.eta = 0;
            }

            (state.snapshot(), state.persisted())
        };

        if persist {
            persist_state(&self.storage_path, &persisted)?;
        }
        Ok(snapshot)
    }

    pub async fn job_requires_sha256(&self, id: &str) -> bool {
        let state = self.inner.read().await;
        state
            .jobs
            .iter()
            .find(|job| job.id == id)
            .and_then(|job| job.integrity_check.as_ref())
            .is_some_and(|check| {
                check.algorithm == IntegrityAlgorithm::Sha256
                    && check.status == IntegrityStatus::Pending
            })
    }

    pub async fn complete_job(
        &self,
        id: &str,
        total_bytes: u64,
        target_path: &Path,
    ) -> Result<DesktopSnapshot, String> {
        self.complete_job_with_integrity(id, total_bytes, target_path, None)
            .await
    }

    pub async fn complete_job_with_integrity(
        &self,
        id: &str,
        total_bytes: u64,
        target_path: &Path,
        actual_sha256: Option<String>,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            job.state = JobState::Completed;
            job.downloaded_bytes = total_bytes;
            job.total_bytes = total_bytes;
            job.progress = 100.0;
            job.speed = 0;
            job.eta = 0;
            job.error = None;
            job.filename = target_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(&job.filename)
                .to_string();
            job.target_path = target_path.display().to_string();
            job.temp_path = format!("{}.part", job.target_path);
            let completed_filename = job.filename.clone();
            let mut event = (
                DiagnosticLevel::Info,
                format!("Completed {completed_filename}"),
            );
            if let Some(check) = &mut job.integrity_check {
                if let Some(actual) = actual_sha256 {
                    check.actual = Some(actual.clone());
                    if check.expected.eq_ignore_ascii_case(&actual) {
                        check.status = IntegrityStatus::Verified;
                        event = (
                            DiagnosticLevel::Info,
                            format!("Verified SHA-256 for {completed_filename}"),
                        );
                    } else {
                        check.status = IntegrityStatus::Failed;
                        job.state = JobState::Failed;
                        job.failure_category = Some(FailureCategory::Integrity);
                        job.error = Some(format!(
                            "SHA-256 checksum mismatch. Expected {}, got {actual}.",
                            check.expected
                        ));
                        event = (
                            DiagnosticLevel::Error,
                            format!("SHA-256 verification failed for {completed_filename}"),
                        );
                    }
                }
            }
            state.active_workers.remove(id);
            state.push_diagnostic_event(event.0, "download".into(), event.1, Some(id.into()));
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn mark_bulk_archive_status(
        &self,
        archive_id: &str,
        archive_status: BulkArchiveStatus,
        output_path: Option<String>,
        error: Option<String>,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.mark_bulk_archive_status_in_memory(
                archive_id,
                archive_status,
                output_path,
                error,
            );
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn bulk_archive_ready_for_job(
        &self,
        id: &str,
    ) -> Result<Option<BulkArchiveReady>, String> {
        let state = self.inner.read().await;
        let Some(job) = state.jobs.iter().find(|job| job.id == id) else {
            return Ok(None);
        };
        let Some(archive) = &job.bulk_archive else {
            return Ok(None);
        };
        if archive.archive_status != BulkArchiveStatus::Pending {
            return Ok(None);
        }

        let members = state
            .jobs
            .iter()
            .filter(|candidate| {
                candidate
                    .bulk_archive
                    .as_ref()
                    .is_some_and(|candidate_archive| candidate_archive.id == archive.id)
            })
            .collect::<Vec<_>>();

        if members.len() < 2
            || members
                .iter()
                .any(|member| member.state != JobState::Completed)
        {
            return Ok(None);
        }

        let output_dir = PathBuf::from(&state.settings.download_directory);
        let output_path = output_dir.join(&archive.name);
        if output_path.exists() {
            return Ok(None);
        }

        let mut used_names = HashSet::new();
        let mut entries = Vec::with_capacity(members.len());
        for member in members {
            let source_path = PathBuf::from(&member.target_path);
            if !source_path.is_file() {
                return Ok(None);
            }

            let archive_name = unique_archive_entry_name(&member.filename, &mut used_names);
            entries.push(BulkArchiveEntry {
                source_path,
                archive_name,
            });
        }

        Ok(Some(BulkArchiveReady {
            archive_id: archive.id.clone(),
            output_path,
            entries,
        }))
    }

    pub async fn fail_job(
        &self,
        id: &str,
        message: impl Into<String>,
        failure_category: FailureCategory,
    ) -> Result<DesktopSnapshot, String> {
        let message = message.into();
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.active_workers.remove(id);
            state.external_reseed_jobs.remove(id);
            let event_message = {
                let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                    return Err("Job not found.".into());
                };

                job.state = JobState::Failed;
                job.speed = 0;
                job.eta = 0;
                job.error = Some(message.clone());
                job.failure_category = Some(failure_category);
                format!("Failed {}: {message}", job.filename)
            };
            state.push_diagnostic_event(
                DiagnosticLevel::Error,
                "download".into(),
                event_message,
                Some(id.into()),
            );
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn finish_interrupted_job(&self, id: &str) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.active_workers.remove(id);
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            if job.state == JobState::Canceled && job.transfer_kind != TransferKind::Torrent {
                job.progress = 0.0;
                job.total_bytes = 0;
                job.downloaded_bytes = 0;
                job.error = None;
            }

            job.speed = 0;
            job.eta = 0;
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn resolve_openable_path(&self, id: &str) -> Result<PathBuf, BackendError> {
        let (path, transfer_kind, job_state) = {
            let state = self.inner.read().await;
            let job = state
                .jobs
                .iter()
                .find(|job| job.id == id)
                .ok_or_else(|| BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Job not found.".into(),
                })?;

            (
                PathBuf::from(&job.target_path),
                job.transfer_kind,
                job.state,
            )
        };

        let openable_torrent_directory = transfer_kind == TransferKind::Torrent
            && matches!(job_state, JobState::Completed | JobState::Paused)
            && path.is_dir();

        if path.is_file() || openable_torrent_directory {
            Ok(path)
        } else {
            Err(BackendError {
                code: "INTERNAL_ERROR",
                message: format!(
                    "The downloaded file is not available on disk: {}",
                    path.display()
                ),
            })
        }
    }

    pub async fn resolve_revealable_path(&self, id: &str) -> Result<PathBuf, BackendError> {
        let (job_state, target_path, temp_path) = {
            let state = self.inner.read().await;
            let job = state
                .jobs
                .iter()
                .find(|job| job.id == id)
                .ok_or_else(|| BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Job not found.".into(),
                })?;

            (
                job.state,
                PathBuf::from(&job.target_path),
                PathBuf::from(&job.temp_path),
            )
        };

        if target_path.exists() {
            return Ok(target_path);
        }

        if temp_path.exists() {
            return Ok(temp_path);
        }

        if job_state == JobState::Completed {
            return Err(BackendError {
                code: "INTERNAL_ERROR",
                message: format!(
                    "Downloaded file is missing from disk: {}",
                    target_path.display()
                ),
            });
        }

        if let Some(parent) = target_path.parent() {
            if parent.exists() {
                return Ok(parent.to_path_buf());
            }
        }

        Err(BackendError {
            code: "INTERNAL_ERROR",
            message: format!(
                "No local path is available for this job yet. Expected path: {}",
                target_path.display()
            ),
        })
    }
}

impl RuntimeState {
    pub(super) fn mark_bulk_archive_status_in_memory(
        &mut self,
        archive_id: &str,
        archive_status: BulkArchiveStatus,
        output_path: Option<String>,
        error: Option<String>,
    ) {
        for job in &mut self.jobs {
            let Some(archive) = &mut job.bulk_archive else {
                continue;
            };
            if archive.id != archive_id {
                continue;
            }

            archive.archive_status = archive_status;
            archive.output_path = output_path.clone();
            archive.error = error.clone();
        }
    }
}

pub(super) fn apply_download_filename(job: &mut DownloadJob, filename: &str) {
    let filename = filename.trim();
    if !filename.is_empty() {
        job.filename = filename.to_string();
    }
}

pub(super) fn apply_preflight_metadata_to_job(
    job: &mut DownloadJob,
    total_bytes: Option<u64>,
    resume_support: ResumeSupport,
    filename: Option<String>,
) {
    if let Some(filename) = filename {
        apply_download_filename(job, &filename);
    }

    if let Some(total_bytes) = total_bytes {
        job.total_bytes = total_bytes.max(job.downloaded_bytes);
        job.progress = if job.total_bytes == 0 {
            0.0
        } else {
            (job.downloaded_bytes as f64 / job.total_bytes as f64 * 100.0).clamp(0.0, 100.0)
        };
    }

    if resume_support != ResumeSupport::Unknown {
        job.resume_support = resume_support;
    }
}
