use super::*;
use futures_util::{stream, StreamExt};

impl SharedState {
    pub async fn pause_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted, diagnostic_events) = {
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
                    job.active_segments = None;
                    job.planned_segments = None;
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
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        self.clear_handoff_auth(id).await;
        Ok(snapshot)
    }

    pub async fn resume_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            let event_message = {
                let job = find_job_mut(&mut state.jobs, id)?;
                ensure_not_removing(job)?;
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
                    job.active_segments = None;
                    job.planned_segments = None;
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
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
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
        self.cancel_jobs(&[id.to_string()]).await
    }

    pub async fn cancel_jobs_for_delete(
        &self,
        ids: &[String],
    ) -> Result<DestructiveCleanupPlan, BackendError> {
        if ids.is_empty() {
            return Ok(DestructiveCleanupPlan {
                snapshot: self.snapshot().await,
                jobs: Vec::new(),
            });
        }
        let (snapshot, persisted, cleanup_jobs, diagnostic_events) = {
            let mut state = self.inner.write().await;
            let mut cleanup_jobs = Vec::new();
            let mut captured_archive_outputs = HashSet::new();
            for id in ids {
                let Some(job_index) = state.jobs.iter().position(|job| job.id == *id) else {
                    continue;
                };
                state.external_reseed_jobs.remove(id);
                let active = state.active_workers.contains(id);
                let (event_message, should_push_event, clear_hoster_health, cleanup_job) = {
                    let job = &mut state.jobs[job_index];
                    let paths =
                        destructive_cleanup_paths_for_job(job, &mut captured_archive_outputs);
                    let cleanup_job = DestructiveCleanupJob {
                        id: job.id.clone(),
                        filename: job.filename.clone(),
                        paths,
                        partial_artifact_roots: destructive_partial_artifact_roots_for_job(job),
                        wait_for_worker_release: active,
                    };
                    let clear_hoster_health = is_protected_bulk_hoster_job(job);
                    if is_cancel_delete_cancel_target(job) {
                        mark_job_canceled(job);
                        job.removal_state = Some(RemovalState::Removing);
                        (
                            Some(format!("Canceled {} for disk cleanup", job.filename)),
                            true,
                            clear_hoster_health,
                            cleanup_job,
                        )
                    } else {
                        job.removal_state = Some(RemovalState::Removing);
                        job.error = None;
                        job.failure_category = None;
                        (
                            Some(format!("Scheduled {} for disk cleanup", job.filename)),
                            true,
                            clear_hoster_health,
                            cleanup_job,
                        )
                    }
                };
                if clear_hoster_health {
                    state.clear_bulk_hoster_worker_health(id);
                }
                cleanup_jobs.push(cleanup_job);
                if should_push_event {
                    state.push_diagnostic_event(
                        DiagnosticLevel::Info,
                        "download".into(),
                        event_message.unwrap_or_default(),
                        Some(id.clone()),
                    );
                }
            }
            let diagnostic_events = state.take_pending_diagnostic_events();
            (
                state.snapshot(),
                state.persisted(),
                cleanup_jobs,
                diagnostic_events,
            )
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        {
            let mut handoff_auth = self.handoff_auth.write().await;
            for id in ids {
                handoff_auth.remove(id);
            }
        }
        Ok(DestructiveCleanupPlan {
            snapshot,
            jobs: cleanup_jobs,
        })
    }

    pub async fn cancel_jobs(&self, ids: &[String]) -> Result<DesktopSnapshot, BackendError> {
        if ids.is_empty() {
            return Ok(self.snapshot().await);
        }
        let temp_paths_to_remove =
            {
                let state = self.inner.read().await;
                let mut temp_paths = Vec::new();
                for id in ids {
                    let job = state.jobs.iter().find(|job| job.id == *id).ok_or_else(|| {
                        BackendError {
                            code: "INTERNAL_ERROR",
                            message: "Job not found.".into(),
                        }
                    })?;

                    if !state.active_workers.contains(id) {
                        temp_paths.push(PathBuf::from(&job.temp_path));
                    }
                }
                temp_paths
            };

        for temp_path in temp_paths_to_remove {
            let _ = remove_path_if_exists(&temp_path);
        }

        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            for id in ids {
                state.external_reseed_jobs.remove(id);
                let job_index =
                    state
                        .jobs
                        .iter()
                        .position(|job| job.id == *id)
                        .ok_or_else(|| BackendError {
                            code: "INTERNAL_ERROR",
                            message: "Job not found.".into(),
                        })?;
                let (event_message, clear_hoster_health) = {
                    let job = &mut state.jobs[job_index];
                    let clear_hoster_health = is_protected_bulk_hoster_job(job);
                    mark_job_canceled(job);
                    (format!("Canceled {}", job.filename), clear_hoster_health)
                };
                if clear_hoster_health {
                    state.clear_bulk_hoster_worker_health(id);
                }
                state.push_diagnostic_event(
                    DiagnosticLevel::Info,
                    "download".into(),
                    event_message,
                    Some(id.clone()),
                );
            }
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        {
            let mut handoff_auth = self.handoff_auth.write().await;
            for id in ids {
                handoff_auth.remove(id);
            }
        }
        Ok(snapshot)
    }

    pub async fn retry_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            let event_message = {
                let job = find_job_mut(&mut state.jobs, id)?;
                ensure_not_removing(job)?;
                if is_recovered_local_job(job) {
                    return Err(recovered_local_action_error());
                }
                job.state = JobState::Queued;
                job.removal_state = None;
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
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
    }

    pub async fn restart_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted, diagnostic_events) = {
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
                ensure_not_removing(job)?;
                if is_recovered_local_job(job) {
                    return Err(recovered_local_action_error());
                }
                remove_partial_artifacts(Path::new(&job.temp_path)).map_err(internal_error)?;
                reset_job_for_restart(job);
                format!("Restart queued for {}", job.filename)
            };
            state.push_diagnostic_event(
                DiagnosticLevel::Info,
                "download".into(),
                event_message,
                Some(id.into()),
            );
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
    }

    pub async fn bulk_member_auto_restart_candidate(
        &self,
        id: &str,
        failure_category: FailureCategory,
        failure_message: &str,
        retryable: bool,
    ) -> Result<Option<BulkMemberAutoRestartCandidate>, String> {
        let state = self.inner.read().await;
        let Some(job) = state.job(id) else {
            return Ok(None);
        };
        let max_attempts = max_auto_retry_attempts_for_job(&state.settings, job);
        let Some(mode) =
            bulk_member_auto_restart_mode(job, failure_category, failure_message, retryable)
        else {
            return Ok(None);
        };

        if max_attempts == 0
            || job.auto_restart_attempts >= max_attempts
            || !is_pending_http_bulk_member(job)
        {
            return Ok(None);
        }

        Ok(Some(BulkMemberAutoRestartCandidate {
            resolved_from_url: job.resolved_from_url.clone(),
            mode,
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

    #[allow(clippy::too_many_arguments)]
    pub async fn auto_restart_bulk_member(
        &self,
        id: &str,
        resolved_url: String,
        mode: BulkMemberAutoRestartMode,
        attempt: u32,
        max_attempts: u32,
        failure_category: FailureCategory,
        failure_message: &str,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            state.remove_active_worker(id);
            state.clear_bulk_hoster_worker_health(id);
            let event_message = {
                let job = find_job_mut(&mut state.jobs, id).map_err(|error| error.message)?;
                match mode {
                    BulkMemberAutoRestartMode::PreservePartial => {
                        queue_job_for_preserved_bulk_recovery(job);
                    }
                    BulkMemberAutoRestartMode::ResetPartial => {
                        remove_partial_artifacts(Path::new(&job.temp_path))?;
                        reset_job_for_restart(job);
                    }
                }
                job.url = resolved_url;
                job.auto_restart_attempts = attempt;
                format!(
                    "Auto-restart queued for {} ({} partial, attempt {attempt}/{max_attempts}, {} error: {failure_message})",
                    job.filename,
                    bulk_member_auto_restart_mode_label(mode),
                    failure_category_label(failure_category),
                )
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
        let (snapshot, persisted, diagnostic_events) = {
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

                queue_job_for_preserved_bulk_recovery(job);
                job.url = resolved_url;
                format!("Retry queued for bulk member {}", job.filename)
            };
            state.push_diagnostic_event(
                DiagnosticLevel::Info,
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
        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;

            for job in &mut state.jobs {
                if job.state == JobState::Failed
                    && job.removal_state.is_none()
                    && !is_recovered_local_job(job)
                {
                    job.state = JobState::Queued;
                    job.removal_state = None;
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

            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
    }

    pub async fn clear_completed_jobs(&self) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.retain_jobs(|job| {
                job.removal_state.is_some()
                    || !matches!(job.state, JobState::Completed | JobState::Canceled)
            });
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn remove_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted, paths_to_cleanup, diagnostic_events) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            let Some(job_index) = state.jobs.iter().position(|job| job.id == id) else {
                return Ok(state.snapshot());
            };
            let is_active_worker = state.active_workers.contains(id);
            let job = &state.jobs[job_index];
            ensure_not_removing(job)?;

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

            let diagnostic_events = state.take_pending_diagnostic_events();
            (
                state.snapshot(),
                state.persisted(),
                paths_to_cleanup,
                diagnostic_events,
            )
        };

        let (temp_path, job_state) = paths_to_cleanup;
        if job_state != JobState::Completed {
            let _ = remove_path_if_exists(&temp_path);
        }

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
    }

    pub async fn delete_job(
        &self,
        id: &str,
        delete_from_disk: bool,
    ) -> Result<DesktopSnapshot, BackendError> {
        if !self.inner.read().await.jobs.iter().any(|job| job.id == id) {
            return Ok(self.snapshot().await);
        }

        if delete_from_disk {
            self.wait_for_disk_delete_release(id).await?;
            let (target_path, temp_path, partial_artifact_roots, bulk_archive_output_path) = {
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
                    destructive_partial_artifact_roots_for_job(job),
                    job.bulk_archive.as_ref().and_then(|archive| {
                        if archive.archive_status == BulkArchiveStatus::Completed {
                            archive.output_path.as_ref().map(PathBuf::from)
                        } else {
                            None
                        }
                    }),
                )
            };

            for partial_artifact_root in partial_artifact_roots {
                remove_partial_artifacts(&partial_artifact_root).map_err(internal_error)?;
            }
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

    pub async fn delete_jobs_for_disk_cleanup(
        &self,
        ids: &[String],
    ) -> Result<DestructiveCleanupPlan, BackendError> {
        if ids.is_empty() {
            return Ok(DestructiveCleanupPlan {
                snapshot: self.snapshot().await,
                jobs: Vec::new(),
            });
        }

        let (snapshot, persisted, cleanup_jobs, diagnostic_events) = {
            let mut state = self.inner.write().await;
            let mut cleanup_jobs = Vec::new();
            let mut captured_archive_outputs = HashSet::new();

            for id in ids {
                let Some(job_index) = state.jobs.iter().position(|job| job.id == *id) else {
                    continue;
                };
                let active = state.active_workers.contains(id);
                let job = &state.jobs[job_index];
                if job_blocks_removal(job, active) {
                    return Err(BackendError {
                        code: "INTERNAL_ERROR",
                        message:
                            "Pause or cancel the active transfer before deleting files from disk."
                                .into(),
                    });
                }

                state.external_reseed_jobs.remove(id);
                let cleanup_job = {
                    let job = &mut state.jobs[job_index];
                    let paths =
                        destructive_cleanup_paths_for_job(job, &mut captured_archive_outputs);
                    job.removal_state = Some(RemovalState::Removing);
                    job.error = None;
                    job.failure_category = None;
                    DestructiveCleanupJob {
                        id: job.id.clone(),
                        filename: job.filename.clone(),
                        paths,
                        partial_artifact_roots: destructive_partial_artifact_roots_for_job(job),
                        wait_for_worker_release: active,
                    }
                };
                state.clear_bulk_hoster_worker_health(id);
                state.push_diagnostic_event(
                    DiagnosticLevel::Info,
                    "download".into(),
                    format!("Scheduled {} for disk cleanup", cleanup_job.filename),
                    Some(id.clone()),
                );
                cleanup_jobs.push(cleanup_job);
            }

            let diagnostic_events = state.take_pending_diagnostic_events();
            (
                state.snapshot(),
                state.persisted(),
                cleanup_jobs,
                diagnostic_events,
            )
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        {
            let mut handoff_auth = self.handoff_auth.write().await;
            for id in ids {
                handoff_auth.remove(id);
            }
        }
        Ok(DestructiveCleanupPlan {
            snapshot,
            jobs: cleanup_jobs,
        })
    }

    pub async fn run_destructive_cleanup(
        &self,
        jobs: Vec<DestructiveCleanupJob>,
    ) -> Result<DesktopSnapshot, BackendError> {
        if jobs.is_empty() {
            return Ok(self.snapshot().await);
        }

        let state = self.clone();
        let mut cleanup_results = stream::iter(jobs)
            .map(move |job| {
                let state = state.clone();
                async move { state.cleanup_destructive_job(job).await }
            })
            .buffer_unordered(8);

        let mut snapshot = self.snapshot().await;
        while let Some(result) = cleanup_results.next().await {
            snapshot = match result {
                Ok(id) => self.finish_destructive_cleanup_success(&id).await?,
                Err((job, message)) => {
                    self.record_destructive_cleanup_failure(&job, message)
                        .await?
                }
            };
        }

        Ok(snapshot)
    }

    async fn cleanup_destructive_job(
        &self,
        job: DestructiveCleanupJob,
    ) -> Result<String, (DestructiveCleanupJob, String)> {
        if job.wait_for_worker_release {
            if let Err(error) = self
                .wait_for_active_worker_release(
                    &job.id,
                    "Canceled download files are still being released. Use Delete from disk again in a moment.",
                )
                .await
            {
                return Err((job, error.message));
            }
        }

        let cleanup_job = job.clone();
        let cleanup_result = tokio::task::spawn_blocking(move || {
            remove_destructive_cleanup_paths(
                &cleanup_job.paths,
                &cleanup_job.partial_artifact_roots,
            )
        })
        .await
        .map_err(|error| (job.clone(), format!("Could not run disk cleanup: {error}")))?;

        match cleanup_result {
            Ok(()) => Ok(job.id),
            Err(message) => Err((job, message)),
        }
    }

    async fn finish_destructive_cleanup_success(
        &self,
        id: &str,
    ) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.external_reseed_jobs.remove(id);
            state.remove_active_worker(id);
            if let Some(index) = state.jobs.iter().position(|job| job.id == id) {
                state.remove_job_at_index(index);
            }
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    async fn record_destructive_cleanup_failure(
        &self,
        job: &DestructiveCleanupJob,
        message: String,
    ) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            if let Some(existing) = state.job_mut(&job.id) {
                existing.removal_state = Some(RemovalState::CleanupFailed);
                mark_job_canceled(existing);
                existing.removal_state = Some(RemovalState::CleanupFailed);
                existing.error = Some(format!("Could not delete files from disk: {message}"));
                existing.failure_category = Some(FailureCategory::Disk);
            }
            state.push_diagnostic_event(
                DiagnosticLevel::Warning,
                "download".into(),
                format!(
                    "Could not delete canceled download files for {}: {message}",
                    job.filename
                ),
                Some(job.id.clone()),
            );
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
    }

    pub async fn delete_canceled_jobs_after_release(
        &self,
        ids: &[String],
    ) -> Result<DesktopSnapshot, BackendError> {
        let mut snapshot = self.snapshot().await;
        for id in ids {
            match self.delete_canceled_job_after_release(id).await {
                Ok(next_snapshot) => snapshot = next_snapshot,
                Err(error) => {
                    snapshot = self
                        .record_cancel_delete_cleanup_failure(id, error.message)
                        .await?;
                }
            }
        }

        Ok(snapshot)
    }

    async fn delete_canceled_job_after_release(
        &self,
        id: &str,
    ) -> Result<DesktopSnapshot, BackendError> {
        let should_wait = {
            let state = self.inner.read().await;
            let Some(_) = state.jobs.iter().find(|job| job.id == id) else {
                return Ok(state.snapshot());
            };
            state.active_workers.contains(id)
        };

        if should_wait {
            self.wait_for_active_worker_release(
                id,
                "Canceled download files are still being released. Use Delete from disk again in a moment.",
            )
            .await?;
        }

        let plan = self.delete_jobs_for_disk_cleanup(&[id.to_string()]).await?;
        self.run_destructive_cleanup(plan.jobs).await
    }

    async fn record_cancel_delete_cleanup_failure(
        &self,
        id: &str,
        message: String,
    ) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            if let Some(job) = state.job_mut(id) {
                mark_job_canceled(job);
                job.error = Some(format!("Could not delete files from disk: {message}"));
                job.failure_category = Some(FailureCategory::Disk);
            }
            state.push_diagnostic_event(
                DiagnosticLevel::Warning,
                "download".into(),
                format!("Could not delete canceled download files: {message}"),
                Some(id.into()),
            );
            let diagnostic_events = state.take_pending_diagnostic_events();
            (state.snapshot(), state.persisted(), diagnostic_events)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        self.append_diagnostic_events_in_background(diagnostic_events);
        Ok(snapshot)
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
    job.removal_state = None;
    job.progress = 0.0;
    job.total_bytes = 0;
    job.downloaded_bytes = 0;
    job.speed = 0;
    job.eta = 0;
    job.active_segments = None;
    job.planned_segments = None;
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

fn queue_job_for_preserved_bulk_recovery(job: &mut DownloadJob) {
    job.state = JobState::Queued;
    job.removal_state = None;
    job.speed = 0;
    job.eta = 0;
    job.active_segments = None;
    job.planned_segments = None;
    job.error = None;
    job.failure_category = None;
    job.retry_attempts = 0;
    reset_integrity_for_retry(job);
}

pub(super) fn reset_integrity_for_retry(job: &mut DownloadJob) {
    if let Some(check) = &mut job.integrity_check {
        check.actual = None;
        check.status = IntegrityStatus::Pending;
    }
}

pub(super) fn bulk_member_auto_restart_mode(
    job: &DownloadJob,
    failure_category: FailureCategory,
    failure_message: &str,
    retryable: bool,
) -> Option<BulkMemberAutoRestartMode> {
    if !bulk_member_auto_restart_failure_is_transient(
        job,
        failure_category,
        failure_message,
        retryable,
    ) {
        return None;
    }

    if failure_category == FailureCategory::Resume {
        return None;
    }

    Some(BulkMemberAutoRestartMode::PreservePartial)
}

fn bulk_member_auto_restart_failure_is_transient(
    job: &DownloadJob,
    failure_category: FailureCategory,
    failure_message: &str,
    retryable: bool,
) -> bool {
    match failure_category {
        FailureCategory::Network | FailureCategory::Server | FailureCategory::Resume => true,
        FailureCategory::Http => {
            retryable
                || (is_protected_bulk_hoster_job(job)
                    && hoster_token_recovery_failure_message(failure_message))
        }
        _ => false,
    }
}

fn hoster_token_recovery_failure_message(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    ["403", "404", "410", "416", "html"]
        .iter()
        .any(|needle| normalized.contains(needle))
}

fn bulk_member_auto_restart_mode_label(mode: BulkMemberAutoRestartMode) -> &'static str {
    match mode {
        BulkMemberAutoRestartMode::PreservePartial => "preserve",
        BulkMemberAutoRestartMode::ResetPartial => "reset",
    }
}

fn failure_category_label(category: FailureCategory) -> &'static str {
    match category {
        FailureCategory::Network => "network",
        FailureCategory::Http => "http",
        FailureCategory::Server => "server",
        FailureCategory::Disk => "disk",
        FailureCategory::Permission => "permission",
        FailureCategory::Resume => "resume",
        FailureCategory::Integrity => "integrity",
        FailureCategory::Torrent => "torrent",
        FailureCategory::Internal => "internal",
    }
}

pub(super) fn is_pending_http_bulk_member(job: &DownloadJob) -> bool {
    job.transfer_kind == TransferKind::Http
        && job
            .bulk_archive
            .as_ref()
            .is_some_and(|archive| archive.archive_status == BulkArchiveStatus::Pending)
}

fn is_cancel_delete_cancel_target(job: &DownloadJob) -> bool {
    matches!(
        job.state,
        JobState::Queued
            | JobState::Starting
            | JobState::Downloading
            | JobState::Seeding
            | JobState::Paused
            | JobState::Failed
    )
}

fn destructive_cleanup_paths_for_job(
    job: &DownloadJob,
    captured_archive_outputs: &mut HashSet<String>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    push_unique_cleanup_path(&mut paths, PathBuf::from(&job.target_path));
    push_unique_cleanup_path(&mut paths, PathBuf::from(&job.temp_path));

    if let Some(archive_path) = job.bulk_archive.as_ref().and_then(|archive| {
        if archive.archive_status == BulkArchiveStatus::Completed {
            archive.output_path.as_ref()
        } else {
            None
        }
    }) {
        if captured_archive_outputs.insert(archive_path.clone()) {
            push_unique_cleanup_path(&mut paths, PathBuf::from(archive_path));
        }
    }

    paths
}

fn destructive_partial_artifact_roots_for_job(job: &DownloadJob) -> Vec<PathBuf> {
    if job.transfer_kind != TransferKind::Http || job.temp_path.trim().is_empty() {
        return Vec::new();
    }

    vec![PathBuf::from(&job.temp_path)]
}

fn push_unique_cleanup_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.as_os_str().is_empty() {
        return;
    }
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn remove_partial_artifacts(temp_path: &Path) -> Result<(), String> {
    remove_file_if_exists(temp_path)?;
    remove_file_if_exists(&PathBuf::from(format!("{}.meta", temp_path.display())))?;
    remove_file_if_exists(&PathBuf::from(format!("{}.meta.tmp", temp_path.display())))?;
    remove_file_if_exists(&PathBuf::from(format!("{}.meta.bak", temp_path.display())))?;

    let Some(parent) = temp_path.parent() else {
        return Ok(());
    };
    if !parent.exists() {
        return Ok(());
    }
    let Some(file_name) = temp_path.file_name().and_then(|value| value.to_str()) else {
        return Ok(());
    };
    let segment_prefix = format!("{file_name}.seg");
    let metadata_temp_prefix = format!("{file_name}.meta.");

    for entry in std::fs::read_dir(parent)
        .map_err(|error| format!("Could not inspect partial download sidecars: {error}"))?
    {
        let entry = entry
            .map_err(|error| format!("Could not inspect partial download sidecar: {error}"))?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with(&segment_prefix)
            || (name.starts_with(&metadata_temp_prefix) && name.ends_with(".tmp"))
        {
            remove_file_if_exists(&entry.path())?;
        }
    }

    Ok(())
}

fn remove_destructive_cleanup_paths(
    paths: &[PathBuf],
    partial_artifact_roots: &[PathBuf],
) -> Result<(), String> {
    for partial_artifact_root in partial_artifact_roots {
        remove_partial_artifacts(partial_artifact_root)?;
    }

    for path in paths {
        remove_path_if_exists(path)?;
    }
    Ok(())
}

fn mark_job_canceled(job: &mut DownloadJob) {
    job.state = JobState::Canceled;
    job.progress = 0.0;
    job.total_bytes = 0;
    job.downloaded_bytes = 0;
    job.speed = 0;
    job.eta = 0;
    job.active_segments = None;
    job.planned_segments = None;
    job.error = None;
    job.failure_category = None;
    job.retry_attempts = 0;
    job.auto_restart_attempts = 0;
    reset_integrity_for_retry(job);
}

fn ensure_not_removing(job: &DownloadJob) -> Result<(), BackendError> {
    if job.removal_state.is_some() {
        return Err(BackendError {
            code: "INTERNAL_ERROR",
            message: "This download is waiting for disk cleanup. Use Delete from disk to retry failed cleanup.".into(),
        });
    }
    Ok(())
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
