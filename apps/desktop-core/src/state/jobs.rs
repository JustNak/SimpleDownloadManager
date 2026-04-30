use super::*;

impl SharedState {
    pub async fn pause_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            let event_message = {
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
                    Some(format!("Paused {}", job.filename))
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
            let event_message = {
                let job = find_job_mut(&mut state.jobs, id)?;
                job.state = JobState::Canceled;
                job.progress = 0.0;
                job.total_bytes = 0;
                job.downloaded_bytes = 0;
                job.speed = 0;
                job.eta = 0;
                job.error = None;
                job.failure_category = None;
                job.retry_attempts = 0;
                reset_integrity_for_retry(job);
                format!("Canceled {}", job.filename)
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
            state
                .jobs
                .retain(|job| !matches!(job.state, JobState::Completed | JobState::Canceled));
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
            state.active_workers.remove(id);
            state.jobs.remove(job_index);
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
            self.wait_for_paused_torrent_delete_release(id).await?;
            let (target_path, temp_path) = {
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
                )
            };

            remove_path_if_exists(&target_path).map_err(internal_error)?;
            if temp_path != target_path {
                remove_path_if_exists(&temp_path).map_err(internal_error)?;
            }
        }

        self.remove_job(id).await
    }

    async fn wait_for_paused_torrent_delete_release(&self, id: &str) -> Result<(), BackendError> {
        let should_wait = {
            let state = self.inner.read().await;
            let job = state
                .jobs
                .iter()
                .find(|job| job.id == id)
                .ok_or_else(|| BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Job not found.".into(),
                })?;

            job.transfer_kind == TransferKind::Torrent
                && job.state == JobState::Paused
                && state.active_workers.contains(id)
        };

        if should_wait {
            self.wait_for_external_use_release(
                id,
                EXTERNAL_USE_HANDLE_RELEASE_TIMEOUT,
                EXTERNAL_USE_HANDLE_RELEASE_POLL,
            )
            .await?;
        }

        Ok(())
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
