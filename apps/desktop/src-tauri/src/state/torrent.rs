use super::*;

pub(crate) fn should_stop_seeding(
    settings: &TorrentSettings,
    ratio: f64,
    elapsed_seconds: u64,
) -> bool {
    match settings.seed_mode {
        TorrentSeedMode::Forever => false,
        TorrentSeedMode::Ratio => ratio >= settings.seed_ratio_limit,
        TorrentSeedMode::Time => {
            elapsed_seconds >= u64::from(settings.seed_time_limit_minutes).saturating_mul(60)
        }
        TorrentSeedMode::RatioOrTime => {
            ratio >= settings.seed_ratio_limit
                || elapsed_seconds >= u64::from(settings.seed_time_limit_minutes).saturating_mul(60)
        }
    }
}

pub(super) fn cumulative_torrent_runtime_bytes(
    previous_total: u64,
    previous_runtime: Option<u64>,
    runtime_value: u64,
) -> u64 {
    match previous_runtime {
        Some(last_runtime) if runtime_value >= last_runtime => {
            previous_total.saturating_add(runtime_value - last_runtime)
        }
        Some(_) => previous_total.saturating_add(runtime_value),
        None if previous_total == 0 => runtime_value,
        None => previous_total,
    }
}

pub(super) fn is_external_reseed_file_access_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    [
        "sharing violation",
        "being used by another process",
        "being used by another program",
        "process cannot access the file",
        "another program is currently using this file",
        "access is denied",
        "permission denied",
        "os error 32",
        "os error 5",
        "failed to open",
        "could not open",
    ]
    .iter()
    .any(|pattern| normalized.contains(pattern))
}

impl SharedState {
    pub async fn begin_external_reseed(&self, id: &str) {
        let mut state = self.inner.write().await;
        if state.jobs.iter().any(|job| {
            job.id == id
                && job.transfer_kind == TransferKind::Torrent
                && job.state == JobState::Paused
        }) {
            state.external_reseed_jobs.insert(id.into());
        }
    }

    pub async fn queue_external_reseed_attempt(
        &self,
        id: &str,
    ) -> Result<ExternalReseedAttempt, String> {
        let queued = {
            let mut state = self.inner.write().await;
            if !state.external_reseed_jobs.contains(id) {
                return Ok(ExternalReseedAttempt::Stop);
            }

            let Some(job_index) = state.jobs.iter().position(|job| job.id == id) else {
                state.external_reseed_jobs.remove(id);
                return Ok(ExternalReseedAttempt::Stop);
            };

            let job_state = state.jobs[job_index].state;
            let active = state.active_workers.contains(id);
            match job_state {
                JobState::Paused if !active => {
                    let filename = {
                        let job = &mut state.jobs[job_index];
                        job.state = JobState::Queued;
                        job.speed = 0;
                        job.eta = 0;
                        job.error = None;
                        job.failure_category = None;
                        job.retry_attempts = 0;
                        reset_integrity_for_retry(job);
                        job.filename.clone()
                    };
                    state.push_diagnostic_event(
                        DiagnosticLevel::Info,
                        "torrent".into(),
                        format!("Queued automatic reseed for {filename}"),
                        Some(id.into()),
                    );
                    Some((state.snapshot(), state.persisted()))
                }
                JobState::Paused
                | JobState::Queued
                | JobState::Starting
                | JobState::Downloading => {
                    return Ok(ExternalReseedAttempt::Pending);
                }
                JobState::Seeding | JobState::Completed | JobState::Failed | JobState::Canceled => {
                    state.external_reseed_jobs.remove(id);
                    return Ok(ExternalReseedAttempt::Stop);
                }
            }
        };

        if let Some((snapshot, persisted)) = queued {
            persist_state(&self.storage_path, &persisted)?;
            return Ok(ExternalReseedAttempt::Queued(snapshot));
        }

        Ok(ExternalReseedAttempt::Stop)
    }

    pub async fn handle_external_reseed_failure(
        &self,
        id: &str,
        message: impl Into<String>,
        failure_category: FailureCategory,
    ) -> Result<Option<DesktopSnapshot>, String> {
        let message = message.into();
        if failure_category != FailureCategory::Torrent
            || !is_external_reseed_file_access_error(&message)
        {
            return Ok(None);
        }

        let handled = {
            let mut state = self.inner.write().await;
            if !state.external_reseed_jobs.contains(id) {
                return Ok(None);
            }

            state.active_workers.remove(id);
            let event_message = {
                let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                    state.external_reseed_jobs.remove(id);
                    return Ok(None);
                };

                job.state = JobState::Paused;
                job.speed = 0;
                job.eta = 0;
                job.error = Some(message.clone());
                job.failure_category = None;
                format!(
                    "Automatic reseed for {} is waiting for external file access to finish: {message}",
                    job.filename
                )
            };
            state.push_diagnostic_event(
                DiagnosticLevel::Warning,
                "torrent".into(),
                event_message,
                Some(id.into()),
            );
            Some((state.snapshot(), state.persisted()))
        };

