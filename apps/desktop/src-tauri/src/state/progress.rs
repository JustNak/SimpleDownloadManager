use super::*;

impl SharedState {
    pub async fn sync_downloaded_bytes(
        &self,
        id: &str,
        downloaded_bytes: u64,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            {
                let Some(job) = state.job_mut(id) else {
                    return Err("Job not found.".into());
                };

                job.downloaded_bytes = downloaded_bytes;
                if job.total_bytes > 0 {
                    job.progress = (downloaded_bytes as f64 / job.total_bytes as f64 * 100.0)
                        .clamp(0.0, 100.0);
                } else {
                    job.progress = 0.0;
                }
                apply_segment_counts(job, None);
            }
            state.update_bulk_hoster_worker_health(id, downloaded_bytes, 0, Instant::now());
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn sync_downloaded_bytes_delta(
        &self,
        id: &str,
        downloaded_bytes: u64,
    ) -> Result<ProgressDelta, String> {
        let (delta, persisted) = {
            let mut state = self.inner.write().await;
            let updated_job = {
                let Some(job) = state.job_mut(id) else {
                    return Err("Job not found.".into());
                };

                job.downloaded_bytes = downloaded_bytes;
                if job.total_bytes > 0 {
                    job.progress = (downloaded_bytes as f64 / job.total_bytes as f64 * 100.0)
                        .clamp(0.0, 100.0);
                } else {
                    job.progress = 0.0;
                }
                apply_segment_counts(job, None);
                job.clone()
            };
            state.update_bulk_hoster_worker_health(id, downloaded_bytes, 0, Instant::now());
            (
                ProgressDelta {
                    job: updated_job,
                    settings: state.settings.clone(),
                },
                state.persisted(),
            )
        };

        persist_state_blocking(&self.storage_path, &persisted).await?;
        Ok(delta)
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
            {
                let Some(job) = state.job_mut(id) else {
                    return Err("Job not found.".into());
                };

                let preserve_interrupted_state =
                    should_preserve_worker_interrupted_state(job.state);
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
                apply_segment_counts(job, None);
                job.resume_support = resume_support;
            }
            state.mark_bulk_hoster_worker_transferring(id, downloaded_bytes, Instant::now());
            state.update_bulk_hoster_worker_health(id, downloaded_bytes, 0, Instant::now());
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn mark_segmented_job_downloading(
        &self,
        id: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        resume_support: ResumeSupport,
        filename: Option<String>,
        active_segments: u32,
        planned_segments: u32,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            {
                let Some(job) = state.job_mut(id) else {
                    return Err("Job not found.".into());
                };

                let preserve_interrupted_state =
                    should_preserve_worker_interrupted_state(job.state);
                if preserve_interrupted_state {
                    job.speed = 0;
                    job.eta = 0;
                    apply_segment_counts(job, None);
                } else {
                    job.state = JobState::Downloading;
                    job.error = None;
                    job.failure_category = None;
                    apply_segment_counts(job, Some((active_segments, planned_segments)));
                }
                if let Some(filename) = filename {
                    apply_download_filename(job, &filename);
                }
                apply_download_progress(job, downloaded_bytes, total_bytes);
                job.resume_support = resume_support;
            }
            state.mark_bulk_hoster_worker_transferring(id, downloaded_bytes, Instant::now());
            state.update_bulk_hoster_worker_health(id, downloaded_bytes, 0, Instant::now());
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn mark_bulk_hoster_resolving(&self, id: &str) {
        let mut state = self.inner.write().await;
        state.mark_bulk_hoster_worker_resolving(id, Instant::now());
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
        let (snapshot, persisted, diagnostic_events) = {
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
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
    }

    pub async fn defer_active_job(
        &self,
        id: &str,
        reason: String,
        delay: Duration,
    ) -> Result<DesktopSnapshot, String> {
        let delay = delay.min(DOWNLOAD_ADMISSION_DEFER_MAX);
        let now = Instant::now();
        let until = now + delay;
        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            if state.job(id).is_none() {
                return Err("Job not found.".into());
            }
            let active_worker_count = state.active_workers.len();
            state.remove_active_worker(id);
            let event_message = {
                let job = state
                    .job_mut(id)
                    .expect("job existence was checked before releasing worker");

                job.state = JobState::Queued;
                job.speed = 0;
                job.eta = 0;
                apply_segment_counts(job, None);
                job.error = None;
                job.failure_category = None;
                format!(
                    "Deferred {} for {} seconds: {reason} (active workers before release: {active_worker_count})",
                    job.filename,
                    delay.as_secs().max(1)
                )
            };
            state.download_admission_defers.insert(
                id.to_string(),
                DownloadAdmissionDefer {
                    until,
                    reason: reason.clone(),
                },
            );
            state.push_diagnostic_event(
                DiagnosticLevel::Warning,
                "download".into(),
                event_message,
                Some(id.into()),
            );
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
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
        let (_, snapshot) = self
            .update_job_progress_with_segments_result(
                id,
                downloaded_bytes,
                total_bytes,
                speed,
                None,
                persist,
                true,
            )
            .await?;
        snapshot.ok_or_else(|| "Progress update did not produce a snapshot.".into())
    }

    pub async fn update_job_progress_delta(
        &self,
        id: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        speed: u64,
        persist: bool,
    ) -> Result<ProgressDelta, String> {
        let (delta, _) = self
            .update_job_progress_with_segments_result(
                id,
                downloaded_bytes,
                total_bytes,
                speed,
                None,
                persist,
                false,
            )
            .await?;
        Ok(delta)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_segmented_job_progress(
        &self,
        id: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        speed: u64,
        active_segments: u32,
        planned_segments: u32,
        persist: bool,
    ) -> Result<DesktopSnapshot, String> {
        let (_, snapshot) = self
            .update_job_progress_with_segments_result(
                id,
                downloaded_bytes,
                total_bytes,
                speed,
                Some((active_segments, planned_segments)),
                persist,
                true,
            )
            .await?;
        snapshot.ok_or_else(|| "Progress update did not produce a snapshot.".into())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_segmented_job_progress_delta(
        &self,
        id: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        speed: u64,
        active_segments: u32,
        planned_segments: u32,
        persist: bool,
    ) -> Result<ProgressDelta, String> {
        let (delta, _) = self
            .update_job_progress_with_segments_result(
                id,
                downloaded_bytes,
                total_bytes,
                speed,
                Some((active_segments, planned_segments)),
                persist,
                false,
            )
            .await?;
        Ok(delta)
    }

    #[allow(clippy::too_many_arguments)]
    async fn update_job_progress_with_segments_result(
        &self,
        id: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        speed: u64,
        segment_counts: Option<(u32, u32)>,
        persist: bool,
        include_snapshot: bool,
    ) -> Result<(ProgressDelta, Option<DesktopSnapshot>), String> {
        let (delta, snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let updated_job = {
                let Some(job) = state.job_mut(id) else {
                    return Err("Job not found.".into());
                };

                let preserve_interrupted_state =
                    should_preserve_worker_interrupted_state(job.state);
                if !preserve_interrupted_state {
                    job.state = JobState::Downloading;
                }
                apply_download_progress(job, downloaded_bytes, total_bytes);

                if preserve_interrupted_state {
                    job.speed = 0;
                    job.eta = 0;
                    apply_segment_counts(job, None);
                } else {
                    job.speed = speed;
                    let remaining = job.total_bytes.saturating_sub(job.downloaded_bytes);
                    job.eta = if speed == 0 {
                        0
                    } else {
                        ((remaining as f64) / (speed as f64)).ceil() as u64
                    };
                    apply_segment_counts(job, segment_counts);
                }
                job.clone()
            };
            state.update_bulk_hoster_worker_health(id, downloaded_bytes, speed, Instant::now());

            let persisted = persist
                .then(|| state.should_persist_progress_at(Instant::now()))
                .filter(|should_persist| *should_persist)
                .map(|_| state.persisted());
            let snapshot = include_snapshot.then(|| state.snapshot());
            (
                ProgressDelta {
                    job: updated_job,
                    settings: state.settings.clone(),
                },
                snapshot,
                persisted,
            )
        };

        if let Some(persisted) = persisted {
            persist_state_blocking(&self.storage_path, &persisted).await?;
        }
        Ok((delta, snapshot))
    }

    pub async fn job_snapshot(&self, id: &str) -> Option<DownloadJob> {
        let state = self.inner.read().await;
        state.job(id).cloned()
    }

    pub async fn progress_job_snapshot_parts(&self, id: &str) -> (Option<DownloadJob>, Settings) {
        let state = self.inner.read().await;
        (state.job(id).cloned(), state.settings.clone())
    }

    pub async fn batch_progress_snapshot_parts(
        &self,
        job_ids: &[String],
    ) -> (Vec<DownloadJob>, Settings) {
        let state = self.inner.read().await;
        (
            batch_progress_jobs_for_state(&state, job_ids),
            state.settings.clone(),
        )
    }

    pub async fn batch_progress_jobs(&self, job_ids: &[String]) -> Vec<DownloadJob> {
        let state = self.inner.read().await;
        batch_progress_jobs_for_state(&state, job_ids)
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
        let (snapshot, persisted, diagnostic_events) = {
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
            apply_segment_counts(job, None);
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
            state.remove_active_worker(id);
            state.push_diagnostic_event(event.0, "download".into(), event.1, Some(id.into()));
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
    }

    #[allow(clippy::too_many_arguments)]
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
            let output_path = archive
                .output_path
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| bulk_output_path_from_settings(&state.settings, &archive.name));

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
        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            state.remove_active_worker(id);
            state.external_reseed_jobs.remove(id);
            let event_message = {
                let Some(job) = state.job_mut(id) else {
                    return Err("Job not found.".into());
                };

                job.state = JobState::Failed;
                job.speed = 0;
                job.eta = 0;
                apply_segment_counts(job, None);
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
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
    }

    pub async fn pause_job_after_retry_exhaustion(
        &self,
        id: &str,
        message: impl Into<String>,
        failure_category: FailureCategory,
    ) -> Result<DesktopSnapshot, String> {
        let message = message.into();
        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            state.remove_active_worker(id);
            state.external_reseed_jobs.remove(id);
            let event_message = {
                let Some(job) = state.job_mut(id) else {
                    return Err("Job not found.".into());
                };

                job.state = JobState::Paused;
                job.speed = 0;
                job.eta = 0;
                apply_segment_counts(job, None);
                job.error = Some(message.clone());
                job.failure_category = Some(failure_category);
                format!("Paused {} after retry exhaustion: {message}", job.filename)
            };
            state.push_diagnostic_event(
                DiagnosticLevel::Warning,
                "download".into(),
                event_message,
                Some(id.into()),
            );
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
    }

    pub async fn has_recoverable_partial_download(&self, id: &str) -> bool {
        let partial_path = {
            let state = self.inner.read().await;
            let Some(job) = state.job(id) else {
                return false;
            };
            if job.transfer_kind != TransferKind::Http
                || job.resume_support == ResumeSupport::Unsupported
                || matches!(job.state, JobState::Completed | JobState::Canceled)
            {
                return false;
            }
            if job.downloaded_bytes > 0 {
                return true;
            }
            PathBuf::from(&job.temp_path)
        };

        tokio::fs::metadata(partial_path)
            .await
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false)
    }

    pub async fn finish_interrupted_job(&self, id: &str) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.remove_active_worker(id);
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
            apply_segment_counts(job, None);
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
            if job.removal_state == Some(RemovalState::Removing) {
                return Err(BackendError {
                    code: "INTERNAL_ERROR",
                    message: "This download is being removed from disk.".into(),
                });
            }

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
        let (job_state, transfer_kind, target_path) = {
            let state = self.inner.read().await;
            let job = state.job(id).ok_or_else(|| BackendError {
                code: "INTERNAL_ERROR",
                message: "Job not found.".into(),
            })?;
            if job.removal_state == Some(RemovalState::Removing) {
                return Err(BackendError {
                    code: "INTERNAL_ERROR",
                    message: "This download is being removed from disk.".into(),
                });
            }

            (
                job.state,
                job.transfer_kind,
                PathBuf::from(&job.target_path),
            )
        };

        let finalized_artifact_state = matches!(job_state, JobState::Completed | JobState::Seeding)
            || (transfer_kind == TransferKind::Torrent
                && job_state == JobState::Paused
                && target_path.exists());

        if finalized_artifact_state && target_path.exists() {
            return Ok(target_path);
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
                .filter(|job| job.removal_state != Some(RemovalState::Removing))
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
    #[allow(clippy::too_many_arguments)]
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

fn batch_progress_jobs_for_state(state: &RuntimeState, job_ids: &[String]) -> Vec<DownloadJob> {
    let mut seen = HashSet::new();
    let mut indexes = job_ids
        .iter()
        .filter_map(|id| {
            if !seen.insert(id.as_str()) {
                return None;
            }
            state.job_index(id)
        })
        .collect::<Vec<_>>();
    indexes.sort_unstable();
    indexes.dedup();
    indexes
        .into_iter()
        .filter_map(|index| state.jobs.get(index).cloned())
        .collect()
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

fn apply_download_progress(job: &mut DownloadJob, downloaded_bytes: u64, total_bytes: Option<u64>) {
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

fn apply_segment_counts(job: &mut DownloadJob, segment_counts: Option<(u32, u32)>) {
    match segment_counts {
        Some((active_segments, planned_segments)) if planned_segments > 0 => {
            job.active_segments = Some(active_segments.min(planned_segments));
            job.planned_segments = Some(planned_segments);
        }
        _ => {
            job.active_segments = None;
            job.planned_segments = None;
        }
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
