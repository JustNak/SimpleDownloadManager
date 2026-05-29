use super::*;

impl SharedState {
    pub fn new() -> Result<Self, String> {
        let data_dir_override = std::env::var("MYAPP_DATA_DIR").map(PathBuf::from).ok();
        let base_dir = data_dir_override
            .clone()
            .or_else(|| dirs::data_local_dir().map(|path| path.join("SimpleDownloadManager")))
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|path| path.join("SimpleDownloadManager"))
            })
            .unwrap_or_else(|| std::env::temp_dir().join("SimpleDownloadManager"));

        std::fs::create_dir_all(&base_dir)
            .map_err(|error| format!("Could not create app data directory: {error}"))?;
        apply_pending_torrent_session_cache_clear(&base_dir);

        let storage_path = base_dir.join("state.json");
        let storage_exists = storage_path.exists();
        let diagnostic_event_store = Arc::new(DiagnosticEventStore::new(
            diagnostic_event_log_path_for(&storage_path),
        ));
        let persisted_load = load_persisted_state_with_recovery(&storage_path);
        let allow_initial_persist = persisted_load.allow_initial_persist;
        let startup_recovery = persisted_load.startup_recovery.clone();
        let mut startup_recovery_messages = persisted_load.recovery_events;
        let mut persisted = persisted_load.state;

        if should_reset_download_directory(
            &persisted.settings.download_directory,
            data_dir_override.is_some(),
            storage_exists,
        ) {
            persisted.settings.download_directory = default_download_directory();
        }

        normalize_accent_color(&mut persisted.settings);
        normalize_extension_settings(&mut persisted.settings.extension_integration);
        normalize_torrent_settings_for_download_directory(
            &mut persisted.settings.torrent,
            &persisted.settings.download_directory,
        );
        normalize_bulk_settings_for_download_directory(
            &mut persisted.settings.bulk,
            &persisted.settings.download_directory,
        );
        startup_recovery_messages.extend(startup_directory_recovery_messages(&persisted.settings));

        let legacy_diagnostic_events = normalize_diagnostic_events(persisted.diagnostic_events);
        let file_diagnostic_events = diagnostic_event_store
            .recent_events(DIAGNOSTIC_EVENT_LIMIT)
            .unwrap_or_default();
        let mut diagnostic_events = if legacy_diagnostic_events.is_empty() {
            file_diagnostic_events
        } else if diagnostic_event_store
            .migrate_legacy_events(legacy_diagnostic_events.clone())
            .is_ok()
        {
            diagnostic_event_store
                .recent_events(DIAGNOSTIC_EVENT_LIMIT)
                .unwrap_or(legacy_diagnostic_events)
        } else {
            let mut events = file_diagnostic_events;
            events.extend(legacy_diagnostic_events);
            normalize_diagnostic_events(events)
        };
        let startup_recovery_events = startup_recovery_messages
            .into_iter()
            .map(startup_diagnostic_event)
            .collect::<Vec<_>>();
        for event in &startup_recovery_events {
            let _ = diagnostic_event_store.append(event);
        }
        diagnostic_events.extend(startup_recovery_events);
        let jobs = persisted
            .jobs
            .into_iter()
            .map(|job| {
                let was_removing = job.removal_state == Some(RemovalState::Removing);
                let job_id = job.id.clone();
                let filename = job.filename.clone();
                let normalized = normalize_job(job, &persisted.settings);
                if was_removing {
                    let event = DiagnosticEvent {
                        timestamp: current_unix_timestamp_millis(),
                        level: DiagnosticLevel::Warning,
                        category: "download".into(),
                        message: format!(
                            "Disk cleanup for {filename} was interrupted by app shutdown"
                        ),
                        job_id: Some(job_id),
                    };
                    diagnostic_events.push(event.clone());
                    let _ = diagnostic_event_store.append(&event);
                }
                normalized
            })
            .collect::<Vec<_>>();
        trim_diagnostic_events(&mut diagnostic_events);
        let next_job_number = next_job_number(&jobs);
        let job_indexes = job_indexes_for(&jobs);

        let state = Self {
            inner: Arc::new(RwLock::new(RuntimeState {
                connection_state: ConnectionState::Checking,
                jobs,
                settings: persisted.settings,
                main_window: persisted.main_window,
                diagnostic_events,
                pending_diagnostic_events: Vec::new(),
                startup_recovery,
                next_job_number,
                job_indexes,
                active_workers: HashSet::new(),
                bulk_hoster_worker_health: HashMap::new(),
                bulk_hoster_fairness: HashMap::new(),
                datanodes_priority_defer_until: HashMap::new(),
                download_admission_defers: HashMap::new(),
                hoster_priority_cap_reports: HashMap::new(),
                external_reseed_jobs: HashSet::new(),
                last_host_contact: None,
                last_progress_persist_at: None,
            })),
            storage_path: Arc::new(storage_path),
            diagnostic_event_store,
            handoff_auth: Arc::new(RwLock::new(HashMap::new())),
            scheduler_wake: Arc::new(StdMutex::new(SchedulerWakeState::default())),
        };

        if allow_initial_persist {
            state.persist_current_state_sync()?;
        }
        Ok(state)
    }

    #[cfg(test)]
    pub(crate) fn for_tests(storage_path: PathBuf, jobs: Vec<DownloadJob>) -> Self {
        let job_indexes = job_indexes_for(&jobs);
        let diagnostic_event_store = Arc::new(DiagnosticEventStore::new(
            diagnostic_event_log_path_for(&storage_path),
        ));
        Self {
            inner: Arc::new(RwLock::new(RuntimeState {
                connection_state: ConnectionState::Connected,
                jobs,
                settings: Settings::default(),
                main_window: None,
                diagnostic_events: Vec::new(),
                pending_diagnostic_events: Vec::new(),
                startup_recovery: None,
                next_job_number: 99,
                job_indexes,
                active_workers: HashSet::new(),
                bulk_hoster_worker_health: HashMap::new(),
                bulk_hoster_fairness: HashMap::new(),
                datanodes_priority_defer_until: HashMap::new(),
                download_admission_defers: HashMap::new(),
                hoster_priority_cap_reports: HashMap::new(),
                external_reseed_jobs: HashSet::new(),
                last_host_contact: None,
                last_progress_persist_at: None,
            })),
            storage_path: Arc::new(storage_path),
            diagnostic_event_store,
            handoff_auth: Arc::new(RwLock::new(HashMap::new())),
            scheduler_wake: Arc::new(StdMutex::new(SchedulerWakeState::default())),
        }
    }

    pub async fn snapshot(&self) -> DesktopSnapshot {
        let state = self.inner.read().await;
        state.snapshot()
    }

    pub async fn set_connection_state(
        &self,
        connection_state: ConnectionState,
    ) -> Result<DesktopSnapshot, String> {
        let snapshot = {
            let mut state = self.inner.write().await;
            state.connection_state = connection_state;
            state.snapshot()
        };

        Ok(snapshot)
    }

    pub async fn register_host_contact(&self) -> DesktopSnapshot {
        let mut state = self.inner.write().await;
        state.last_host_contact = Some(Instant::now());
        state.connection_state = ConnectionState::Connected;
        state.snapshot()
    }

    pub async fn has_recent_host_contact(&self, ttl: Duration) -> bool {
        let state = self.inner.read().await;
        state
            .last_host_contact
            .map(|last_seen| last_seen.elapsed() <= ttl)
            .unwrap_or(false)
    }

    pub async fn queue_summary(&self) -> QueueSummary {
        let state = self.inner.read().await;
        state.queue_summary()
    }

    pub async fn connection_state(&self) -> ConnectionState {
        let state = self.inner.read().await;
        state.connection_state
    }

    pub async fn notifications_enabled(&self) -> bool {
        let state = self.inner.read().await;
        state.settings.notifications_enabled
    }

    pub async fn notification_sounds_enabled(&self) -> bool {
        let state = self.inner.read().await;
        state.settings.notifications_enabled && state.settings.notification_sounds_enabled
    }

    pub async fn auto_retry_attempts(&self) -> u32 {
        let state = self.inner.read().await;
        state.settings.auto_retry_attempts.min(10)
    }

    pub async fn auto_retry_attempts_for_job(&self, id: &str) -> u32 {
        let state = self.inner.read().await;
        state
            .job(id)
            .map(|job| max_auto_retry_attempts_for_job(&state.settings, job))
            .unwrap_or_else(|| state.settings.auto_retry_attempts.min(10))
    }

    pub async fn speed_limit_bytes_per_second(&self) -> Option<u64> {
        self.speed_limit_bytes_per_second_for_task(false).await
    }

    pub async fn speed_limit_bytes_per_second_for_task(&self, is_bulk_member: bool) -> Option<u64> {
        let state = self.inner.read().await;
        let limit = if is_bulk_member {
            state.settings.bulk.speed_limit_kib_per_second
        } else {
            state.settings.speed_limit_kib_per_second
        };
        if limit == 0 {
            None
        } else {
            Some((limit as u64).saturating_mul(1024))
        }
    }

    pub async fn download_performance_mode(&self) -> DownloadPerformanceMode {
        self.download_performance_mode_for_task(false).await
    }

    pub async fn download_performance_mode_for_task(
        &self,
        is_bulk_member: bool,
    ) -> DownloadPerformanceMode {
        let state = self.inner.read().await;
        if is_bulk_member {
            state.settings.bulk.download_performance_mode
        } else {
            state.settings.download_performance_mode
        }
    }

    pub async fn bulk_hoster_acceleration_mode(&self) -> BulkHosterAccelerationMode {
        let state = self.inner.read().await;
        state.settings.bulk.hoster_acceleration_mode
    }

    pub async fn datanodes_hoster_warmup_candidates(&self) -> Vec<HosterWarmupCandidate> {
        let state = self.inner.read().await;
        let Some(limit) = datanodes_hoster_warmup_horizon(&state.settings) else {
            return Vec::new();
        };
        let now = Instant::now();
        let priority_blocked_keys = state
            .active_workers
            .iter()
            .filter_map(|id| {
                let job = state.job(id)?;
                if !is_accelerated_datanodes_bulk_job(&state.settings, job) {
                    return None;
                }
                let health = state.bulk_hoster_worker_health.get(id)?;
                health
                    .datanodes_priority_pressure(now)
                    .and_then(|_| protected_bulk_hoster_fairness_key(job))
            })
            .collect::<HashSet<_>>();

        state
            .jobs
            .iter()
            .filter(|job| {
                job.state == JobState::Queued
                    && !state.active_workers.contains(&job.id)
                    && is_protected_bulk_hoster_job(job)
                    && !job
                        .hoster_preflight
                        .as_ref()
                        .is_some_and(|preflight| preflight.status == HosterPreflightStatus::Failed)
                    && datanodes_accelerated_hoster_concurrency(&state.settings, job).is_some()
                    && protected_bulk_hoster_fairness_key(job)
                        .map(|key| !priority_blocked_keys.contains(&key))
                        .unwrap_or(true)
            })
            .filter_map(|job| {
                let source_url = job.resolved_from_url.as_ref()?.clone();
                Some(HosterWarmupCandidate {
                    job_id: job.id.clone(),
                    source_url,
                })
            })
            .take(limit)
            .collect()
    }

    pub async fn active_direct_bulk_worker_counts(
        &self,
        job_id: &str,
        effective_url: &str,
    ) -> (usize, usize) {
        let state = self.inner.read().await;
        let target_origin = download_origin_key(effective_url);
        let mut total = 0_usize;
        let mut same_origin = 0_usize;

        for active_id in &state.active_workers {
            let Some(job) = state.job(active_id) else {
                continue;
            };
            if !is_direct_bulk_http_job(job)
                || !matches!(job.state, JobState::Starting | JobState::Downloading)
            {
                continue;
            }

            total += 1;
            if active_id == job_id
                || target_origin
                    .as_ref()
                    .is_some_and(|origin| download_origin_key(&job.url).as_ref() == Some(origin))
            {
                same_origin += 1;
            }
        }

        if !state.active_workers.contains(job_id) {
            if let Some(job) = state.job(job_id).filter(|job| is_direct_bulk_http_job(job)) {
                total += 1;
                if target_origin
                    .as_ref()
                    .is_some_and(|origin| download_origin_key(&job.url).as_ref() == Some(origin))
                {
                    same_origin += 1;
                }
            }
        }

        (total, same_origin)
    }

    pub async fn active_protected_hoster_bulk_worker_counts(
        &self,
        job_id: &str,
        effective_url: &str,
    ) -> (usize, usize) {
        let state = self.inner.read().await;
        let target_origin = download_origin_key(effective_url);
        let mut total = 0_usize;
        let mut same_origin = 0_usize;

        for active_id in &state.active_workers {
            let Some(job) = state.job(active_id) else {
                continue;
            };
            if !is_protected_bulk_hoster_job(job)
                || !matches!(job.state, JobState::Starting | JobState::Downloading)
            {
                continue;
            }

            total += 1;
            if active_id == job_id
                || target_origin
                    .as_ref()
                    .is_some_and(|origin| download_origin_key(&job.url).as_ref() == Some(origin))
            {
                same_origin += 1;
            }
        }

        if !state.active_workers.contains(job_id) {
            if let Some(job) = state
                .job(job_id)
                .filter(|job| is_protected_bulk_hoster_job(job))
            {
                total += 1;
                if target_origin
                    .as_ref()
                    .is_some_and(|origin| download_origin_key(&job.url).as_ref() == Some(origin))
                {
                    same_origin += 1;
                }
            }
        }

        (total, same_origin)
    }

    pub async fn extension_integration_settings(&self) -> ExtensionIntegrationSettings {
        let state = self.inner.read().await;
        state.settings.extension_integration.clone()
    }

    pub async fn appearance_settings(&self) -> crate::storage::AppearanceSettings {
        let state = self.inner.read().await;
        crate::storage::AppearanceSettings {
            theme: state.settings.theme.clone(),
            accent_color: state.settings.accent_color.clone(),
        }
    }

    pub async fn show_progress_after_handoff(&self) -> bool {
        let state = self.inner.read().await;
        state
            .settings
            .extension_integration
            .show_progress_after_handoff
    }

    pub async fn diagnostics_snapshot(
        &self,
        host_registration: HostRegistrationDiagnostics,
    ) -> DiagnosticsSnapshot {
        let state = self.inner.read().await;
        DiagnosticsSnapshot {
            connection_state: state.connection_state,
            queue_summary: state.queue_summary(),
            last_host_contact_seconds_ago: state
                .last_host_contact
                .map(|last_seen| last_seen.elapsed().as_secs()),
            host_registration,
            torrent_diagnostics: state.torrent_diagnostics_snapshot(),
            recent_events: state.diagnostic_events.clone(),
        }
    }

    pub async fn diagnostics_export(
        &self,
        host_registration: HostRegistrationDiagnostics,
    ) -> DiagnosticsExport {
        let snapshot = self.diagnostics_snapshot(host_registration).await;
        let event_history = self.diagnostic_event_history().await.unwrap_or_default();
        DiagnosticsExport {
            snapshot,
            event_history,
        }
    }

    pub async fn diagnostic_event_history(&self) -> Result<Vec<DiagnosticEvent>, String> {
        self.diagnostic_event_store.retained_events()
    }

    pub(super) async fn append_diagnostic_events_blocking(&self, events: Vec<DiagnosticEvent>) {
        if events.is_empty() {
            return;
        }

        let diagnostic_event_store = Arc::clone(&self.diagnostic_event_store);
        let _ = tokio::task::spawn_blocking(move || {
            append_diagnostic_events_best_effort(diagnostic_event_store, events);
        })
        .await;
    }

    pub(super) fn append_diagnostic_events_in_background(&self, events: Vec<DiagnosticEvent>) {
        if events.is_empty() {
            return;
        }

        let diagnostic_event_store = Arc::clone(&self.diagnostic_event_store);
        drop(tokio::task::spawn_blocking(move || {
            append_diagnostic_events_best_effort(diagnostic_event_store, events);
        }));
    }

    pub async fn record_diagnostic_event(
        &self,
        level: DiagnosticLevel,
        category: impl Into<String>,
        message: impl Into<String>,
        job_id: Option<String>,
    ) -> Result<(), String> {
        let events = {
            let mut state = self.inner.write().await;
            state.push_diagnostic_event(level, category.into(), message.into(), job_id);
            state.take_pending_diagnostic_events()
        };
        self.append_diagnostic_events_blocking(events).await;

        Ok(())
    }

    pub fn app_data_dir(&self) -> PathBuf {
        self.storage_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir().join("SimpleDownloadManager"))
    }

    pub(super) fn persist_current_state_sync(&self) -> Result<(), String> {
        let state = self.inner.blocking_read();
        persist_state(&self.storage_path, &state.persisted())
    }
}

