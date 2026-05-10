use super::*;

impl SharedState {
    pub async fn pause_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            let (event_message, clear_hoster_health) = {
                let job = find_job_mut(&mut state.jobs, id)?;
                if matches!(
                    job.state,
                    JobState::Queued
                        | JobState::Starting
                        | JobState::Downloading
                        | JobState::Seeding
                ) {
                    job.state = JobState::Paused;
                    job.speed = 0;
                    job.eta = 0;
                    (
                        Some(format!("Paused {}", job.filename)),
                        is_protected_bulk_hoster_job(job),
                    )
                } else {
                    (None, false)
                }
            };
            if clear_hoster_health {
                state.clear_bulk_hoster_worker_health(id);
            }
            if let Some(message) = event_message {
                state.push_diagnostic_event(
                    DiagnosticLevel::Info,
                    "download".into(),
                    message,
                    Some(id.into()),
                );
            }
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.clear_handoff_auth(id).await;
        Ok(snapshot)
    }

    pub async fn resume_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            let event_message = {
                let job = find_job_mut(&mut state.jobs, id)?;
                if matches!(
                    job.state,
                    JobState::Paused | JobState::Failed | JobState::Canceled
                ) {
                    job.state = JobState::Queued;
                    job.error = None;
                    job.failure_category = None;
                    job.retry_attempts = 0;
                    job.auto_restart_attempts = 0;
                    job.speed = 0;
                    job.eta = 0;
                    reset_integrity_for_retry(job);
                    Some(format!("Resumed {}", job.filename))
                } else {
                    None
                }
            };
            if let Some(message) = event_message {
                state.push_diagnostic_event(
                    DiagnosticLevel::Info,
                    "download".into(),
                    message,
                    Some(id.into()),
                );
            }
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn pause_all_jobs(&self) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.clear();
            state.pause_all_jobs();
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn resume_all_jobs(&self) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.clear();
            state.resume_all_jobs();
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn cancel_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let temp_to_remove = {
            let state = self.inner.read().await;
            let job = state
                .jobs
                .iter()
                .find(|job| job.id == id)
                .ok_or_else(|| BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Job not found.".into(),
                })?;

            if state.active_workers.contains(id) {
                None
            } else {
                Some(PathBuf::from(&job.temp_path))
            }
        };

        if let Some(temp_path) = temp_to_remove {
            let _ = remove_path_if_exists(&temp_path);
        }

        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            let (event_message, clear_hoster_health) = {
                let job = find_job_mut(&mut state.jobs, id)?;
                let clear_hoster_health = is_protected_bulk_hoster_job(job);
                job.state = JobState::Canceled;
                job.progress = 0.0;
                job.total_bytes = 0;
                job.downloaded_bytes = 0;
                job.speed = 0;
                job.eta = 0;
                job.error = None;
                job.failure_category = None;
                job.retry_attempts = 0;
                job.auto_restart_attempts = 0;
                reset_integrity_for_retry(job);
                (format!("Canceled {}", job.filename), clear_hoster_health)
            };
            if clear_hoster_health {
                state.clear_bulk_hoster_worker_health(id);
            }
            state.push_diagnostic_event(
                DiagnosticLevel::Info,
                "download".into(),
                event_message,
                Some(id.into()),
            );
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.clear_handoff_auth(id).await;
        Ok(snapshot)
    }

    pub async fn retry_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            let event_message = {
                let job = find_job_mut(&mut state.jobs, id)?;
                job.state = JobState::Queued;
                job.speed = 0;
                job.eta = 0;
                job.error = None;
                job.failure_category = None;
                job.retry_attempts = 0;
                job.auto_restart_attempts = 0;
                reset_integrity_for_retry(job);
                format!("Retry queued for {}", job.filename)
            };
            state.push_diagnostic_event(
                DiagnosticLevel::Info,
                "download".into(),
                event_message,
                Some(id.into()),
            );
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn restart_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            if state.active_workers.contains(id) {
                return Err(BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Pause or cancel the active transfer before restarting it.".into(),
                });
            }

            let event_message = {
                let job = find_job_mut(&mut state.jobs, id)?;
                remove_file_if_exists(Path::new(&job.temp_path)).map_err(internal_error)?;
                reset_job_for_restart(job);
                format!("Restart queued for {}", job.filename)
            };
            state.push_diagnostic_event(
                DiagnosticLevel::Info,
                "download".into(),
                event_message,
                Some(id.into()),
            );
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn bulk_member_auto_restart_candidate(
        &self,
        id: &str,
        failure_category: FailureCategory,
    ) -> Result<Option<BulkMemberAutoRestartCandidate>, String> {
        let state = self.inner.read().await;
        let Some(job) = state.job(id) else {
            return Ok(None);
        };
        let max_attempts = max_auto_retry_attempts_for_job(&state.settings, job);

        if max_attempts == 0
            || job.auto_restart_attempts >= max_attempts
            || !bulk_member_auto_restart_failure_category(failure_category)
            || !is_pending_http_bulk_member(job)
        {
            return Ok(None);
        }

        Ok(Some(BulkMemberAutoRestartCandidate {
            resolved_from_url: job.resolved_from_url.clone(),
            attempt: job.auto_restart_attempts.saturating_add(1),
            max_attempts,
        }))
    }

    pub async fn bulk_member_slow_recovery_state(
        &self,
        id: &str,
    ) -> Result<Option<BulkMemberSlowRecoveryState>, String> {
        let state = self.inner.read().await;
        let Some(job) = state.job(id) else {
            return Ok(None);
        };

        if !is_pending_http_bulk_member(job) {
            return Ok(None);
        }

        Ok(Some(BulkMemberSlowRecoveryState {
            retry_attempts: job.retry_attempts,
            max_retry_attempts: max_auto_retry_attempts_for_job(&state.settings, job),
        }))
    }

    pub async fn auto_restart_bulk_member(
        &self,
        id: &str,
        resolved_url: String,
        attempt: u32,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            state.remove_active_worker(id);
            let event_message = {
                let job = find_job_mut(&mut state.jobs, id).map_err(|error| error.message)?;
                remove_file_if_exists(Path::new(&job.temp_path))?;
                reset_job_for_restart(job);
                job.url = resolved_url;
                job.auto_restart_attempts = attempt;
                format!("Auto-restart queued for {}", job.filename)
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

    pub async fn bulk_member_retry_candidates(
        &self,
        archive_id: &str,
    ) -> Result<Vec<BulkMemberRetryCandidate>, String> {
        let state = self.inner.read().await;
        let members = state
            .jobs
            .iter()
            .filter(|job| {
                job.bulk_archive
                    .as_ref()
                    .is_some_and(|archive| archive.id == archive_id)
            })
            .collect::<Vec<_>>();

        if members.is_empty() {
            return Err("Bulk archive was not found.".into());
        }
        if members.iter().any(|job| {
            job.bulk_archive
                .as_ref()
                .is_some_and(|archive| archive.archive_status != BulkArchiveStatus::Pending)
        }) {
            return Err(
                "Bulk member retry is only available while the bulk archive is pending.".into(),
            );
        }

        let candidates = members
            .into_iter()
            .filter(|job| job.transfer_kind == TransferKind::Http && job.state == JobState::Failed)
            .map(|job| {
                let resolved_from_url = job
                    .resolved_from_url
                    .clone()
                    .filter(|url| !url.trim().is_empty());
                let source_url = resolved_from_url.clone().unwrap_or_else(|| job.url.clone());
                BulkMemberRetryCandidate {
                    id: job.id.clone(),
                    source_url,
                    resolved_from_url,
                }
            })
            .collect::<Vec<_>>();

        if candidates.is_empty() {
            return Err("No failed bulk member downloads are available to retry.".into());
        }

        Ok(candidates)
    }

    pub async fn retry_bulk_member(
        &self,
        id: &str,
        resolved_url: String,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            state.remove_active_worker(id);
            let event_message = {
                let job = find_job_mut(&mut state.jobs, id).map_err(|error| error.message)?;
                if job.transfer_kind != TransferKind::Http
                    || job.state != JobState::Failed
                    || !job
                        .bulk_archive
                        .as_ref()
                        .is_some_and(|archive| archive.archive_status == BulkArchiveStatus::Pending)
                {
                    return Err("Only failed pending HTTP bulk members can be retried.".into());
                }

                remove_file_if_exists(Path::new(&job.temp_path))?;
                reset_job_for_restart(job);
                job.url = resolved_url;
                format!("Retry queued for bulk member {}", job.filename)
            };
            state.push_diagnostic_event(
                DiagnosticLevel::Info,
                "download".into(),
                event_message,
                Some(id.into()),
            );
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn set_hoster_preflight(
        &self,
        id: &str,
        preflight: HosterPreflightInfo,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let job = find_job_mut(&mut state.jobs, id).map_err(|error| error.message)?;
            job.hoster_preflight = Some(preflight);
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn torrent_restart_cleanup_info(
        &self,
        id: &str,
    ) -> Result<Option<TorrentInfo>, BackendError> {
        let state = self.inner.read().await;
        if state.active_workers.contains(id) {
            return Err(BackendError {
                code: "INTERNAL_ERROR",
                message: "Pause or cancel the active transfer before restarting it.".into(),
            });
        }

        let job = state
            .jobs
            .iter()
            .find(|job| job.id == id)
            .ok_or_else(|| BackendError {
                code: "INTERNAL_ERROR",
                message: "Job not found.".into(),
            })?;
        if job.transfer_kind != TransferKind::Torrent {
            return Ok(None);
        }

        Ok(job.torrent.clone())
    }

    pub async fn retry_failed_jobs(&self) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;

            for job in &mut state.jobs {
                if job.state == JobState::Failed {
                    job.state = JobState::Queued;
                    job.speed = 0;
                    job.eta = 0;
                    job.error = None;
                    job.failure_category = None;
                    job.retry_attempts = 0;
                    job.auto_restart_attempts = 0;
                    reset_integrity_for_retry(job);
                }
            }
            state.push_diagnostic_event(
                DiagnosticLevel::Info,
                "download".into(),
                "Retry queued for failed downloads".into(),
                None,
            );

            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn clear_completed_jobs(&self) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.retain_jobs(|job| !matches!(job.state, JobState::Completed | JobState::Canceled));
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn remove_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted, paths_to_cleanup) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            let job_index = state
                .jobs
                .iter()
                .position(|job| job.id == id)
                .ok_or_else(|| BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Job not found.".into(),
                })?;
            let is_active_worker = state.active_workers.contains(id);
            let job = &state.jobs[job_index];

            if job_blocks_removal(job, is_active_worker) {
                return Err(BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Pause or cancel the active transfer before removing it.".into(),
                });
            }

            let paths_to_cleanup = (PathBuf::from(&job.temp_path), job.state);
            let removed_canceled_torrent =
                job.transfer_kind == TransferKind::Torrent && job.state == JobState::Canceled;
            let filename = job.filename.clone();
            state.remove_active_worker(id);
            state.remove_job_at_index(job_index);
            if removed_canceled_torrent {
                state.push_diagnostic_event(
                    DiagnosticLevel::Info,
                    "torrent".into(),
                    format!("Removed canceled torrent {filename}"),
                    Some(id.into()),
                );
            }

            (state.snapshot(), state.persisted(), paths_to_cleanup)
        };

        let (temp_path, job_state) = paths_to_cleanup;
        if job_state != JobState::Completed {
            let _ = remove_path_if_exists(&temp_path);
        }

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn delete_job(
        &self,
        id: &str,
        delete_from_disk: bool,
    ) -> Result<DesktopSnapshot, BackendError> {
        if delete_from_disk {
            self.wait_for_disk_delete_release(id).await?;
            let (target_path, temp_path, bulk_archive_output_path) = {
                let state = self.inner.read().await;
                let job =
                    state
                        .jobs
                        .iter()
                        .find(|job| job.id == id)
                        .ok_or_else(|| BackendError {
                            code: "INTERNAL_ERROR",
                            message: "Job not found.".into(),
                        })?;

                if job_blocks_removal(job, state.active_workers.contains(id)) {
                    return Err(BackendError {
                        code: "INTERNAL_ERROR",
                        message:
                            "Pause or cancel the active transfer before deleting files from disk."
                                .into(),
                    });
                }

                (
                    PathBuf::from(&job.target_path),
                    PathBuf::from(&job.temp_path),
                    job.bulk_archive.as_ref().and_then(|archive| {
                        if archive.archive_status == BulkArchiveStatus::Completed {
                            archive.output_path.as_ref().map(PathBuf::from)
                        } else {
                            None
                        }
                    }),
                )
            };

            remove_path_if_exists(&target_path).map_err(internal_error)?;
            if temp_path != target_path {
                remove_path_if_exists(&temp_path).map_err(internal_error)?;
            }
            if let Some(archive_path) = bulk_archive_output_path {
                if archive_path != target_path && archive_path != temp_path {
                    remove_path_if_exists(&archive_path).map_err(internal_error)?;
                }
            }
        }

        self.remove_job(id).await
    }

    async fn wait_for_disk_delete_release(&self, id: &str) -> Result<(), BackendError> {
        let (wait_for_canceled_worker, wait_for_paused_torrent) = {
            let state = self.inner.read().await;
            let job = state
                .jobs
                .iter()
                .find(|job| job.id == id)
                .ok_or_else(|| BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Job not found.".into(),
                })?;
            let active = state.active_workers.contains(id);

            (
                job.transfer_kind != TransferKind::Torrent
                    && job.state == JobState::Canceled
                    && active,
                job.transfer_kind == TransferKind::Torrent
                    && job.state == JobState::Paused
                    && active,
            )
        };

        if wait_for_canceled_worker {
            self.wait_for_active_worker_release(
                id,
                "The download was canceled, but its file handles are still being released. Try again in a moment.",
            )
            .await?;
        }

        if wait_for_paused_torrent {
            self.wait_for_external_use_release(
                id,
                EXTERNAL_USE_HANDLE_RELEASE_TIMEOUT,
                EXTERNAL_USE_HANDLE_RELEASE_POLL,
            )
            .await?;
        }

        Ok(())
    }

    async fn wait_for_active_worker_release(
        &self,
        id: &str,
        timeout_message: &str,
    ) -> Result<(), BackendError> {
        let started_at = Instant::now();
        let poll_interval = EXTERNAL_USE_HANDLE_RELEASE_POLL;

        loop {
            let ready = {
                let state = self.inner.read().await;
                if !state.jobs.iter().any(|job| job.id == id) {
                    return Err(BackendError {
                        code: "INTERNAL_ERROR",
                        message: "Job not found.".into(),
                    });
                }

                !state.active_workers.contains(id)
            };

            if ready {
                return Ok(());
            }

            let elapsed = started_at.elapsed();
            if elapsed >= EXTERNAL_USE_HANDLE_RELEASE_TIMEOUT {
                return Err(BackendError {
                    code: "INTERNAL_ERROR",
                    message: timeout_message.into(),
                });
            }

            tokio::time::sleep(
                poll_interval.min(EXTERNAL_USE_HANDLE_RELEASE_TIMEOUT.saturating_sub(elapsed)),
            )
            .await;
        }
    }

    pub async fn rename_job(
        &self,
        id: &str,
        filename: &str,
    ) -> Result<DesktopSnapshot, BackendError> {
        let filename = sanitize_filename(filename);
        if filename.trim().is_empty() {
            return Err(BackendError {
                code: "INTERNAL_ERROR",
                message: "Filename cannot be empty.".into(),
            });
        }

        let (current_target_path, current_temp_path, next_target_path, next_temp_path) = {
            let state = self.inner.read().await;
            let job = state
                .jobs
                .iter()
                .find(|job| job.id == id)
                .ok_or_else(|| BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Job not found.".into(),
                })?;

            if state.active_workers.contains(id)
                || matches!(
                    job.state,
                    JobState::Starting | JobState::Downloading | JobState::Seeding
                )
            {
                return Err(BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Pause or cancel the active transfer before renaming it.".into(),
                });
            }

            let current_target_path = PathBuf::from(&job.target_path);
            let current_temp_path = PathBuf::from(&job.temp_path);
            let default_directory = state.settings.download_directory.clone();
            let parent = current_target_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(default_directory));
            let next_target_path = parent.join(&filename);
            let next_temp_path = PathBuf::from(format!("{}.part", next_target_path.display()));

            if next_target_path != current_target_path && next_target_path.exists() {
                return Err(BackendError {
                    code: "DESTINATION_INVALID",
                    message: format!("A file already exists at {}.", next_target_path.display()),
                });
            }

            (
                current_target_path,
                current_temp_path,
                next_target_path,
                next_temp_path,
            )
        };

        if current_target_path.is_file() && current_target_path != next_target_path {
            std::fs::rename(&current_target_path, &next_target_path).map_err(|error| {
                internal_error(format!("Could not rename downloaded file: {error}"))
            })?;
        } else if current_temp_path.is_file() && current_temp_path != next_temp_path {
            std::fs::rename(&current_temp_path, &next_temp_path).map_err(|error| {
                internal_error(format!("Could not rename partial download file: {error}"))
            })?;
        }

        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err(BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Job not found.".into(),
                });
            };

            job.filename = filename;
            job.target_path = next_target_path.display().to_string();
            job.temp_path = next_temp_path.display().to_string();
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }
}

