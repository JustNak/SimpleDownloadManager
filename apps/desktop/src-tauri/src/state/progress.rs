use super::*;

impl SharedState {
    pub async fn sync_downloaded_bytes(
        &self,
        id: &str,
        downloaded_bytes: u64,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.job_mut(id) else {
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
            let Some(job) = state.job_mut(id) else {
                return Err("Job not found.".into());
            };

            let preserve_interrupted_state = should_preserve_worker_interrupted_state(job.state);
            if preserve_interrupted_state {
                job.speed = 0;
                job.eta = 0;
            } else {
                job.state = JobState::Downloading;
                job.error = None;
                job.failure_category = None;
            }
            if let Some(filename) = filename {
                apply_download_filename(job, &filename);
            }
            apply_download_progress(job, downloaded_bytes, total_bytes);
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
            let Some(job) = state.job_mut(id) else {
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
                let Some(job) = state.job_mut(id) else {
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
            let Some(job) = state.job_mut(id) else {
                return Err("Job not found.".into());
            };

            let preserve_interrupted_state = should_preserve_worker_interrupted_state(job.state);
            if !preserve_interrupted_state {
                job.state = JobState::Downloading;
            }
            apply_download_progress(job, downloaded_bytes, total_bytes);

            if preserve_interrupted_state {
                job.speed = 0;
                job.eta = 0;
            } else {
                job.speed = speed;
                let remaining = job.total_bytes.saturating_sub(job.downloaded_bytes);
                job.eta = if speed == 0 {
                    0
                } else {
                    ((remaining as f64) / (speed as f64)).ceil() as u64
                };
            }

            let persisted = persist
                .then(|| state.should_persist_progress_at(Instant::now()))
                .filter(|should_persist| *should_persist)
                .map(|_| state.persisted());
            (state.snapshot(), persisted)
        };

        if let Some(persisted) = persisted {
            persist_state(&self.storage_path, &persisted)?;
        }
        Ok(snapshot)
    }

    pub async fn job_requires_sha256(&self, id: &str) -> bool {
        let state = self.inner.read().await;
        state
            .job(id)
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
            let Some(job) = state.job_mut(id) else {
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
        requires_extraction: Option<bool>,
        output_path: Option<String>,
        error: Option<String>,
        warning: Option<String>,
        finalize_mode: Option<BulkFinalizeMode>,
        finalize_total_bytes: Option<u64>,
        finalize_processed_bytes: Option<u64>,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.mark_bulk_archive_status_in_memory(
                archive_id,
                archive_status,
                requires_extraction,
                output_path,
                error,
                warning,
                finalize_mode,
                finalize_total_bytes,
                finalize_processed_bytes,
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
        let Some((ready, persisted)) = ({
            let mut state = self.inner.write().await;
            let Some(job) = state.job(id) else {
                return Ok(None);
            };
            let Some(archive) = job.bulk_archive.clone() else {
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

            let output_kind = BulkArchiveOutputKind::Folder;
            let output_path = bulk_output_path_from_settings(&state.settings, &archive.name);

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

            let ready = BulkArchiveReady {
                archive_id: archive.id.clone(),
                output_kind,
                output_path,
                entries,
            };
            state.mark_bulk_archive_status_in_memory(
                &archive.id,
                BulkArchiveStatus::CreatingFolder,
                None,
                Some(ready.output_path.display().to_string()),
                None,
                None,
                None,
                None,
                Some(0),
            );
            Some((ready, state.persisted()))
        }) else {
            return Ok(None);
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(Some(ready))
    }

    pub async fn bulk_archive_ready_for_retry(
        &self,
        archive_id: &str,
    ) -> Result<BulkArchiveReady, String> {
        let (ready, persisted) = {
            let mut state = self.inner.write().await;
            let archive = state
                .jobs
                .iter()
                .filter_map(|job| job.bulk_archive.as_ref())
                .find(|archive| archive.id == archive_id)
                .cloned()
                .ok_or_else(|| "Bulk archive was not found.".to_string())?;

            if archive.archive_status != BulkArchiveStatus::Failed {
                return Err(match archive.archive_status {
                    BulkArchiveStatus::Completed => "Bulk archive is already completed.".into(),
                    BulkArchiveStatus::Pending => "Bulk archive is not ready to retry yet.".into(),
                    BulkArchiveStatus::Extracting
                    | BulkArchiveStatus::Combining
                    | BulkArchiveStatus::CreatingFolder
                    | BulkArchiveStatus::Compressing => {
                        "Bulk archive creation is already running.".into()
                    }
                    BulkArchiveStatus::Failed => unreachable!(),
                });
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

            if members.len() < 2 {
                return Err("Bulk archive retry needs at least two completed downloads.".into());
            }
            if members
                .iter()
                .any(|member| member.state != JobState::Completed)
            {
                return Err(
                    "Bulk archive retry can only run after every member download is completed."
                        .into(),
                );
            }

            let download_dir = PathBuf::from(&state.settings.download_directory);
            let output_kind = BulkArchiveOutputKind::Folder;
            let categorized_output_path =
                bulk_output_path_from_settings(&state.settings, &archive.name);
            let legacy_categorized_output_path =
                bulk_output_path(&download_dir, &archive.name, output_kind);
            let legacy_root_output_path = download_dir.join(&archive.name);
            let output_path = archive
                .output_path
                .as_ref()
                .map(PathBuf::from)
                .map(|stored_path| {
                    if stored_path == legacy_root_output_path
                        || stored_path == legacy_categorized_output_path
                    {
                        categorized_output_path.clone()
                    } else {
                        stored_path
                    }
                })
                .unwrap_or(categorized_output_path);
            let mut used_names = HashSet::new();
            let mut entries = Vec::with_capacity(members.len());
            for member in members {
                let source_path = PathBuf::from(&member.target_path);
                if !source_path.is_file() {
                    return Err(format!(
                        "Downloaded file is missing for {}: {}",
                        member.filename,
                        source_path.display()
                    ));
                }

                let archive_name = unique_archive_entry_name(&member.filename, &mut used_names);
                entries.push(BulkArchiveEntry {
                    source_path,
                    archive_name,
                });
            }

            let ready = BulkArchiveReady {
                archive_id: archive.id.clone(),
                output_kind,
                output_path,
                entries,
            };
            state.mark_bulk_archive_status_in_memory(
                &archive.id,
                BulkArchiveStatus::CreatingFolder,
                None,
                Some(ready.output_path.display().to_string()),
                None,
                None,
                None,
                None,
                Some(0),
            );
            (ready, state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(ready)
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
                let Some(job) = state.job_mut(id) else {
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
            let Some(job) = state.job_mut(id) else {
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
            let job = state.job(id).ok_or_else(|| BackendError {
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
            let job = state.job(id).ok_or_else(|| BackendError {
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

    pub async fn resolve_bulk_archive_openable_path(
        &self,
        archive_id: &str,
    ) -> Result<PathBuf, BackendError> {
        self.resolve_completed_bulk_archive_path(archive_id).await
    }

    pub async fn resolve_bulk_archive_revealable_path(
        &self,
        archive_id: &str,
    ) -> Result<PathBuf, BackendError> {
        self.resolve_completed_bulk_archive_path(archive_id).await
    }

    async fn resolve_completed_bulk_archive_path(
        &self,
        archive_id: &str,
    ) -> Result<PathBuf, BackendError> {
        let archive = {
            let state = self.inner.read().await;
            state
                .jobs
                .iter()
                .filter_map(|job| job.bulk_archive.as_ref())
                .find(|archive| archive.id == archive_id)
                .cloned()
        }
        .ok_or_else(|| BackendError {
            code: "INTERNAL_ERROR",
            message: "Bulk archive not found.".into(),
        })?;

        if archive.archive_status == BulkArchiveStatus::Failed {
            return Err(BackendError {
                code: "INTERNAL_ERROR",
                message: archive
                    .error
                    .map(|error| format!("Bulk archive failed: {error}"))
                    .unwrap_or_else(|| "Bulk archive failed.".into()),
            });
        }

        if archive.archive_status != BulkArchiveStatus::Completed {
            return Err(BackendError {
                code: "INTERNAL_ERROR",
                message: "Bulk archive is not ready yet.".into(),
            });
        }

        let Some(output_path) = archive.output_path.filter(|path| !path.trim().is_empty()) else {
            return Err(BackendError {
                code: "INTERNAL_ERROR",
                message: "Bulk archive is not ready yet.".into(),
            });
        };

        let path = PathBuf::from(output_path);
        if path.is_file() || path.is_dir() {
            return Ok(path);
        }

        Err(BackendError {
            code: "INTERNAL_ERROR",
            message: format!("Bulk archive is not available on disk: {}", path.display()),
        })
    }
}

impl RuntimeState {
    pub(super) fn mark_bulk_archive_status_in_memory(
        &mut self,
        archive_id: &str,
        archive_status: BulkArchiveStatus,
        requires_extraction: Option<bool>,
        output_path: Option<String>,
        error: Option<String>,
        warning: Option<String>,
        finalize_mode: Option<BulkFinalizeMode>,
        finalize_total_bytes: Option<u64>,
        finalize_processed_bytes: Option<u64>,
    ) {
        for job in &mut self.jobs {
            let Some(archive) = &mut job.bulk_archive else {
                continue;
            };
            if archive.id != archive_id {
                continue;
            }

            archive.archive_status = archive_status;
            if let Some(requires_extraction) = requires_extraction {
                archive.requires_extraction = Some(requires_extraction);
            }
            archive.output_path = output_path.clone();
            archive.error = error.clone();
            archive.warning = warning.clone();
            if let Some(finalize_mode) = finalize_mode {
                archive.finalize_mode = Some(finalize_mode);
            }
            if let Some(finalize_total_bytes) = finalize_total_bytes {
                archive.finalize_total_bytes = Some(finalize_total_bytes);
            }
            if let Some(finalize_processed_bytes) = finalize_processed_bytes {
                archive.finalize_processed_bytes = Some(finalize_processed_bytes);
            }
        }
    }
}

pub(super) fn apply_download_filename(job: &mut DownloadJob, filename: &str) {
    let filename = filename.trim();
    if !filename.is_empty() {
        job.filename = filename.to_string();
    }
}

fn should_preserve_worker_interrupted_state(state: JobState) -> bool {
    matches!(
        state,
        JobState::Paused | JobState::Canceled | JobState::Failed | JobState::Completed
    )
}

fn apply_download_progress(
    job: &mut DownloadJob,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) {
    job.downloaded_bytes = downloaded_bytes;
    if let Some(total_bytes) = total_bytes {
        job.total_bytes = total_bytes.max(downloaded_bytes);
    }
    job.progress = if job.total_bytes == 0 {
        0.0
    } else {
        (job.downloaded_bytes as f64 / job.total_bytes as f64 * 100.0).clamp(0.0, 100.0)
    };
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
