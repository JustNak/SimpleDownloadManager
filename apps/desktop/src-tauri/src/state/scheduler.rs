use super::*;
use crate::archive_parts::detect_archive_part;

#[derive(Debug)]
pub(super) struct SchedulerAdmissionIndex {
    accelerated_bulk_slot_floor: u32,
    protected_hoster_max_adaptive_concurrency_by_key: HashMap<String, u32>,
    protected_bulk_archive_groups: HashMap<(String, String), Vec<ProtectedBulkArchiveMember>>,
    scheduler_job_order: Vec<usize>,
}

#[derive(Debug)]
enum SchedulerOrderEntry {
    Job(usize),
    ProtectedGroup((String, String)),
}

impl SchedulerAdmissionIndex {
    pub(super) fn new(state: &RuntimeState) -> Self {
        let mut accelerated_bulk_slot_floor = 0_u32;
        let mut protected_hoster_max_adaptive_concurrency_by_key: HashMap<String, u32> =
            HashMap::new();
        let mut protected_bulk_archive_groups: HashMap<
            (String, String),
            Vec<ProtectedBulkArchiveMember>,
        > = HashMap::new();
        let mut order_entries = Vec::with_capacity(state.jobs.len());
        let mut emitted_protected_groups = HashSet::new();

        for (index, job) in state.jobs.iter().enumerate() {
            if is_bulk_member_job(job) {
                if let Some(max_concurrency) = accelerated_hoster_concurrency(&state.settings, job)
                {
                    accelerated_bulk_slot_floor = accelerated_bulk_slot_floor.max(max_concurrency);
                }
            }

            if let Some(fairness_key) = protected_bulk_hoster_fairness_key(job) {
                let max_concurrency = accelerated_hoster_concurrency(&state.settings, job)
                    .unwrap_or(BULK_HOSTER_MAX_ADAPTIVE_CONCURRENCY);
                protected_hoster_max_adaptive_concurrency_by_key
                    .entry(fairness_key)
                    .and_modify(|current| *current = (*current).max(max_concurrency))
                    .or_insert(max_concurrency);
            }

            let Some(group_key) = protected_bulk_hoster_priority_group_key(job) else {
                order_entries.push(SchedulerOrderEntry::Job(index));
                continue;
            };

            protected_bulk_archive_groups
                .entry(group_key.clone())
                .or_default()
                .push(ProtectedBulkArchiveMember::from_job(index, job));
            if emitted_protected_groups.insert(group_key.clone()) {
                order_entries.push(SchedulerOrderEntry::ProtectedGroup(group_key));
            }
        }

        for members in protected_bulk_archive_groups.values_mut() {
            sort_protected_bulk_archive_members(members);
        }

        let mut scheduler_job_order = Vec::with_capacity(state.jobs.len());
        for entry in order_entries {
            match entry {
                SchedulerOrderEntry::Job(index) => scheduler_job_order.push(index),
                SchedulerOrderEntry::ProtectedGroup(group_key) => {
                    if let Some(members) = protected_bulk_archive_groups.get(&group_key) {
                        scheduler_job_order.extend(members.iter().map(|member| member.index));
                    }
                }
            }
        }

        Self {
            accelerated_bulk_slot_floor,
            protected_hoster_max_adaptive_concurrency_by_key,
            protected_bulk_archive_groups,
            scheduler_job_order,
        }
    }

    pub(super) fn accelerated_bulk_slot_floor(&self) -> u32 {
        self.accelerated_bulk_slot_floor
    }

    pub(super) fn max_adaptive_concurrency_for_key(&self, fairness_key: &str) -> u32 {
        self.protected_hoster_max_adaptive_concurrency_by_key
            .get(fairness_key)
            .copied()
            .unwrap_or(BULK_HOSTER_MAX_ADAPTIVE_CONCURRENCY)
    }

    fn protected_hoster_fairness_keys(&self) -> impl Iterator<Item = &String> {
        self.protected_hoster_max_adaptive_concurrency_by_key.keys()
    }

