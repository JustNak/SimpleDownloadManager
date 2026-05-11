use super::*;

impl SharedState {
    pub async fn claim_schedulable_jobs(
        &self,
    ) -> Result<(DesktopSnapshot, Vec<DownloadTask>), String> {
        let auth_by_job = self.handoff_auth.read().await.clone();
        let (snapshot, persisted, tasks) = {
            let mut state = self.inner.write().await;
            let now = Instant::now();
            let active_normal_workers = state
                .active_workers
                .iter()
                .filter(|id| {
                    state
                        .job(id)
                        .map(|job| job.state != JobState::Seeding && !is_bulk_member_job(job))
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
            let normal_slot_limit = state.settings.max_concurrent_downloads.max(1);
            let bulk_slot_limit = state.settings.bulk.max_concurrent_downloads.max(1);
            let available_normal_slots = normal_slot_limit.saturating_sub(active_normal_workers);
            let available_bulk_slots = bulk_slot_limit.saturating_sub(active_bulk_workers);

            if available_normal_slots == 0 && available_bulk_slots == 0 {
                return Ok((state.snapshot(), Vec::new()));
            }

            let mut scheduled_ids = Vec::new();
            let mut scheduled_normal_workers = 0_u32;
            let mut scheduled_bulk_workers = 0_u32;
            let fairness_mode = state.settings.bulk.hoster_fairness_mode;
            let fairness_metrics_by_key = bulk_hoster_fairness_metrics_by_key(&state, now);
            let fairness_diagnostics = if fairness_mode == BulkHosterFairnessMode::Adaptive {
                state
                    .bulk_hoster_fairness
                    .retain(|key, _| fairness_metrics_by_key.contains_key(key));
                let mut diagnostics = Vec::new();
                for (key, metrics) in &fairness_metrics_by_key {
                    diagnostics.extend(
                        state
                            .bulk_hoster_fairness
                            .entry(key.clone())
                            .or_default()
                            .reconcile(*metrics, bulk_slot_limit, now),
                    );
                }
                diagnostics
            } else {
                state.bulk_hoster_fairness.clear();
                Vec::new()
            };
            for message in fairness_diagnostics {
                state.push_diagnostic_event(
                    DiagnosticLevel::Info,
                    "download".into(),
                    message,
                    None,
                );
            }
            let mut protected_bulk_hoster_targets = HashMap::new();
            for key in state
                .jobs
                .iter()
                .filter_map(protected_bulk_hoster_fairness_key)
                .chain(fairness_metrics_by_key.keys().cloned())
            {
                let target = match fairness_mode {
                    BulkHosterFairnessMode::Adaptive => state
                        .bulk_hoster_fairness
                        .get(&key)
                        .map(|controller| controller.target_for_bulk_limit(bulk_slot_limit))
                        .unwrap_or(1),
                    BulkHosterFairnessMode::Safe => 1,
                    BulkHosterFairnessMode::Off => bulk_slot_limit,
                };
                protected_bulk_hoster_targets.insert(key, target);
            }
            let mut scheduled_protected_bulk_hoster_workers: HashMap<String, u32> = HashMap::new();
            for job in &state.jobs {
                if scheduled_normal_workers >= available_normal_slots
                    && scheduled_bulk_workers >= available_bulk_slots
                {
                    break;
                }

                if job.state == JobState::Queued && !state.active_workers.contains(&job.id) {
                    if is_bulk_member_job(job) {
                        if scheduled_bulk_workers >= available_bulk_slots {
                            continue;
                        }
                        if let Some(fairness_key) = protected_bulk_hoster_fairness_key(job) {
                            let fairness_metrics = fairness_metrics_by_key
                                .get(&fairness_key)
                                .copied()
                                .unwrap_or_default();
                            let protected_bulk_hoster_claim_blocked = fairness_mode
                                != BulkHosterFairnessMode::Off
                                && fairness_metrics.has_blocking_worker;
                            let scheduled_for_origin = scheduled_protected_bulk_hoster_workers
                                .get(&fairness_key)
                                .copied()
                                .unwrap_or(0);
                            let protected_bulk_hoster_target = protected_bulk_hoster_targets
                                .get(&fairness_key)
                                .copied()
                                .unwrap_or(1);
                            if protected_bulk_hoster_claim_blocked
                                || fairness_metrics.active_count + scheduled_for_origin
                                    >= protected_bulk_hoster_target
                            {
                                continue;
                            }
                            *scheduled_protected_bulk_hoster_workers
                                .entry(fairness_key)
                                .or_default() += 1;
                        }
                        scheduled_bulk_workers += 1;
                    } else {
                        if scheduled_normal_workers >= available_normal_slots {
                            continue;
                        }
                        scheduled_normal_workers += 1;
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

fn bulk_hoster_fairness_metrics_by_key(
    state: &RuntimeState,
    now: Instant,
) -> HashMap<String, BulkHosterFairnessMetrics> {
    let mut metrics_by_key = HashMap::new();

    for id in &state.active_workers {
        let Some(job) = state.job(id) else {
            continue;
        };
        if !matches!(job.state, JobState::Starting | JobState::Downloading) {
            continue;
        }
        let Some(fairness_key) = protected_bulk_hoster_fairness_key(job) else {
            continue;
        };

        let Some(health) = state.bulk_hoster_worker_health.get(id) else {
            continue;
        };
        let metrics = metrics_by_key
            .entry(fairness_key)
            .or_insert(BulkHosterFairnessMetrics {
                all_healthy: true,
                ..Default::default()
            });
        metrics.active_count = metrics.active_count.saturating_add(1);
        metrics.aggregate_speed = metrics
            .aggregate_speed
            .saturating_add(health.last_reported_speed);
        if health.blocks_bulk_hoster_claim(now) {
            metrics.has_blocking_worker = true;
        }
        if !health.is_healthy(now) {
            metrics.all_healthy = false;
        }
    }

    metrics_by_key
}