        let Some((snapshot, persisted)) = handled else {
            return Ok(None);
        };
        persist_state(&self.storage_path, &persisted)?;
        Ok(Some(snapshot))
    }

    pub async fn handle_torrent_seeding_restore_failure(
        &self,
        id: &str,
        message: impl Into<String>,
        failure_category: FailureCategory,
    ) -> Result<Option<TorrentSeedingRestoreFailure>, String> {
        let message = message.into();
        let retry_reseed = failure_category == FailureCategory::Torrent
            && is_external_reseed_file_access_error(&message);
        let handled = {
            let mut state = self.inner.write().await;

            let filename = {
                let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                    return Err("Job not found.".into());
                };

                if job.transfer_kind != TransferKind::Torrent
                    || job
                        .torrent
                        .as_ref()
                        .and_then(|torrent| torrent.seeding_started_at)
                        .is_none()
                {
                    return Ok(None);
                }

                job.state = JobState::Paused;
                job.speed = 0;
                job.eta = 0;
                job.error = Some(message.clone());
                job.failure_category = Some(failure_category);
                job.filename.clone()
            };

            state.active_workers.remove(id);
            let event_message = if retry_reseed {
                state.external_reseed_jobs.insert(id.into());
                format!(
                    "Automatic seeding restore for {filename} is waiting for external file access to finish: {message}"
                )
            } else {
                state.external_reseed_jobs.remove(id);
                format!("Paused seeding restore for {filename}: {message}")
            };

            state.push_diagnostic_event(
                DiagnosticLevel::Warning,
                "torrent".into(),
                event_message,
                Some(id.into()),
            );

            Some((state.snapshot(), state.persisted()))
        };

        let Some((snapshot, persisted)) = handled else {
            return Ok(None);
        };
        persist_state(&self.storage_path, &persisted)?;
        Ok(Some(TorrentSeedingRestoreFailure {
            snapshot,
            retry_reseed,
        }))
    }

    #[cfg(test)]
    pub(super) async fn is_external_reseed_pending(&self, id: &str) -> bool {
        self.inner.read().await.external_reseed_jobs.contains(id)
    }

    pub async fn prepare_job_for_external_use(
        &self,
        id: &str,
    ) -> Result<ExternalUsePreparation, BackendError> {
        self.prepare_job_for_external_use_with_wait(
            id,
            EXTERNAL_USE_HANDLE_RELEASE_TIMEOUT,
            EXTERNAL_USE_HANDLE_RELEASE_POLL,
        )
        .await
    }

    pub async fn prepare_job_for_external_use_with_wait(
        &self,
        id: &str,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<ExternalUsePreparation, BackendError> {
        let paused = {
            let mut state = self.inner.write().await;
            let event_message = {
                let job = find_job_mut(&mut state.jobs, id)?;
                if job.transfer_kind == TransferKind::Torrent && job.state == JobState::Seeding {
                    job.state = JobState::Paused;
                    job.speed = 0;
                    job.eta = 0;
                    Some(format!("Paused {} for external file use", job.filename))
                } else {
                    None
                }
            };

            if let Some(message) = event_message {
                state.push_diagnostic_event(
                    DiagnosticLevel::Info,
                    "torrent".into(),
                    message,
                    Some(id.into()),
                );
                Some((state.snapshot(), state.persisted()))
            } else {
                None
            }
        };

        let Some((snapshot, persisted)) = paused else {
            return Ok(ExternalUsePreparation {
                paused_torrent: false,
                snapshot: None,
            });
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.clear_handoff_auth(id).await;
        self.wait_for_external_use_release(id, timeout, poll_interval)
            .await?;

        Ok(ExternalUsePreparation {
            paused_torrent: true,
            snapshot: Some(snapshot),
        })
    }

    pub(super) async fn wait_for_external_use_release(
        &self,
        id: &str,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<(), BackendError> {
        let started_at = Instant::now();
        let poll_interval = if poll_interval.is_zero() {
            Duration::from_millis(1)
        } else {
            poll_interval
        };

        loop {
            let ready = {
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

                !state.active_workers.contains(id) && job.state != JobState::Seeding
            };

            if ready {
                return Ok(());
            }

            let elapsed = started_at.elapsed();
            if elapsed >= timeout {
                return Err(BackendError {
                    code: "INTERNAL_ERROR",
                    message: "The torrent was paused, but its file handles are still being released. Try again in a moment.".into(),
                });
            }

            tokio::time::sleep(poll_interval.min(timeout.saturating_sub(elapsed))).await;
        }
    }

    pub async fn update_torrent_restore_target_path(
        &self,
        id: &str,
        target_path: &Path,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };
            if job.transfer_kind != TransferKind::Torrent {
                return Err("Job is not a torrent.".into());
            }

            job.target_path = target_path.display().to_string();
            let filename = job.filename.clone();
            state.push_diagnostic_event(
                DiagnosticLevel::Info,
                "torrent".into(),
                format!(
                    "Repaired seeding restore target for {filename} to {}",
                    target_path.display()
                ),
                Some(id.into()),
            );

            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn update_torrent_progress(
        &self,
        id: &str,
        update: TorrentRuntimeSnapshot,
        persist: bool,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            let preserve_paused = job.state == JobState::Paused;
            let was_seeding = job.state == JobState::Seeding;
            let restoring_seeding = job
                .torrent
                .as_ref()
                .and_then(|torrent| torrent.seeding_started_at)
                .is_some();
            let restore_validation = restoring_seeding
                && !update.finished
                && matches!(
                    update.phase,
                    TorrentRuntimePhase::Initializing | TorrentRuntimePhase::Paused
                );
            if !preserve_paused {
                job.state = if update.finished {
                    JobState::Seeding
                } else if restore_validation {
                    JobState::Downloading
                } else {
                    JobState::Downloading
                };
            }
            job.downloaded_bytes = update.downloaded_bytes;
            job.total_bytes = update.total_bytes.max(update.downloaded_bytes);
            job.speed = if preserve_paused {
                0
            } else if update.finished {
                update.upload_speed
            } else if restore_validation {
                0
            } else {
                update.download_speed
            };
            job.eta = if preserve_paused || update.finished || restore_validation {
                0
            } else {
                update.eta.unwrap_or(0)
            };
            job.progress = if job.total_bytes == 0 {
                0.0
            } else {
                (job.downloaded_bytes as f64 / job.total_bytes as f64 * 100.0).clamp(0.0, 100.0)
            };
            if let Some(name) = &update.name {
                job.filename = sanitize_filename(name);
            }
            let torrent = job.torrent.get_or_insert_with(TorrentInfo::default);
            let had_seeding_started = torrent.seeding_started_at.is_some();
            torrent.engine_id = Some(update.engine_id);
            torrent.info_hash = Some(update.info_hash);
            torrent.name = update.name;
            torrent.total_files = update.total_files;
            torrent.peers = update.peers;
            torrent.seeds = update.seeds;
            torrent.uploaded_bytes = cumulative_torrent_runtime_bytes(
                torrent.uploaded_bytes,
                torrent.last_runtime_uploaded_bytes,
                update.uploaded_bytes,
            );
            torrent.last_runtime_uploaded_bytes = Some(update.uploaded_bytes);
            torrent.fetched_bytes = cumulative_torrent_runtime_bytes(
                torrent.fetched_bytes,
                torrent.last_runtime_fetched_bytes,
                update.fetched_bytes,
            );
            torrent.last_runtime_fetched_bytes = Some(update.fetched_bytes);
            torrent.ratio = if update.downloaded_bytes == 0 {
                0.0
            } else {
                torrent.uploaded_bytes as f64 / update.downloaded_bytes as f64
            };
            if update.finished && torrent.seeding_started_at.is_none() {
                torrent.seeding_started_at = Some(current_unix_timestamp_millis());
            }
            let started_seeding =
                !preserve_paused && update.finished && !was_seeding && !had_seeding_started;
            let event_message = started_seeding.then(|| format!("Seeding {}", job.filename));
            if let Some(message) = event_message {
                state.push_diagnostic_event(
                    DiagnosticLevel::Info,
                    "torrent".into(),
                    message,
                    Some(id.into()),
                );
            }

            (state.snapshot(), state.persisted())
        };

        if persist {
            persist_state(&self.storage_path, &persisted)?;
        }
        Ok(snapshot)
    }

    pub async fn reset_stale_torrent_completion_for_recheck(
        &self,
        id: &str,
        info_hash: Option<String>,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let filename = {
                let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                    return Err("Job not found.".into());
                };

                job.state = JobState::Starting;
                job.progress = 0.0;
                job.downloaded_bytes = 0;
                job.total_bytes = 0;
                job.speed = 0;
                job.eta = 0;
                job.error = None;
                job.failure_category = None;

                let torrent = job.torrent.get_or_insert_with(TorrentInfo::default);
                if info_hash.is_some() {
                    torrent.info_hash = info_hash;
                }
                torrent.engine_id = None;
                torrent.peers = None;
                torrent.seeds = None;
                torrent.uploaded_bytes = 0;
                torrent.last_runtime_uploaded_bytes = None;
                torrent.fetched_bytes = 0;
                torrent.last_runtime_fetched_bytes = None;
                torrent.ratio = 0.0;
                torrent.seeding_started_at = None;

                job.filename.clone()
            };
            state.push_diagnostic_event(
                DiagnosticLevel::Warning,
                "torrent".into(),
                format!("Cleared stale torrent verification for {filename}; rechecking files"),
                Some(id.into()),
            );
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn complete_torrent_job(&self, id: &str) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let filename = {
                let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                    return Err("Job not found.".into());
                };
                job.state = JobState::Completed;
                job.progress = 100.0;
                job.speed = 0;
                job.eta = 0;
                job.filename.clone()
            };
            state.active_workers.remove(id);
            state.push_diagnostic_event(
                DiagnosticLevel::Info,
                "torrent".into(),
                format!("Completed torrent seeding for {filename}"),
                Some(id.into()),
            );
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }
}