    fn protected_bulk_archive_groups(
        &self,
    ) -> &HashMap<(String, String), Vec<ProtectedBulkArchiveMember>> {
        &self.protected_bulk_archive_groups
    }

    pub(super) fn scheduler_job_order(&self) -> &[usize] {
        &self.scheduler_job_order
    }
}

impl SharedState {
    pub(crate) fn request_scheduler_wake(&self) -> bool {
        let mut scheduler_wake = self
            .scheduler_wake
            .lock()
            .expect("scheduler wake lock poisoned");
        if scheduler_wake.running {
            scheduler_wake.pending = true;
            return false;
        }

        scheduler_wake.running = true;
        true
    }

    pub(crate) fn complete_scheduler_run(&self) -> bool {
        let mut scheduler_wake = self
            .scheduler_wake
            .lock()
            .expect("scheduler wake lock poisoned");
        if scheduler_wake.pending {
            scheduler_wake.pending = false;
            return true;
        }

        scheduler_wake.running = false;
        false
    }

    pub async fn claim_schedulable_jobs(
        &self,
    ) -> Result<(DesktopSnapshot, Vec<DownloadTask>), String> {
        let claim = self.claim_schedulable_jobs_for_scheduler().await?;
        let snapshot = match claim.snapshot {
            Some(snapshot) => snapshot,
            None => self.snapshot().await,
        };

        Ok((snapshot, claim.tasks))
    }

