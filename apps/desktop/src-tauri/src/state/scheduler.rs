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
                    let max_adaptive_concurrency =
                        protected_bulk_hoster_max_adaptive_concurrency_for_key(&state, key);
                    diagnostics.extend(
                        state
                            .bulk_hoster_fairness
                            .entry(key.clone())
                            .or_default()
                            .reconcile(metrics, bulk_slot_limit, max_adaptive_concurrency, now),
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
            state
                .datanodes_priority_defer_until
                .retain(|_, until| *until > now);
            let mut protected_bulk_hoster_targets = HashMap::new();
            for key in state
                .jobs
                .iter()
                .filter_map(protected_bulk_hoster_fairness_key)
                .chain(fairness_metrics_by_key.keys().cloned())
            {
                let max_adaptive_concurrency =
                    protected_bulk_hoster_max_adaptive_concurrency_for_key(&state, &key);
                let target = match fairness_mode {
                    BulkHosterFairnessMode::Adaptive => state
                        .bulk_hoster_fairness
                        .get(&key)
                        .map(|controller| {
                            controller
                                .target_for_bulk_limit(bulk_slot_limit, max_adaptive_concurrency)
                        })
                        .unwrap_or(1),
                    BulkHosterFairnessMode::Safe => 1,
                    BulkHosterFairnessMode::Off => bulk_slot_limit,
                };
                protected_bulk_hoster_targets.insert(key, target);
            }
            let mut scheduled_protected_bulk_hoster_workers: HashMap<String, u32> = HashMap::new();
            let mut blocked_datanodes_queue_keys: HashSet<String> = HashSet::new();
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
                                .cloned()
                                .unwrap_or_default();
                            let accelerated_datanodes =
                                is_accelerated_datanodes_bulk_job(&state.settings, job);
                            if accelerated_datanodes {
                                if blocked_datanodes_queue_keys.contains(&fairness_key) {
                                    continue;
                                }
                                if state.datanodes_priority_defer_until.contains_key(&job.id) {
                                    blocked_datanodes_queue_keys.insert(fairness_key);
                                    continue;
                                }
                            }
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
                                || (accelerated_datanodes && scheduled_for_origin > 0)
                            {
                                if accelerated_datanodes {
                                    blocked_datanodes_queue_keys.insert(fairness_key);
                                }
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
            let settings_for_claim = state.settings.clone();
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
                        bulk_archive_id: job
                            .bulk_archive
                            .as_ref()
                            .map(|archive| archive.id.clone()),
                        retry_attempts: job.retry_attempts,
                        target_path: PathBuf::from(&job.target_path),
                        temp_path: PathBuf::from(&job.temp_path),
                    };
                    let task_id = task.id.clone();
                    let hoster_health = is_protected_bulk_hoster_job(job).then(|| {
                        BulkHosterWorkerHealth::from_job_with_profile(
                            job,
                            bulk_hoster_worker_profile_for_job(&settings_for_claim, job),
                            now,
                        )
                    });
                    let accelerated_datanodes =
                        is_accelerated_datanodes_bulk_job(&settings_for_claim, job);
                    let _ = job;
                    state.active_workers.insert(task_id);
                    if let Some(health) = hoster_health {
                        state
                            .bulk_hoster_worker_health
                            .insert(task.id.clone(), health);
                        if accelerated_datanodes {
                            state.push_diagnostic_event(
                                DiagnosticLevel::Info,
                                "download".into(),
                                format!(
                                    "DataNodes priority admitted {} after healthy runway.",
                                    task.id
                                ),
                                Some(task.id.clone()),
                            );
                        }
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

    pub(crate) async fn hoster_priority_throttle_decision(
        &self,
        id: &str,
    ) -> Option<HosterPriorityThrottleDecision> {
        let mut state = self.inner.write().await;
        let decision = hoster_priority_throttle_decision_for_state(&state, id);
        match decision.as_ref() {
            Some(decision) => {
                let should_report = state
                    .hoster_priority_cap_reports
                    .get(id)
                    .map(|report| hoster_priority_cap_report_changed(report, decision))
                    .unwrap_or(true);
                if should_report {
                    state.push_diagnostic_event(
                        DiagnosticLevel::Warning,
                        "download".into(),
                        format!(
                            "Hoster priority capped {id} to {} B/s to keep {} ahead; reference {} B/s, current {} B/s, target {} B/s, average {} B/s, peak {} B/s.",
                            decision.cap_bytes_per_second,
                            decision.protected_job_id,
                            decision.reference_bytes_per_second,
                            decision.current_speed,
                            decision.target_speed,
                            decision.baseline_speed,
                            decision.peak_speed
                        ),
                        Some(id.into()),
                    );
                }
                state.hoster_priority_cap_reports.insert(
                    id.to_string(),
                    HosterPriorityCapReport {
                        protected_job_id: decision.protected_job_id.clone(),
                        cap_bytes_per_second: decision.cap_bytes_per_second,
                    },
                );
            }
            None => {
                if let Some(report) = state.hoster_priority_cap_reports.remove(id) {
                    state.push_diagnostic_event(
                        DiagnosticLevel::Info,
                        "download".into(),
                        format!(
                            "Hoster priority released cap for {id}; protected worker {} left the active priority group or no longer needs a cascade cap.",
                            report.protected_job_id
                        ),
                        Some(id.into()),
                    );
                }
            }
        }
        decision
    }
}

fn hoster_priority_cap_report_changed(
    report: &HosterPriorityCapReport,
    decision: &HosterPriorityThrottleDecision,
) -> bool {
    if report.protected_job_id != decision.protected_job_id {
        return true;
    }
    let old_cap = report.cap_bytes_per_second.max(1);
    let delta = old_cap.abs_diff(decision.cap_bytes_per_second);
    delta.saturating_mul(100) >= old_cap.saturating_mul(HOSTER_PRIORITY_CAP_REPORT_CHANGE_PERCENT)
}

fn hoster_priority_throttle_decision_for_state(
    state: &RuntimeState,
    id: &str,
) -> Option<HosterPriorityThrottleDecision> {
    #[derive(Clone)]
    struct Candidate {
        id: String,
        started_at: Instant,
        reference: Option<HosterPrioritySpeedReference>,
    }

    #[derive(Clone)]
    struct EffectiveReference {
        id: String,
        current_speed: u64,
        peak_speed: u64,
        baseline_speed: u64,
        target_speed: u64,
        reference_bytes_per_second: u64,
    }

    impl EffectiveReference {
        fn from_candidate_reference(id: String, reference: HosterPrioritySpeedReference) -> Self {
            Self {
                id,
                current_speed: reference.current_speed,
                peak_speed: reference.peak_speed,
                baseline_speed: reference.baseline_speed,
                target_speed: reference.target_speed,
                reference_bytes_per_second: reference.reference_bytes_per_second,
            }
        }

        fn capped_candidate(
            id: String,
            reference: Option<HosterPrioritySpeedReference>,
            cap: u64,
        ) -> Self {
            let reference = reference.unwrap_or(HosterPrioritySpeedReference {
                current_speed: 0,
                peak_speed: 0,
                baseline_speed: 0,
                target_speed: 0,
                reference_bytes_per_second: cap,
            });
            Self {
                id,
                current_speed: reference.current_speed,
                peak_speed: reference.peak_speed,
                baseline_speed: reference.baseline_speed,
                target_speed: reference.target_speed,
                reference_bytes_per_second: cap,
            }
        }
    }

    let job = state.job(id)?;
    if state.settings.bulk.hoster_fairness_mode == BulkHosterFairnessMode::Off {
        return None;
    }
    if !state.active_workers.contains(id)
        || !matches!(job.state, JobState::Starting | JobState::Downloading)
        || !is_protected_bulk_hoster_job(job)
    {
        return None;
    }
    let priority_key = protected_bulk_hoster_priority_group_key(job)?;
    let mut candidates = state
        .active_workers
        .iter()
        .filter_map(|active_id| {
            let active_job = state.job(active_id)?;
            if !matches!(active_job.state, JobState::Starting | JobState::Downloading)
                || !is_protected_bulk_hoster_job(active_job)
                || protected_bulk_hoster_priority_group_key(active_job).as_ref()
                    != Some(&priority_key)
            {
                return None;
            }
            let health = state.bulk_hoster_worker_health.get(active_id)?;
            Some(Candidate {
                id: active_id.clone(),
                started_at: health.priority_started_at(),
                reference: health.hoster_priority_reference(),
            })
        })
        .collect::<Vec<_>>();
    if candidates.len() < 2 {
        return None;
    }
    candidates.sort_by(|left, right| {
        left.started_at
            .cmp(&right.started_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let current_index = candidates.iter().position(|candidate| candidate.id == id)?;
    if current_index == 0 {
        return None;
    }

    let mut previous_effective = candidates[0].reference.map(|reference| {
        EffectiveReference::from_candidate_reference(candidates[0].id.clone(), reference)
    });
    for candidate in candidates.iter().skip(1) {
        let Some(previous) = previous_effective.clone() else {
            if candidate.id == id {
                return None;
            }
            previous_effective = candidate.reference.map(|reference| {
                EffectiveReference::from_candidate_reference(candidate.id.clone(), reference)
            });
            continue;
        };

        let cap_bytes_per_second = previous.reference_bytes_per_second.saturating_div(2).max(1);
        if candidate.id == id {
            return Some(HosterPriorityThrottleDecision {
                protected_job_id: previous.id,
                current_speed: previous.current_speed,
                peak_speed: previous.peak_speed,
                baseline_speed: previous.baseline_speed,
                target_speed: previous.target_speed,
                reference_bytes_per_second: previous.reference_bytes_per_second,
                cap_bytes_per_second,
            });
        }
        previous_effective = Some(EffectiveReference::capped_candidate(
            candidate.id.clone(),
            candidate.reference,
            cap_bytes_per_second,
        ));
    }

    None
}

fn bulk_hoster_fairness_metrics_by_key(
    state: &RuntimeState,
    now: Instant,
) -> HashMap<String, BulkHosterFairnessMetrics> {
    #[derive(Clone)]
    struct DatanodesPriorityCandidate {
        id: String,
        started_at: Instant,
        pressure: Option<DataNodesPriorityPressureSample>,
    }

    let mut metrics_by_key = HashMap::new();
    let mut datanodes_candidates_by_key: HashMap<String, Vec<DatanodesPriorityCandidate>> =
        HashMap::new();

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
        let metrics =
            metrics_by_key
                .entry(fairness_key.clone())
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
        if is_accelerated_datanodes_bulk_job(&state.settings, job) {
            datanodes_candidates_by_key
                .entry(fairness_key)
                .or_default()
                .push(DatanodesPriorityCandidate {
                    id: id.clone(),
                    started_at: health.priority_started_at(),
                    pressure: health.datanodes_priority_pressure(now),
                });
        }
    }

    for (fairness_key, mut candidates) in datanodes_candidates_by_key {
        if candidates.len() < 2 {
            continue;
        }
        candidates.sort_by_key(|candidate| candidate.started_at);
        let Some(metrics) = metrics_by_key.get_mut(&fairness_key) else {
            continue;
        };
        for candidate in candidates.iter().take(candidates.len().saturating_sub(1)) {
            let Some(pressure) = candidate.pressure else {
                continue;
            };
            metrics.has_blocking_worker = true;
            metrics.all_healthy = false;
            metrics.priority_pressure = Some(DataNodesPriorityPressure {
                older_job_id: candidate.id.clone(),
                current_speed: pressure.current_speed,
                peak_speed: pressure.peak_speed,
                baseline_speed: pressure.baseline_speed,
                target_speed: pressure.target_speed,
            });
            break;
        }
    }

    metrics_by_key
}
