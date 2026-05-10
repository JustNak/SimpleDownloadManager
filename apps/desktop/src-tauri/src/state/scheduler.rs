use super::*;

impl SharedState {
    pub async fn claim_schedulable_jobs(
        &self,
    ) -> Result<(DesktopSnapshot, Vec<DownloadTask>), String> {
        let auth_by_job = self.handoff_auth.read().await.clone();
        let (snapshot, persisted, tasks) = {
            let mut state = self.inner.write().await;
            let now = Instant::now();
            let active_download_workers = state
                .active_workers
                .iter()
                .filter(|id| {
                    state
                        .job(id)
                        .map(|job| job.state != JobState::Seeding)
                        .unwrap_or(false)
                })
                .count() as u32;
            let active_bulk_workers = state
                .active_workers
                .iter()
                .filter(|id| {
                    state
                        .job(id)
                        .map(|job| job.state != JobState::Seeding && is_bulk_member_job(job))
                        .unwrap_or(false)
                })
                .count() as u32;
            let available_slots = state
                .settings
                .max_concurrent_downloads
                .max(1)
                .saturating_sub(active_download_workers) as usize;
            let bulk_slot_limit = state.settings.bulk.max_concurrent_downloads.max(1);

            if available_slots == 0 {
                return Ok((state.snapshot(), Vec::new()));
            }

            let mut scheduled_ids = Vec::new();
            let mut scheduled_bulk_workers = 0_u32;
            let mut protected_bulk_hoster_claim_blocked =
                protected_bulk_hoster_worker_blocks_claim(&state, now);
            for job in &state.jobs {
                if scheduled_ids.len() >= available_slots {
                    break;
                }

                if job.state == JobState::Queued && !state.active_workers.contains(&job.id) {
                    if is_bulk_member_job(job) {
                        if active_bulk_workers + scheduled_bulk_workers >= bulk_slot_limit {
                            continue;
                        }
                        if is_protected_bulk_hoster_job(job) && protected_bulk_hoster_claim_blocked
                        {
                            continue;
                        }
                        scheduled_bulk_workers += 1;
                        if is_protected_bulk_hoster_job(job) {
                            protected_bulk_hoster_claim_blocked = true;
                        }
                    }
                    scheduled_ids.push(job.id.clone());
                }
            }

            let mut tasks = Vec::new();
            for scheduled_id in scheduled_ids {
                if let Some(job) = state.job_mut(&scheduled_id) {
                    job.state = JobState::Starting;
                    job.speed = 0;
                    job.eta = 0;
                    job.error = None;
                    let task = DownloadTask {
                        id: job.id.clone(),
                        url: job.url.clone(),
                        filename: job.filename.clone(),
                        transfer_kind: job.transfer_kind,
                        torrent: job.torrent.clone(),
                        handoff_auth: auth_by_job.get(&job.id).cloned(),
                        resolved_from_url: job.resolved_from_url.clone(),
                        is_bulk_member: is_bulk_member_job(job),
                        retry_attempts: job.retry_attempts,
                        target_path: PathBuf::from(&job.target_path),
                        temp_path: PathBuf::from(&job.temp_path),
                    };
                    let task_id = task.id.clone();
                    let hoster_health = is_protected_bulk_hoster_job(job)
                        .then(|| BulkHosterWorkerHealth::from_job(job, now));
                    let _ = job;
                    state.active_workers.insert(task_id);
                    if let Some(health) = hoster_health {
                        state
                            .bulk_hoster_worker_health
                            .insert(task.id.clone(), health);
                    }
                    state.push_diagnostic_event(
                        DiagnosticLevel::Info,
                        "download".into(),
                        format!("Starting {}", task.id),
                        Some(task.id.clone()),
                    );
                    tasks.push(task);
                }
            }

            (state.snapshot(), state.persisted(), tasks)
        };

        if !tasks.is_empty() {
            persist_state(&self.storage_path, &persisted)?;
        }

        Ok((snapshot, tasks))
    }

    pub async fn clear_handoff_auth(&self, id: &str) {
        self.handoff_auth.write().await.remove(id);
    }

    #[cfg(test)]
    pub(super) async fn has_handoff_auth(&self, id: &str) -> bool {
        self.handoff_auth.read().await.contains_key(id)
    }

    pub async fn worker_control(&self, id: &str) -> WorkerControl {
        let state = self.inner.read().await;
        let Some(job) = state.job(id) else {
            return WorkerControl::Missing;
        };

        match job.state {
            JobState::Paused => WorkerControl::Paused,
            JobState::Canceled => WorkerControl::Canceled,
            JobState::Completed | JobState::Failed => WorkerControl::Missing,
            _ => WorkerControl::Continue,
        }
    }
}

fn is_bulk_member_job(job: &DownloadJob) -> bool {
    job.transfer_kind == TransferKind::Http && job.bulk_archive.is_some()
}

fn is_protected_bulk_hoster_job(job: &DownloadJob) -> bool {
    is_bulk_member_job(job) && job.resolved_from_url.is_some()
}

fn protected_bulk_hoster_worker_blocks_claim(state: &RuntimeState, now: Instant) -> bool {
    state.active_workers.iter().any(|id| {
        state
            .job(id)
            .filter(|job| is_protected_bulk_hoster_job(job))
            .and_then(|_| state.bulk_hoster_worker_health.get(id))
            .is_some_and(|health| health.blocks_bulk_hoster_claim(now))
    })
}