    pub async fn claim_schedulable_jobs_for_scheduler(&self) -> Result<SchedulableClaim, String> {
        let auth_by_job = self.handoff_auth.read().await.clone();
        let (claim, persisted, diagnostic_events) = {
            let mut state = self.inner.write().await;
            let now = Instant::now();
            let admission_index = SchedulerAdmissionIndex::new(&state);
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
            let bulk_slot_limit = state
                .settings
                .bulk
                .max_concurrent_downloads
                .max(1)
                .max(admission_index.accelerated_bulk_slot_floor());
            let available_normal_slots = normal_slot_limit.saturating_sub(active_normal_workers);
            let available_bulk_slots = bulk_slot_limit.saturating_sub(active_bulk_workers);

            if available_normal_slots == 0 && available_bulk_slots == 0 {
                return Ok(SchedulableClaim {
                    snapshot: None,
                    tasks: Vec::new(),
                });
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
                        admission_index.max_adaptive_concurrency_for_key(key);
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
            state.retain_active_download_admission_defers(now);
            let mut protected_bulk_hoster_targets = HashMap::new();
            for key in admission_index
                .protected_hoster_fairness_keys()
                .cloned()
                .chain(fairness_metrics_by_key.keys().cloned())
            {
                let max_adaptive_concurrency =
                    admission_index.max_adaptive_concurrency_for_key(&key);
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
            let (protected_bulk_archive_windows, archive_window_diagnostics) =
                protected_bulk_archive_scheduling_windows(
                    &state,
                    admission_index.protected_bulk_archive_groups(),
                    &fairness_metrics_by_key,
                    &protected_bulk_hoster_targets,
                    bulk_slot_limit,
                    fairness_mode,
                );
            for message in archive_window_diagnostics {
                state.push_diagnostic_event(
                    DiagnosticLevel::Info,
                    "download".into(),
                    message,
                    None,
                );
            }
            let mut scheduled_protected_bulk_hoster_workers: HashMap<String, u32> = HashMap::new();
            let mut blocked_accelerated_hoster_queue_keys: HashSet<String> = HashSet::new();
            let mut blocked_admission_hoster_queue_keys: HashSet<String> = HashSet::new();
            let mut admission_defer_diagnostics = Vec::new();
            for &job_index in admission_index.scheduler_job_order() {
                let job = &state.jobs[job_index];
                if job.transfer_kind == TransferKind::BrowserAdopted {
                    continue;
                }
                if scheduled_normal_workers >= available_normal_slots
                    && scheduled_bulk_workers >= available_bulk_slots
                {
                    break;
                }

                if job.state == JobState::Queued && !state.active_workers.contains(&job.id) {
                    let admission_defer = state.download_admission_defers.get(&job.id);
                    let admission_deferred = admission_defer.is_some();
                    if is_bulk_member_job(job) {
                        if scheduled_bulk_workers >= available_bulk_slots {
                            continue;
                        }
                        if let Some(fairness_key) = protected_bulk_hoster_fairness_key(job) {
                            if blocked_admission_hoster_queue_keys.contains(&fairness_key) {
                                continue;
                            }
                            if admission_deferred {
                                if let Some(defer) = admission_defer {
                                    admission_defer_diagnostics.push(format!(
                                        "Scheduler kept {} queued for {} seconds: {}",
                                        job.id,
                                        defer.until.saturating_duration_since(now).as_secs().max(1),
                                        defer.reason
                                    ));
                                }
                                blocked_admission_hoster_queue_keys.insert(fairness_key);
                                continue;
                            }
                            let fairness_metrics = fairness_metrics_by_key
                                .get(&fairness_key)
                                .cloned()
                                .unwrap_or_default();
                            let archive_window = protected_bulk_hoster_priority_group_key(job)
                                .and_then(|key| protected_bulk_archive_windows.get(&key));
                            let accelerated_hoster =
                                is_accelerated_hoster_bulk_job(&state.settings, job);
                            if active_finish_protected_bulk_hoster_blocks_claim(
                                &state, job_index, job,
                            ) {
                                continue;
                            }
                            if archive_window
                                .is_some_and(|window| !window.allowed_job_ids.contains(&job.id))
                            {
                                continue;
                            }
                            if accelerated_hoster {
                                if blocked_accelerated_hoster_queue_keys.contains(&fairness_key) {
                                    continue;
                                }
                                if state.datanodes_priority_defer_until.contains_key(&job.id) {
                                    blocked_accelerated_hoster_queue_keys.insert(fairness_key);
                                    continue;
                                }
                            }
                            let protected_bulk_hoster_claim_blocked = fairness_mode
                                != BulkHosterFairnessMode::Off
                                && fairness_metrics.has_blocking_worker
                                && !archive_window.is_some_and(|window| window.throughput_rescue);
                            let scheduled_for_origin = scheduled_protected_bulk_hoster_workers
                                .get(&fairness_key)
                                .copied()
                                .unwrap_or(0);
                            let protected_bulk_hoster_target = protected_bulk_hoster_targets
                                .get(&fairness_key)
                                .copied()
                                .unwrap_or(1);
                            let protected_bulk_hoster_target = archive_window
                                .map(|window| window.target_active)
                                .unwrap_or(protected_bulk_hoster_target);
                            if protected_bulk_hoster_claim_blocked
                                || fairness_metrics.active_count + scheduled_for_origin
                                    >= protected_bulk_hoster_target
                                || (accelerated_hoster
                                    && scheduled_for_origin > 0
                                    && !archive_window
                                        .is_some_and(|window| window.throughput_rescue))
                            {
                                if accelerated_hoster {
                                    blocked_accelerated_hoster_queue_keys.insert(fairness_key);
                                }
                                continue;
                            }
                            *scheduled_protected_bulk_hoster_workers
                                .entry(fairness_key)
                                .or_default() += 1;
                        } else if admission_deferred {
                            if let Some(defer) = admission_defer {
                                admission_defer_diagnostics.push(format!(
                                    "Scheduler kept {} queued for {} seconds: {}",
                                    job.id,
                                    defer.until.saturating_duration_since(now).as_secs().max(1),
                                    defer.reason
                                ));
                            }
                            continue;
                        }
                        scheduled_bulk_workers += 1;
                    } else {
                        if scheduled_normal_workers >= available_normal_slots {
                            continue;
                        }
                        if admission_deferred {
                            if let Some(defer) = admission_defer {
                                admission_defer_diagnostics.push(format!(
                                    "Scheduler kept {} queued for {} seconds: {}",
                                    job.id,
                                    defer.until.saturating_duration_since(now).as_secs().max(1),
                                    defer.reason
                                ));
                            }
                            continue;
                        }
                        scheduled_normal_workers += 1;
                    }
                    scheduled_ids.push(job.id.clone());
                }
            }
            if scheduled_ids.is_empty() {
                for message in admission_defer_diagnostics.into_iter().take(3) {
                    state.push_diagnostic_event(
                        DiagnosticLevel::Info,
                        "download".into(),
                        message,
                        None,
                    );
                }
            }

            let mut tasks = Vec::new();
            let settings_for_claim = state.settings.clone();
            for scheduled_id in scheduled_ids {
                state.download_admission_defers.remove(&scheduled_id);
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
                        source: job.source.clone(),
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
                    let accelerated_hoster =
                        is_accelerated_hoster_bulk_job(&settings_for_claim, job);
                    let _ = job;
                    state.active_workers.insert(task_id);
                    if let Some(health) = hoster_health {
                        state
                            .bulk_hoster_worker_health
                            .insert(task.id.clone(), health);
                        if accelerated_hoster {
                            state.push_diagnostic_event(
                                DiagnosticLevel::Info,
                                "download".into(),
                                format!(
                                    "Accelerated hoster admitted {} after healthy runway.",
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

            let has_tasks = !tasks.is_empty();
            let diagnostic_events = state.take_pending_diagnostic_events();
            (
                SchedulableClaim {
                    snapshot: has_tasks.then(|| state.snapshot()),
                    tasks,
                },
                has_tasks.then(|| state.persisted()),
                diagnostic_events,
            )
        };

        if let Some(persisted) = persisted {
            persist_state(&self.storage_path, &persisted)?;
        }
        self.append_diagnostic_events_in_background(diagnostic_events);

        Ok(claim)
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

        if job.removal_state == Some(RemovalState::Removing) {
            return WorkerControl::Canceled;
        }

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
        let (decision, diagnostic_events) = {
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
            let diagnostic_events = state.take_pending_diagnostic_events();
            (decision, diagnostic_events)
        };
        self.append_diagnostic_events_in_background(diagnostic_events);
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
    if is_accelerated_hoster_bulk_job(&state.settings, job) {
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

#[derive(Debug, Clone)]
struct ProtectedBulkArchiveMember {
    index: usize,
    id: String,
    state: JobState,
    detected_rank: u8,
    part_key: String,
    part_number: u32,
    created_at: u64,
}

impl ProtectedBulkArchiveMember {
    fn from_job(index: usize, job: &DownloadJob) -> Self {
        let detected_part = detect_archive_part(&job.filename);
        let (detected_rank, part_key, part_number) = detected_part
            .map(|part| (0, part.key, part.part_number))
            .unwrap_or_else(|| (1, String::new(), u32::try_from(index).unwrap_or(u32::MAX)));

        Self {
            index,
            id: job.id.clone(),
            state: job.state,
            detected_rank,
            part_key,
            part_number,
            created_at: job.created_at,
        }
    }
}

#[derive(Debug, Clone)]
struct ProtectedBulkArchiveWindow {
    allowed_job_ids: HashSet<String>,
    target_active: u32,
    throughput_rescue: bool,
}

fn protected_bulk_archive_scheduling_windows(
    state: &RuntimeState,
    groups: &HashMap<(String, String), Vec<ProtectedBulkArchiveMember>>,
    fairness_metrics_by_key: &HashMap<String, BulkHosterFairnessMetrics>,
    protected_bulk_hoster_targets: &HashMap<String, u32>,
    bulk_slot_limit: u32,
    fairness_mode: BulkHosterFairnessMode,
) -> (
    HashMap<(String, String), ProtectedBulkArchiveWindow>,
    Vec<String>,
) {
    let mut windows = HashMap::new();
    let mut diagnostics = Vec::new();
    for (group_key, members) in groups {
        let fairness_key = &group_key.1;
        let metrics = fairness_metrics_by_key
            .get(fairness_key)
            .cloned()
            .unwrap_or_default();
        let configured_target = protected_bulk_hoster_targets
            .get(fairness_key)
            .copied()
            .unwrap_or(1)
            .max(1);
        let performance_cap = members
            .iter()
            .filter_map(|member| state.jobs.get(member.index))
            .filter_map(|job| accelerated_hoster_concurrency(&state.settings, job))
            .max()
            .unwrap_or(configured_target)
            .max(1);
        let has_queued_member = members
            .iter()
            .any(|member| member.state == JobState::Queued);
        let throughput_rescue = fairness_mode == BulkHosterFairnessMode::Adaptive
            && has_queued_member
            && !metrics.has_blocking_worker
            && metrics.active_count > 0
            && metrics.aggregate_speed > 0
            && metrics.aggregate_speed < BULK_HOSTER_THROUGHPUT_RESCUE_FLOOR_BYTES_PER_SECOND
            && performance_cap > configured_target;
        let target_active = if throughput_rescue {
            configured_target.max(performance_cap)
        } else {
            configured_target
        }
        .min(bulk_slot_limit.max(1))
        .max(1);

        let Some(first_unfinished_index) = members
            .iter()
            .position(|member| is_unfinished_bulk_archive_member(member.state))
        else {
            continue;
        };
        let allowed_job_ids = members
            .iter()
            .skip(first_unfinished_index)
            .take(target_active as usize)
            .map(|member| member.id.clone())
            .collect::<HashSet<_>>();

        if throughput_rescue {
            diagnostics.push(format!(
                "Throughput rescue expanded protected bulk archive {} on {} to {} active files at {} B/s.",
                group_key.0, group_key.1, target_active, metrics.aggregate_speed
            ));
        } else {
            diagnostics.push(format!(
                "Archive-aware protected bulk window for {} on {} allows {} active files at {} B/s.",
                group_key.0, group_key.1, target_active, metrics.aggregate_speed
            ));
        }

        windows.insert(
            group_key.clone(),
            ProtectedBulkArchiveWindow {
                allowed_job_ids,
                target_active,
                throughput_rescue,
            },
        );
    }

    (windows, diagnostics)
}

fn active_finish_protected_bulk_hoster_blocks_claim(
    state: &RuntimeState,
    candidate_index: usize,
    candidate: &DownloadJob,
) -> bool {
    let Some(candidate_key) = protected_bulk_hoster_priority_group_key(candidate) else {
        return false;
    };

    state.active_workers.iter().any(|active_id| {
        let Some(active_index) = state.job_index(active_id) else {
            return false;
        };
        if active_index >= candidate_index {
            return false;
        }
        let Some(active_job) = state.jobs.get(active_index) else {
            return false;
        };
        matches!(active_job.state, JobState::Starting | JobState::Downloading)
            && protected_bulk_hoster_priority_group_key(active_job).as_ref() == Some(&candidate_key)
            && bulk_member_is_close_to_finish(active_job)
    })
}

fn bulk_member_is_close_to_finish(job: &DownloadJob) -> bool {
    if job.total_bytes < BULK_HOSTER_FINISH_PROTECTION_MIN_TOTAL_BYTES
        || job.downloaded_bytes >= job.total_bytes
    {
        return false;
    }

    let remaining_bytes = job.total_bytes.saturating_sub(job.downloaded_bytes);
    if remaining_bytes <= BULK_HOSTER_FINISH_PROTECTION_REMAINING_BYTES {
        return true;
    }

    job.speed > 0
        && remaining_bytes
            .saturating_add(job.speed.saturating_sub(1))
            .saturating_div(job.speed)
            <= BULK_HOSTER_FINISH_PROTECTION_ETA
}

fn sort_protected_bulk_archive_members(members: &mut [ProtectedBulkArchiveMember]) {
    members.sort_by(|left, right| {
        (
            left.detected_rank,
            left.part_key.as_str(),
            left.part_number,
            left.created_at,
            left.index,
        )
            .cmp(&(
                right.detected_rank,
                right.part_key.as_str(),
                right.part_number,
                right.created_at,
                right.index,
            ))
    });
}

fn is_unfinished_bulk_archive_member(state: JobState) -> bool {
    !matches!(state, JobState::Completed | JobState::Canceled)
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
        if is_accelerated_hoster_bulk_job(&state.settings, job) {
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