pub(super) fn max_auto_retry_attempts_for_job(settings: &Settings, job: &DownloadJob) -> u32 {
    if job.bulk_archive.is_some() && settings.bulk.auto_retry_override_enabled {
        settings.bulk.auto_retry_attempts.min(10)
    } else {
        settings.auto_retry_attempts.min(10)
    }
}

pub(super) fn find_job_mut<'a>(
    jobs: &'a mut [DownloadJob],
    id: &str,
) -> Result<&'a mut DownloadJob, BackendError> {
    jobs.iter_mut()
        .find(|job| job.id == id)
        .ok_or_else(|| BackendError {
            code: "INTERNAL_ERROR",
            message: "Job not found.".into(),
        })
}

pub(super) fn reset_job_for_restart(job: &mut DownloadJob) {
    job.state = JobState::Queued;
    job.progress = 0.0;
    job.total_bytes = 0;
    job.downloaded_bytes = 0;
    job.speed = 0;
    job.eta = 0;
    job.error = None;
    job.failure_category = None;
    job.resume_support = ResumeSupport::Unknown;
    job.retry_attempts = 0;
    job.auto_restart_attempts = 0;
    reset_integrity_for_retry(job);
    if job.transfer_kind == TransferKind::Torrent {
        job.torrent = Some(TorrentInfo::default());
    }
}

pub(super) fn reset_integrity_for_retry(job: &mut DownloadJob) {
    if let Some(check) = &mut job.integrity_check {
        check.actual = None;
        check.status = IntegrityStatus::Pending;
    }
}

pub(super) fn bulk_member_auto_restart_failure_category(failure_category: FailureCategory) -> bool {
    matches!(
        failure_category,
        FailureCategory::Network
            | FailureCategory::Server
            | FailureCategory::Http
            | FailureCategory::Resume
    )
}

pub(super) fn is_pending_http_bulk_member(job: &DownloadJob) -> bool {
    job.transfer_kind == TransferKind::Http
        && job
            .bulk_archive
            .as_ref()
            .is_some_and(|archive| archive.archive_status == BulkArchiveStatus::Pending)
}

pub(super) fn job_blocks_removal(job: &DownloadJob, is_active_worker: bool) -> bool {
    if job.state == JobState::Canceled {
        return false;
    }

    is_active_worker
        || matches!(
            job.state,
            JobState::Starting | JobState::Downloading | JobState::Seeding
        )
}