fn append_diagnostic_events_best_effort(
    diagnostic_event_store: Arc<DiagnosticEventStore>,
    events: Vec<DiagnosticEvent>,
) {
    for event in events {
        let _ = diagnostic_event_store.append(&event);
    }
}

fn startup_diagnostic_event(message: String) -> DiagnosticEvent {
    DiagnosticEvent {
        timestamp: current_unix_timestamp_millis(),
        level: DiagnosticLevel::Warning,
        category: "startup".into(),
        message,
        job_id: None,
    }
}

fn startup_directory_recovery_messages(settings: &Settings) -> Vec<String> {
    [
        (
            "saved download directory",
            settings.download_directory.as_str(),
        ),
        (
            "saved torrent download directory",
            settings.torrent.download_directory.as_str(),
        ),
        (
            "saved bulk output directory",
            settings.bulk.output_directory.as_str(),
        ),
    ]
    .into_iter()
    .filter_map(|(label, value)| startup_directory_recovery_message(label, value))
    .collect()
}

fn startup_directory_recovery_message(label: &str, value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = Path::new(trimmed);
    if path.is_dir() {
        return None;
    }
    if path.exists() {
        return Some(format!(
            "The {label} is not a directory and was left unchanged for startup: {}",
            path.display()
        ));
    }

    let blocking_ancestor = path
        .ancestors()
        .skip(1)
        .find(|ancestor| ancestor.exists() && !ancestor.is_dir())?;
    Some(format!(
        "The {label} is unavailable because {} is not a directory; startup continued without creating {}",
        blocking_ancestor.display(),
        path.display()
    ))
}
