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
        let mut persisted = load_persisted_state(&storage_path)?;

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
        ensure_download_category_directories(Path::new(&persisted.settings.download_directory))?;
        std::fs::create_dir_all(&persisted.settings.torrent.download_directory)
            .map_err(|error| format!("Could not create torrent download directory: {error}"))?;

        let jobs = persisted
            .jobs
            .into_iter()
            .map(|job| normalize_job(job, &persisted.settings))
            .collect::<Vec<_>>();
        let diagnostic_events = normalize_diagnostic_events(persisted.diagnostic_events);
        let next_job_number = next_job_number(&jobs);

        let state = Self {
            inner: Arc::new(RwLock::new(RuntimeState {
                connection_state: ConnectionState::Checking,
                jobs,
                settings: persisted.settings,
                main_window: persisted.main_window,
                diagnostic_events,
                next_job_number,
                active_workers: HashSet::new(),
                external_reseed_jobs: HashSet::new(),
                last_host_contact: None,
            })),
            storage_path: Arc::new(storage_path),
            handoff_auth: Arc::new(RwLock::new(HashMap::new())),
        };

        state.persist_current_state_sync()?;
        Ok(state)
    }

    #[cfg(test)]
    pub(crate) fn for_tests(storage_path: PathBuf, jobs: Vec<DownloadJob>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(RuntimeState {
                connection_state: ConnectionState::Connected,
                jobs,
                settings: Settings::default(),
                main_window: None,
                diagnostic_events: Vec::new(),
                next_job_number: 99,
                active_workers: HashSet::new(),
                external_reseed_jobs: HashSet::new(),
                last_host_contact: None,
            })),
            storage_path: Arc::new(storage_path),
            handoff_auth: Arc::new(RwLock::new(HashMap::new())),
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

    pub async fn auto_retry_attempts(&self) -> u32 {
        let state = self.inner.read().await;
        state.settings.auto_retry_attempts.min(10)
    }

    pub async fn speed_limit_bytes_per_second(&self) -> Option<u64> {
        let state = self.inner.read().await;
        let limit = state.settings.speed_limit_kib_per_second;
        if limit == 0 {
            None
        } else {
            Some((limit as u64).saturating_mul(1024))
        }
    }

    pub async fn download_performance_mode(&self) -> DownloadPerformanceMode {
        let state = self.inner.read().await;
        state.settings.download_performance_mode
    }

    pub async fn extension_integration_settings(&self) -> ExtensionIntegrationSettings {
        let state = self.inner.read().await;
        state.settings.extension_integration.clone()
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
            recent_events: state.diagnostic_events.clone(),
        }
    }

    pub async fn record_diagnostic_event(
        &self,
        level: DiagnosticLevel,
        category: impl Into<String>,
        message: impl Into<String>,
        job_id: Option<String>,
    ) -> Result<(), String> {
        let persisted = {
            let mut state = self.inner.write().await;
            state.push_diagnostic_event(level, category.into(), message.into(), job_id);
            state.persisted()
        };

        persist_state(&self.storage_path, &persisted)
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
