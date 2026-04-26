use crate::storage::{
    default_download_directory, default_extension_listen_port, load_persisted_state, persist_state,
    BulkArchiveInfo, BulkArchiveStatus, ConnectionState, DesktopSnapshot, DiagnosticEvent,
    DiagnosticLevel, DiagnosticsSnapshot, DownloadJob, DownloadPerformanceMode, DownloadPrompt,
    DownloadSource, ExtensionIntegrationSettings, FailureCategory, HostRegistrationDiagnostics,
    IntegrityAlgorithm, IntegrityCheck, IntegrityStatus, JobState, MainWindowState, PersistedState,
    QueueSummary, ResumeSupport, Settings, TransferKind,
};
use percent_encoding::percent_decode_str;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use url::Url;

const MAX_URL_LENGTH: usize = 2048;
const SHA256_HEX_LENGTH: usize = 64;
const DIAGNOSTIC_EVENT_LIMIT: usize = 100;
const DOWNLOAD_CATEGORY_FOLDERS: [&str; 7] = [
    "Document",
    "Program",
    "Picture",
    "Video",
    "Compressed",
    "Music",
    "Other",
];
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "csv", "md", "epub",
];
const PROGRAM_EXTENSIONS: &[&str] = &["exe", "msi", "apk", "dmg", "pkg", "deb", "rpm", "appimage"];
const PICTURE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "tif", "tiff", "heic",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv", "flv"];
const COMPRESSED_EXTENSIONS: &[&str] = &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "tgz"];
const MUSIC_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "ogg", "m4a", "aac", "opus", "wma"];

#[derive(Debug, Clone)]
pub struct BackendError {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub transfer_kind: TransferKind,
    pub target_path: PathBuf,
    pub temp_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct BulkArchiveReady {
    pub archive_id: String,
    pub output_path: PathBuf,
    pub entries: Vec<BulkArchiveEntry>,
}

#[derive(Debug, Clone)]
pub struct BulkArchiveEntry {
    pub source_path: PathBuf,
    pub archive_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueStatus {
    Queued,
    DuplicateExistingJob,
}

impl EnqueueStatus {
    pub fn as_protocol_value(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::DuplicateExistingJob => "duplicate_existing_job",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DuplicatePolicy {
    #[default]
    ReturnExisting,
    Allow,
}

#[derive(Debug, Clone, Default)]
pub struct EnqueueOptions {
    pub source: Option<DownloadSource>,
    pub directory_override: Option<String>,
    pub filename_hint: Option<String>,
    pub expected_sha256: Option<String>,
    pub duplicate_policy: DuplicatePolicy,
    pub bulk_archive: Option<BulkArchiveInfo>,
}

#[derive(Debug, Clone)]
pub struct EnqueueResult {
    pub snapshot: DesktopSnapshot,
    pub job_id: String,
    pub filename: String,
    pub status: EnqueueStatus,
}

#[derive(Debug)]
struct RuntimeState {
    connection_state: ConnectionState,
    jobs: Vec<DownloadJob>,
    settings: Settings,
    main_window: Option<MainWindowState>,
    diagnostic_events: Vec<DiagnosticEvent>,
    next_job_number: u64,
    active_workers: HashSet<String>,
    last_host_contact: Option<Instant>,
}

#[derive(Clone)]
pub struct SharedState {
    inner: Arc<RwLock<RuntimeState>>,
    storage_path: Arc<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerControl {
    Continue,
    Paused,
    Canceled,
    Missing,
}

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
        ensure_download_category_directories(Path::new(&persisted.settings.download_directory))?;

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
                last_host_contact: None,
            })),
            storage_path: Arc::new(storage_path),
        };

        state.persist_current_state_sync()?;
        Ok(state)
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

    pub async fn save_settings(&self, mut settings: Settings) -> Result<DesktopSnapshot, String> {
        validate_settings(&mut settings)?;

        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.settings = settings;
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn settings(&self) -> Settings {
        let state = self.inner.read().await;
        state.settings.clone()
    }

    pub fn settings_sync(&self) -> Settings {
        let state = self.inner.blocking_read();
        state.settings.clone()
    }

    pub async fn main_window_state(&self) -> Option<MainWindowState> {
        let state = self.inner.read().await;
        state.main_window.clone()
    }

    pub fn main_window_state_sync(&self) -> Option<MainWindowState> {
        let state = self.inner.blocking_read();
        state.main_window.clone()
    }

    pub async fn save_main_window_state(&self, main_window: MainWindowState) -> Result<(), String> {
        let persisted = {
            let mut state = self.inner.write().await;
            state.main_window = Some(main_window);
            state.persisted()
        };

        persist_state(&self.storage_path, &persisted)
    }

    pub fn save_main_window_state_sync(&self, main_window: MainWindowState) -> Result<(), String> {
        let persisted = {
            let mut state = self.inner.blocking_write();
            state.main_window = Some(main_window);
            state.persisted()
        };

        persist_state(&self.storage_path, &persisted)
    }

    pub async fn save_extension_integration_settings(
        &self,
        mut extension_settings: ExtensionIntegrationSettings,
    ) -> Result<DesktopSnapshot, String> {
        normalize_extension_settings(&mut extension_settings);

        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.settings.extension_integration = extension_settings;
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn enqueue_download(
        &self,
        url: String,
        source: Option<DownloadSource>,
    ) -> Result<EnqueueResult, BackendError> {
        self.enqueue_download_with_options(
            url,
            EnqueueOptions {
                source,
                ..Default::default()
            },
        )
        .await
    }

    pub async fn enqueue_downloads(
        &self,
        urls: Vec<String>,
        source: Option<DownloadSource>,
        bulk_archive_name: Option<String>,
    ) -> Result<Vec<EnqueueResult>, BackendError> {
        if urls.is_empty() {
            return Err(BackendError {
                code: "INVALID_URL",
                message: "Add at least one download URL.".into(),
            });
        }

        let normalized_urls = urls
            .iter()
            .map(|url| normalize_download_url(url))
            .collect::<Result<Vec<_>, _>>()?;
        let bulk_archive = bulk_archive_name
            .filter(|_| normalized_urls.len() > 1)
            .map(|name| BulkArchiveInfo {
                id: format!(
                    "bulk_{}_{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|duration| duration.as_millis())
                        .unwrap_or_default(),
                    normalized_urls.len()
                ),
                name: normalize_archive_filename(&name),
                archive_status: BulkArchiveStatus::Pending,
                output_path: None,
                error: None,
            });

        let mut results = Vec::with_capacity(normalized_urls.len());
        for url in normalized_urls {
            results.push(
                self.enqueue_download_with_options(
                    url,
                    EnqueueOptions {
                        source: source.clone(),
                        bulk_archive: bulk_archive.clone(),
                        ..Default::default()
                    },
                )
                .await?,
            );
        }

        Ok(results)
    }

    pub async fn enqueue_download_with_options(
        &self,
        url: String,
        options: EnqueueOptions,
    ) -> Result<EnqueueResult, BackendError> {
        let (result, persisted) = {
            let mut state = self.inner.write().await;
            let result = state.enqueue_download_in_memory(&url, options)?;
            let persisted = state.persisted();
            (result, persisted)
        };

        if result.status == EnqueueStatus::Queued {
            persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        }

        Ok(result)
    }

    pub async fn prepare_download_prompt(
        &self,
        id: impl Into<String>,
        url: &str,
        source: Option<DownloadSource>,
        filename_hint: Option<String>,
        total_bytes: Option<u64>,
    ) -> Result<DownloadPrompt, BackendError> {
        let state = self.inner.read().await;
        state.prepare_download_prompt(id, url, source, filename_hint, total_bytes)
    }
}

impl RuntimeState {
    fn enqueue_download_in_memory(
        &mut self,
        url: &str,
        mut options: EnqueueOptions,
    ) -> Result<EnqueueResult, BackendError> {
        let url = normalize_download_url(url)?;
        options.expected_sha256 = normalize_expected_sha256(options.expected_sha256)?;

        if options.duplicate_policy == DuplicatePolicy::ReturnExisting {
            if let Some(result) = self.duplicate_enqueue_result(&url) {
                return Ok(result);
            }
        }

        let directory = options
            .directory_override
            .as_deref()
            .unwrap_or(&self.settings.download_directory)
            .trim();
        if directory.is_empty() {
            return Err(BackendError {
                code: "DESTINATION_NOT_CONFIGURED",
                message: "Configure a download directory before adding downloads.".into(),
            });
        }

        let download_dir = PathBuf::from(directory);
        std::fs::create_dir_all(&download_dir).map_err(|error| BackendError {
            code: "DESTINATION_INVALID",
            message: format!("Could not create the download directory: {error}"),
        })?;

        let filename = filename_from_hint(options.filename_hint.as_deref(), &url);
        let target_dir = prepare_category_download_directory(&download_dir, &filename)?;
        verify_download_directory_writable(&target_dir)?;
        let (target_path, temp_path) = allocate_target_paths(&target_dir, &filename, &self.jobs);
        let job_id = format!("job_{}", self.next_job_number);
        self.next_job_number += 1;
        let integrity_check = options.expected_sha256.map(|expected| IntegrityCheck {
            algorithm: IntegrityAlgorithm::Sha256,
            expected,
            actual: None,
            status: IntegrityStatus::Pending,
        });

        self.jobs.push(DownloadJob {
            id: job_id.clone(),
            url: url.clone(),
            filename: filename.clone(),
            source: options.source,
            transfer_kind: TransferKind::Http,
            integrity_check,
            state: JobState::Queued,
            created_at: current_unix_timestamp_millis(),
            progress: 0.0,
            total_bytes: 0,
            downloaded_bytes: 0,
            speed: 0,
            eta: 0,
            error: None,
            failure_category: None,
            resume_support: ResumeSupport::Unknown,
            retry_attempts: 0,
            target_path: target_path.display().to_string(),
            temp_path: temp_path.display().to_string(),
            artifact_exists: None,
            bulk_archive: options.bulk_archive,
        });
        self.push_diagnostic_event(
            DiagnosticLevel::Info,
            "download".into(),
            format!("Queued {filename}"),
            Some(job_id.clone()),
        );

        Ok(EnqueueResult {
            snapshot: self.snapshot(),
            job_id,
            filename,
            status: EnqueueStatus::Queued,
        })
    }

    fn prepare_download_prompt(
        &self,
        id: impl Into<String>,
        url: &str,
        source: Option<DownloadSource>,
        filename_hint: Option<String>,
        total_bytes: Option<u64>,
    ) -> Result<DownloadPrompt, BackendError> {
        let url = normalize_download_url(url)?;
        let filename = filename_from_hint(filename_hint.as_deref(), &url);
        let default_directory = self.settings.download_directory.clone();
        let target_path = if default_directory.trim().is_empty() {
            String::new()
        } else {
            let category_dir =
                category_download_directory(Path::new(&default_directory), &filename);
            let (target_path, _) = allocate_target_paths(&category_dir, &filename, &self.jobs);
            target_path.display().to_string()
        };
        let duplicate_job = self.jobs.iter().find(|job| job.url == url).cloned();

        Ok(DownloadPrompt {
            id: id.into(),
            url,
            filename,
            source,
            total_bytes: total_bytes.filter(|bytes| *bytes > 0),
            default_directory,
            target_path,
            duplicate_job,
        })
    }

    fn mark_bulk_archive_status_in_memory(
        &mut self,
        archive_id: &str,
        archive_status: BulkArchiveStatus,
        output_path: Option<String>,
        error: Option<String>,
    ) {
        for job in &mut self.jobs {
            let Some(archive) = &mut job.bulk_archive else {
                continue;
            };
            if archive.id != archive_id {
                continue;
            }

            archive.archive_status = archive_status;
            archive.output_path = output_path.clone();
            archive.error = error.clone();
        }
    }
}

impl SharedState {
    pub async fn pause_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let event_message = {
                let job = find_job_mut(&mut state.jobs, id)?;
                if matches!(
                    job.state,
                    JobState::Queued | JobState::Starting | JobState::Downloading
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
        Ok(snapshot)
    }

    pub async fn resume_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
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
            state.pause_all_jobs();
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn resume_all_jobs(&self) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
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
            let _ = std::fs::remove_file(temp_path);
        }

        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
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
        Ok(snapshot)
    }

    pub async fn retry_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
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

            if is_active_worker || matches!(job.state, JobState::Starting | JobState::Downloading) {
                return Err(BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Pause or cancel the active transfer before removing it.".into(),
                });
            }

            let paths_to_cleanup = (PathBuf::from(&job.temp_path), job.state);
            state.jobs.remove(job_index);

            (state.snapshot(), state.persisted(), paths_to_cleanup)
        };

        let (temp_path, job_state) = paths_to_cleanup;
        if job_state != JobState::Completed {
            let _ = std::fs::remove_file(temp_path);
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

                if state.active_workers.contains(id)
                    || matches!(job.state, JobState::Starting | JobState::Downloading)
                {
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

            remove_file_if_exists(&target_path).map_err(internal_error)?;
            if temp_path != target_path {
                remove_file_if_exists(&temp_path).map_err(internal_error)?;
            }
        }

        self.remove_job(id).await
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
                || matches!(job.state, JobState::Starting | JobState::Downloading)
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

    pub async fn claim_schedulable_jobs(
        &self,
    ) -> Result<(DesktopSnapshot, Vec<DownloadTask>), String> {
        let (snapshot, persisted, tasks) = {
            let mut state = self.inner.write().await;
            let available_slots = state
                .settings
                .max_concurrent_downloads
                .max(1)
                .saturating_sub(state.active_workers.len() as u32)
                as usize;

            if available_slots == 0 {
                return Ok((state.snapshot(), Vec::new()));
            }

            let mut scheduled_ids = Vec::new();
            for job in &state.jobs {
                if scheduled_ids.len() >= available_slots {
                    break;
                }

                if job.state == JobState::Queued && !state.active_workers.contains(&job.id) {
                    scheduled_ids.push(job.id.clone());
                }
            }

            let mut tasks = Vec::new();
            for scheduled_id in scheduled_ids {
                if let Some(job) = state.jobs.iter_mut().find(|job| job.id == scheduled_id) {
                    job.state = JobState::Starting;
                    job.speed = 0;
                    job.eta = 0;
                    job.error = None;
                    let task = DownloadTask {
                        id: job.id.clone(),
                        url: job.url.clone(),
                        transfer_kind: job.transfer_kind,
                        target_path: PathBuf::from(&job.target_path),
                        temp_path: PathBuf::from(&job.temp_path),
                    };
                    let task_id = task.id.clone();
                    let _ = job;
                    state.active_workers.insert(task_id);
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

    pub async fn worker_control(&self, id: &str) -> WorkerControl {
        let state = self.inner.read().await;
        let Some(job) = state.jobs.iter().find(|job| job.id == id) else {
            return WorkerControl::Missing;
        };

        match job.state {
            JobState::Paused => WorkerControl::Paused,
            JobState::Canceled => WorkerControl::Canceled,
            JobState::Completed | JobState::Failed => WorkerControl::Missing,
            _ => WorkerControl::Continue,
        }
    }

    pub async fn sync_downloaded_bytes(
        &self,
        id: &str,
        downloaded_bytes: u64,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            job.downloaded_bytes = downloaded_bytes;
            if job.total_bytes > 0 {
                job.progress =
                    (downloaded_bytes as f64 / job.total_bytes as f64 * 100.0).clamp(0.0, 100.0);
            } else {
                job.progress = 0.0;
            }
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn mark_job_downloading(
        &self,
        id: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        resume_support: ResumeSupport,
        filename: Option<String>,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            job.state = JobState::Downloading;
            if let Some(filename) = filename {
                apply_download_filename(job, &filename);
            }
            job.downloaded_bytes = downloaded_bytes;
            if let Some(total_bytes) = total_bytes {
                job.total_bytes = total_bytes.max(downloaded_bytes);
            }
            job.progress = if job.total_bytes == 0 {
                0.0
            } else {
                (job.downloaded_bytes as f64 / job.total_bytes as f64 * 100.0).clamp(0.0, 100.0)
            };
            job.error = None;
            job.failure_category = None;
            job.resume_support = resume_support;
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn apply_preflight_metadata(
        &self,
        id: &str,
        total_bytes: Option<u64>,
        resume_support: ResumeSupport,
        filename: Option<String>,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            apply_preflight_metadata_to_job(job, total_bytes, resume_support, filename);
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn record_retry_attempt(
        &self,
        id: &str,
        retry_attempts: u32,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let event_message = {
                let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                    return Err("Job not found.".into());
                };

                job.retry_attempts = retry_attempts;
                job.speed = 0;
                job.eta = 0;
                format!("Retry attempt {retry_attempts} for {}", job.filename)
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

    pub async fn update_job_progress(
        &self,
        id: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        speed: u64,
        persist: bool,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            job.state = JobState::Downloading;
            job.downloaded_bytes = downloaded_bytes;
            if let Some(total_bytes) = total_bytes {
                job.total_bytes = total_bytes.max(downloaded_bytes);
            }
            job.speed = speed;

            if job.total_bytes > 0 {
                job.progress = (job.downloaded_bytes as f64 / job.total_bytes as f64 * 100.0)
                    .clamp(0.0, 100.0);
                let remaining = job.total_bytes.saturating_sub(job.downloaded_bytes);
                job.eta = if speed == 0 {
                    0
                } else {
                    ((remaining as f64) / (speed as f64)).ceil() as u64
                };
            } else {
                job.progress = 0.0;
                job.eta = 0;
            }

            (state.snapshot(), state.persisted())
        };

        if persist {
            persist_state(&self.storage_path, &persisted)?;
        }
        Ok(snapshot)
    }

    pub async fn job_requires_sha256(&self, id: &str) -> bool {
        let state = self.inner.read().await;
        state
            .jobs
            .iter()
            .find(|job| job.id == id)
            .and_then(|job| job.integrity_check.as_ref())
            .is_some_and(|check| {
                check.algorithm == IntegrityAlgorithm::Sha256
                    && check.status == IntegrityStatus::Pending
            })
    }

    pub async fn complete_job(
        &self,
        id: &str,
        total_bytes: u64,
        target_path: &Path,
    ) -> Result<DesktopSnapshot, String> {
        self.complete_job_with_integrity(id, total_bytes, target_path, None)
            .await
    }

    pub async fn complete_job_with_integrity(
        &self,
        id: &str,
        total_bytes: u64,
        target_path: &Path,
        actual_sha256: Option<String>,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            job.state = JobState::Completed;
            job.downloaded_bytes = total_bytes;
            job.total_bytes = total_bytes;
            job.progress = 100.0;
            job.speed = 0;
            job.eta = 0;
            job.error = None;
            job.filename = target_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(&job.filename)
                .to_string();
            job.target_path = target_path.display().to_string();
            job.temp_path = format!("{}.part", job.target_path);
            let completed_filename = job.filename.clone();
            let mut event = (
                DiagnosticLevel::Info,
                format!("Completed {completed_filename}"),
            );
            if let Some(check) = &mut job.integrity_check {
                if let Some(actual) = actual_sha256 {
                    check.actual = Some(actual.clone());
                    if check.expected.eq_ignore_ascii_case(&actual) {
                        check.status = IntegrityStatus::Verified;
                        event = (
                            DiagnosticLevel::Info,
                            format!("Verified SHA-256 for {completed_filename}"),
                        );
                    } else {
                        check.status = IntegrityStatus::Failed;
                        job.state = JobState::Failed;
                        job.failure_category = Some(FailureCategory::Integrity);
                        job.error = Some(format!(
                            "SHA-256 checksum mismatch. Expected {}, got {actual}.",
                            check.expected
                        ));
                        event = (
                            DiagnosticLevel::Error,
                            format!("SHA-256 verification failed for {completed_filename}"),
                        );
                    }
                }
            }
            state.active_workers.remove(id);
            state.push_diagnostic_event(event.0, "download".into(), event.1, Some(id.into()));
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn mark_bulk_archive_status(
        &self,
        archive_id: &str,
        archive_status: BulkArchiveStatus,
        output_path: Option<String>,
        error: Option<String>,
    ) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.mark_bulk_archive_status_in_memory(
                archive_id,
                archive_status,
                output_path,
                error,
            );
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn bulk_archive_ready_for_job(
        &self,
        id: &str,
    ) -> Result<Option<BulkArchiveReady>, String> {
        let state = self.inner.read().await;
        let Some(job) = state.jobs.iter().find(|job| job.id == id) else {
            return Ok(None);
        };
        let Some(archive) = &job.bulk_archive else {
            return Ok(None);
        };
        if archive.archive_status != BulkArchiveStatus::Pending {
            return Ok(None);
        }

        let members = state
            .jobs
            .iter()
            .filter(|candidate| {
                candidate
                    .bulk_archive
                    .as_ref()
                    .is_some_and(|candidate_archive| candidate_archive.id == archive.id)
            })
            .collect::<Vec<_>>();

        if members.len() < 2
            || members
                .iter()
                .any(|member| member.state != JobState::Completed)
        {
            return Ok(None);
        }

        let output_dir = PathBuf::from(&state.settings.download_directory);
        let output_path = output_dir.join(&archive.name);
        if output_path.exists() {
            return Ok(None);
        }

        let mut used_names = HashSet::new();
        let mut entries = Vec::with_capacity(members.len());
        for member in members {
            let source_path = PathBuf::from(&member.target_path);
            if !source_path.is_file() {
                return Ok(None);
            }

            let archive_name = unique_archive_entry_name(&member.filename, &mut used_names);
            entries.push(BulkArchiveEntry {
                source_path,
                archive_name,
            });
        }

        Ok(Some(BulkArchiveReady {
            archive_id: archive.id.clone(),
            output_path,
            entries,
        }))
    }

    pub async fn fail_job(
        &self,
        id: &str,
        message: impl Into<String>,
        failure_category: FailureCategory,
    ) -> Result<DesktopSnapshot, String> {
        let message = message.into();
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.active_workers.remove(id);
            let event_message = {
                let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                    return Err("Job not found.".into());
                };

                job.state = JobState::Failed;
                job.speed = 0;
                job.eta = 0;
                job.error = Some(message.clone());
                job.failure_category = Some(failure_category);
                format!("Failed {}: {message}", job.filename)
            };
            state.push_diagnostic_event(
                DiagnosticLevel::Error,
                "download".into(),
                event_message,
                Some(id.into()),
            );
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn finish_interrupted_job(&self, id: &str) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.active_workers.remove(id);
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            if job.state == JobState::Canceled {
                job.progress = 0.0;
                job.total_bytes = 0;
                job.downloaded_bytes = 0;
                job.error = None;
            }

            job.speed = 0;
            job.eta = 0;
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn resolve_openable_path(&self, id: &str) -> Result<PathBuf, BackendError> {
        let path = {
            let state = self.inner.read().await;
            let job = state
                .jobs
                .iter()
                .find(|job| job.id == id)
                .ok_or_else(|| BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Job not found.".into(),
                })?;

            PathBuf::from(&job.target_path)
        };

        if path.is_file() {
            Ok(path)
        } else {
            Err(BackendError {
                code: "INTERNAL_ERROR",
                message: format!(
                    "The downloaded file is not available on disk: {}",
                    path.display()
                ),
            })
        }
    }

    pub async fn resolve_revealable_path(&self, id: &str) -> Result<PathBuf, BackendError> {
        let (job_state, target_path, temp_path) = {
            let state = self.inner.read().await;
            let job = state
                .jobs
                .iter()
                .find(|job| job.id == id)
                .ok_or_else(|| BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Job not found.".into(),
                })?;

            (
                job.state,
                PathBuf::from(&job.target_path),
                PathBuf::from(&job.temp_path),
            )
        };

        if target_path.exists() {
            return Ok(target_path);
        }

        if temp_path.exists() {
            return Ok(temp_path);
        }

        if job_state == JobState::Completed {
            return Err(BackendError {
                code: "INTERNAL_ERROR",
                message: format!(
                    "Downloaded file is missing from disk: {}",
                    target_path.display()
                ),
            });
        }

        if let Some(parent) = target_path.parent() {
            if parent.exists() {
                return Ok(parent.to_path_buf());
            }
        }

        Err(BackendError {
            code: "INTERNAL_ERROR",
            message: format!(
                "No local path is available for this job yet. Expected path: {}",
                target_path.display()
            ),
        })
    }

    fn persist_current_state_sync(&self) -> Result<(), String> {
        let state = self.inner.blocking_read();
        persist_state(&self.storage_path, &state.persisted())
    }
}

impl RuntimeState {
    fn push_diagnostic_event(
        &mut self,
        level: DiagnosticLevel,
        category: String,
        message: String,
        job_id: Option<String>,
    ) {
        self.diagnostic_events.push(DiagnosticEvent {
            timestamp: current_unix_timestamp_millis(),
            level,
            category,
            message,
            job_id,
        });

        trim_diagnostic_events(&mut self.diagnostic_events);
    }

    fn snapshot(&self) -> DesktopSnapshot {
        DesktopSnapshot {
            connection_state: self.connection_state,
            jobs: self
                .jobs
                .iter()
                .cloned()
                .map(add_artifact_existence)
                .collect(),
            settings: self.settings.clone(),
        }
    }

    fn persisted(&self) -> PersistedState {
        PersistedState {
            jobs: self
                .jobs
                .iter()
                .cloned()
                .map(clear_transient_job_state)
                .collect(),
            settings: self.settings.clone(),
            main_window: self.main_window.clone(),
            diagnostic_events: self.diagnostic_events.clone(),
        }
    }

    fn pause_all_jobs(&mut self) {
        for job in &mut self.jobs {
            if matches!(
                job.state,
                JobState::Queued | JobState::Starting | JobState::Downloading
            ) {
                job.state = JobState::Paused;
                job.speed = 0;
                job.eta = 0;
            }
        }
    }

    fn resume_all_jobs(&mut self) {
        for job in &mut self.jobs {
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
            }
        }
    }

    fn duplicate_enqueue_result(&self, url: &str) -> Option<EnqueueResult> {
        let existing_index = self.jobs.iter().position(|job| job.url == url)?;
        let existing_job = &self.jobs[existing_index];

        Some(EnqueueResult {
            snapshot: self.snapshot(),
            job_id: existing_job.id.clone(),
            filename: existing_job.filename.clone(),
            status: EnqueueStatus::DuplicateExistingJob,
        })
    }

    fn queue_summary(&self) -> QueueSummary {
        QueueSummary {
            total: self.jobs.len(),
            active: self
                .jobs
                .iter()
                .filter(|job| {
                    matches!(
                        job.state,
                        JobState::Queued
                            | JobState::Starting
                            | JobState::Downloading
                            | JobState::Paused
                    )
                })
                .count(),
            attention: self
                .jobs
                .iter()
                .filter(|job| job_needs_attention(job))
                .count(),
            queued: self
                .jobs
                .iter()
                .filter(|job| job.state == JobState::Queued)
                .count(),
            downloading: self
                .jobs
                .iter()
                .filter(|job| matches!(job.state, JobState::Starting | JobState::Downloading))
                .count(),
            completed: self
                .jobs
                .iter()
                .filter(|job| matches!(job.state, JobState::Completed | JobState::Canceled))
                .count(),
            failed: self
                .jobs
                .iter()
                .filter(|job| job.state == JobState::Failed)
                .count(),
        }
    }
}

fn job_needs_attention(job: &DownloadJob) -> bool {
    if job.state == JobState::Failed || job.failure_category.is_some() {
        return true;
    }

    let is_unfinished = !matches!(job.state, JobState::Completed | JobState::Canceled);
    let has_partial_progress = job.downloaded_bytes > 0 || job.progress > 0.0;
    is_unfinished && has_partial_progress && job.resume_support == ResumeSupport::Unsupported
}

fn normalize_job(mut job: DownloadJob, settings: &Settings) -> DownloadJob {
    if let Some(check) = &mut job.integrity_check {
        check.expected = check.expected.trim().to_ascii_lowercase();
        check.actual = check.actual.as_ref().map(|value| value.trim().to_ascii_lowercase());
    }

    if job.filename.trim().is_empty() {
        job.filename = derive_filename(&job.url);
    }

    if job.target_path.trim().is_empty() {
        let target_path = PathBuf::from(&settings.download_directory).join(&job.filename);
        job.target_path = target_path.display().to_string();
    }

    if job.temp_path.trim().is_empty() {
        job.temp_path = format!("{}.part", job.target_path);
    }

    if matches!(job.state, JobState::Starting | JobState::Downloading) {
        job.state = JobState::Queued;
        job.speed = 0;
        job.eta = 0;
    }

    if job.created_at == 0 {
        job.created_at = current_unix_timestamp_millis();
    }

    job.artifact_exists = None;

    job
}

fn add_artifact_existence(mut job: DownloadJob) -> DownloadJob {
    job.artifact_exists = if job.state == JobState::Completed {
        Some(Path::new(&job.target_path).is_file())
    } else {
        None
    };
    job
}

fn clear_transient_job_state(mut job: DownloadJob) -> DownloadJob {
    job.artifact_exists = None;
    job
}

fn current_unix_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn normalize_diagnostic_events(mut events: Vec<DiagnosticEvent>) -> Vec<DiagnosticEvent> {
    trim_diagnostic_events(&mut events);
    events
}

fn trim_diagnostic_events(events: &mut Vec<DiagnosticEvent>) {
    if events.len() > DIAGNOSTIC_EVENT_LIMIT {
        let overflow = events.len() - DIAGNOSTIC_EVENT_LIMIT;
        events.drain(0..overflow);
    }
}

fn normalize_extension_settings(settings: &mut ExtensionIntegrationSettings) {
    if settings.listen_port == 0 || settings.listen_port > u16::MAX as u32 {
        settings.listen_port = default_extension_listen_port();
    }

    let mut normalized_hosts = Vec::new();
    let mut seen_hosts = HashSet::new();

    for host in &settings.excluded_hosts {
        let mut host = host.trim().to_ascii_lowercase();
        if let Some(stripped) = host.strip_prefix("http://") {
            host = stripped.to_string();
        } else if let Some(stripped) = host.strip_prefix("https://") {
            host = stripped.to_string();
        }
        let host = host
            .split('/')
            .next()
            .unwrap_or_default()
            .trim_matches('/')
            .to_string();

        if host.is_empty() || !seen_hosts.insert(host.clone()) {
            continue;
        }

        normalized_hosts.push(host);
    }

    settings.excluded_hosts = normalized_hosts;

    let mut normalized_extensions = Vec::new();
    let mut seen_extensions = HashSet::new();

    for extension in &settings.ignored_file_extensions {
        for candidate in
            extension.split(|character: char| character == ',' || character.is_whitespace())
        {
            let candidate = normalize_file_extension(candidate);
            if candidate.is_empty() || !seen_extensions.insert(candidate.clone()) {
                continue;
            }

            normalized_extensions.push(candidate);
        }
    }

    settings.ignored_file_extensions = normalized_extensions;
}

fn normalize_accent_color(settings: &mut Settings) {
    let accent_color = settings.accent_color.trim();
    let is_hex_color = accent_color.len() == 7
        && accent_color.starts_with('#')
        && accent_color
            .chars()
            .skip(1)
            .all(|character| character.is_ascii_hexdigit());

    if is_hex_color {
        settings.accent_color = accent_color.to_ascii_lowercase();
    } else {
        settings.accent_color = "#3b82f6".into();
    }
}

fn normalize_file_extension(value: &str) -> String {
    let extension = value.trim().trim_start_matches('.').to_ascii_lowercase();
    if extension.is_empty()
        || extension.contains('/')
        || extension.contains('\\')
        || extension.chars().all(|character| character == '.')
    {
        return String::new();
    }

    extension
}

fn normalize_expected_sha256(value: Option<String>) -> Result<Option<String>, BackendError> {
    let Some(value) = value else {
        return Ok(None);
    };

    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(None);
    }

    if normalized.len() != SHA256_HEX_LENGTH
        || !normalized
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(BackendError {
            code: "INVALID_INTEGRITY_HASH",
            message: "SHA-256 checksum must be 64 hexadecimal characters.".into(),
        });
    }

    Ok(Some(normalized))
}

fn normalize_download_url(raw_url: &str) -> Result<String, BackendError> {
    let trimmed_url = raw_url.trim();
    if trimmed_url.len() > MAX_URL_LENGTH {
        return Err(BackendError {
            code: "URL_TOO_LONG",
            message: format!("URL exceeds {MAX_URL_LENGTH} characters."),
        });
    }

    let parsed = Url::parse(trimmed_url).map_err(|_| BackendError {
        code: "INVALID_URL",
        message: "URL is not valid.".into(),
    })?;

    match parsed.scheme() {
        "http" | "https" => Ok(parsed.to_string()),
        _ => Err(BackendError {
            code: "UNSUPPORTED_SCHEME",
            message: "Only http and https URLs are supported.".into(),
        }),
    }
}

fn verify_download_directory_writable(download_dir: &Path) -> Result<(), BackendError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let probe_name = format!(
        ".simple-download-manager-write-test-{}-{timestamp}",
        std::process::id()
    );

    verify_download_directory_writable_with_probe_name(download_dir, &probe_name)
}

fn verify_download_directory_writable_with_probe_name(
    download_dir: &Path,
    probe_name: &str,
) -> Result<(), BackendError> {
    let probe_path = download_dir.join(probe_name);
    let probe_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe_path)
        .map_err(destination_write_error)?;
    drop(probe_file);

    std::fs::remove_file(&probe_path).map_err(destination_write_error)?;
    Ok(())
}

fn destination_write_error(error: std::io::Error) -> BackendError {
    let code = if error.kind() == std::io::ErrorKind::PermissionDenied {
        "PERMISSION_DENIED"
    } else {
        "DESTINATION_INVALID"
    };

    BackendError {
        code,
        message: format!("Download directory is not writable: {error}"),
    }
}

fn prepare_category_download_directory(
    download_dir: &Path,
    filename: &str,
) -> Result<PathBuf, BackendError> {
    ensure_download_category_directories(download_dir).map_err(|error| BackendError {
        code: "DESTINATION_INVALID",
        message: error,
    })?;
    Ok(category_download_directory(download_dir, filename))
}

fn ensure_download_category_directories(download_dir: &Path) -> Result<(), String> {
    for folder in DOWNLOAD_CATEGORY_FOLDERS {
        let category_dir = download_dir.join(folder);
        std::fs::create_dir_all(&category_dir).map_err(|error| {
            format!("Could not create {folder} download category directory: {error}")
        })?;
    }

    Ok(())
}

fn category_download_directory(download_dir: &Path, filename: &str) -> PathBuf {
    download_dir.join(category_folder_for_filename(filename))
}

fn category_folder_for_filename(filename: &str) -> &'static str {
    let Some(extension) = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
    else {
        return "Other";
    };

    match extension.as_str() {
        ext if DOCUMENT_EXTENSIONS.contains(&ext) => "Document",
        ext if PROGRAM_EXTENSIONS.contains(&ext) => "Program",
        ext if PICTURE_EXTENSIONS.contains(&ext) => "Picture",
        ext if VIDEO_EXTENSIONS.contains(&ext) => "Video",
        ext if COMPRESSED_EXTENSIONS.contains(&ext) => "Compressed",
        ext if MUSIC_EXTENSIONS.contains(&ext) => "Music",
        _ => "Other",
    }
}

fn allocate_target_paths(
    download_dir: &Path,
    filename: &str,
    jobs: &[DownloadJob],
) -> (PathBuf, PathBuf) {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();

    let occupied_targets = jobs
        .iter()
        .map(|job| job.target_path.clone())
        .collect::<HashSet<_>>();
    let occupied_temps = jobs
        .iter()
        .map(|job| job.temp_path.clone())
        .collect::<HashSet<_>>();

    for index in 0..10_000 {
        let candidate = if index == 0 {
            format!("{stem}{extension}")
        } else {
            format!("{stem} ({index}){extension}")
        };
        let target_path = download_dir.join(&candidate);
        let temp_path = download_dir.join(format!("{candidate}.part"));
        let target_key = target_path.display().to_string();
        let temp_key = temp_path.display().to_string();

        if occupied_targets.contains(&target_key) || occupied_temps.contains(&temp_key) {
            continue;
        }

        if target_path.exists() || temp_path.exists() {
            continue;
        }

        return (target_path, temp_path);
    }

    let fallback_target = download_dir.join(filename);
    let fallback_temp = download_dir.join(format!("{filename}.part"));
    (fallback_target, fallback_temp)
}

fn derive_filename(raw_url: &str) -> String {
    let fallback = "download.bin".to_string();
    let Ok(url) = Url::parse(raw_url) else {
        return fallback;
    };

    let candidate = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .unwrap_or("download.bin");

    let decoded = percent_decode_str(candidate).decode_utf8_lossy();
    sanitize_filename(&decoded)
}

fn filename_from_hint(filename_hint: Option<&str>, raw_url: &str) -> String {
    filename_hint
        .map(|hint| {
            let decoded = percent_decode_str(hint).decode_utf8_lossy();
            sanitize_filename(&decoded)
        })
        .filter(|filename| !filename.trim().is_empty())
        .unwrap_or_else(|| derive_filename(raw_url))
}

fn sanitize_filename(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            character if character.is_control() => '_',
            _ => character,
        })
        .collect();
    let mut sanitized = sanitized.trim().trim_matches('.').trim().to_string();

    if sanitized.trim().is_empty() {
        "download.bin".into()
    } else {
        if is_windows_reserved_filename(&sanitized) {
            sanitized.push('_');
        }
        sanitized
    }
}

fn should_reset_download_directory(
    download_directory: &str,
    has_data_dir_override: bool,
    storage_exists: bool,
) -> bool {
    download_directory.trim().is_empty()
        || is_legacy_default_download_directory(download_directory)
        || (has_data_dir_override && !storage_exists)
}

fn is_legacy_default_download_directory(download_directory: &str) -> bool {
    let normalized = download_directory
        .trim()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();

    normalized == "c:/downloads" || normalized == "c:/users/you/downloads"
}

fn is_windows_reserved_filename(filename: &str) -> bool {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename)
        .to_ascii_uppercase();

    matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn normalize_archive_filename(input: &str) -> String {
    let mut filename = sanitize_filename(input);
    if !filename.to_ascii_lowercase().ends_with(".zip") {
        filename.push_str(".zip");
    }
    filename
}

fn unique_archive_entry_name(filename: &str, used_names: &mut HashSet<String>) -> String {
    let sanitized = sanitize_filename(filename);
    if used_names.insert(sanitized.clone()) {
        return sanitized;
    }

    let stem = Path::new(&sanitized)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = Path::new(&sanitized)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();

    for index in 1..10_000 {
        let candidate = format!("{stem} ({index}){extension}");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
    }

    sanitized
}

fn next_job_number(jobs: &[DownloadJob]) -> u64 {
    jobs.iter()
        .filter_map(|job| job.id.strip_prefix("job_"))
        .filter_map(|value| value.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

fn find_job_mut<'a>(
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

fn reset_job_for_restart(job: &mut DownloadJob) {
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
}

fn reset_integrity_for_retry(job: &mut DownloadJob) {
    if let Some(check) = &mut job.integrity_check {
        check.actual = None;
        check.status = IntegrityStatus::Pending;
    }
}

fn apply_download_filename(job: &mut DownloadJob, filename: &str) {
    let filename = filename.trim();
    if !filename.is_empty() {
        job.filename = filename.to_string();
    }
}

fn apply_preflight_metadata_to_job(
    job: &mut DownloadJob,
    total_bytes: Option<u64>,
    resume_support: ResumeSupport,
    filename: Option<String>,
) {
    if let Some(filename) = filename {
        apply_download_filename(job, &filename);
    }

    if let Some(total_bytes) = total_bytes {
        job.total_bytes = total_bytes.max(job.downloaded_bytes);
        job.progress = if job.total_bytes == 0 {
            0.0
        } else {
            (job.downloaded_bytes as f64 / job.total_bytes as f64 * 100.0).clamp(0.0, 100.0)
        };
    }

    if resume_support != ResumeSupport::Unknown {
        job.resume_support = resume_support;
    }
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not remove partial download file: {error}")),
    }
}

pub fn validate_settings(settings: &mut Settings) -> Result<(), String> {
    if settings.download_directory.trim().is_empty() {
        return Err("Download directory cannot be empty.".into());
    }

    normalize_accent_color(settings);
    normalize_extension_settings(&mut settings.extension_integration);

    std::fs::create_dir_all(&settings.download_directory)
        .map_err(|error| format!("Could not create download directory: {error}"))?;
    ensure_download_category_directories(Path::new(&settings.download_directory))?;

    Ok(())
}

fn internal_error(error: String) -> BackendError {
    BackendError {
        code: "INTERNAL_ERROR",
        message: error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::HostRegistrationStatus;

    #[test]
    fn queue_summary_counts_attention_jobs() {
        let state = RuntimeState {
            connection_state: ConnectionState::Connected,
            jobs: vec![
                download_job("job_1", JobState::Failed, ResumeSupport::Supported, 25),
                download_job("job_2", JobState::Paused, ResumeSupport::Unsupported, 40),
                download_job(
                    "job_3",
                    JobState::Downloading,
                    ResumeSupport::Unsupported,
                    0,
                ),
                download_job(
                    "job_4",
                    JobState::Completed,
                    ResumeSupport::Unsupported,
                    100,
                ),
                download_job("job_5", JobState::Queued, ResumeSupport::Unknown, 0),
            ],
            settings: Settings::default(),
            main_window: None,
            diagnostic_events: Vec::new(),
            next_job_number: 6,
            active_workers: HashSet::new(),
            last_host_contact: None,
        };

        let summary = state.queue_summary();

        assert_eq!(summary.attention, 2);
    }

    #[test]
    fn duplicate_enqueue_result_includes_existing_job_details() {
        let mut existing_job =
            download_job("job_9", JobState::Paused, ResumeSupport::Supported, 50);
        existing_job.url = "https://example.com/file.zip".into();
        existing_job.filename = "file.zip".into();

        let state = RuntimeState {
            connection_state: ConnectionState::Connected,
            jobs: vec![existing_job],
            settings: Settings::default(),
            main_window: None,
            diagnostic_events: Vec::new(),
            next_job_number: 10,
            active_workers: HashSet::new(),
            last_host_contact: None,
        };

        let result = state
            .duplicate_enqueue_result("https://example.com/file.zip")
            .expect("duplicate result");

        assert_eq!(result.status, EnqueueStatus::DuplicateExistingJob);
        assert_eq!(result.job_id, "job_9");
        assert_eq!(result.filename, "file.zip");
        assert_eq!(result.snapshot.jobs.len(), 1);
    }

    #[test]
    fn enqueue_options_allow_duplicate_copy_with_unique_path() {
        let download_dir = test_runtime_dir("duplicate-copy");
        let mut existing_job =
            download_job("job_9", JobState::Paused, ResumeSupport::Supported, 50);
        existing_job.url = "https://example.com/file.zip".into();
        existing_job.filename = "file.zip".into();
        existing_job.target_path = download_dir.join("file.zip").display().to_string();
        existing_job.temp_path = download_dir.join("file.zip.part").display().to_string();

        let mut state = runtime_state_with_jobs(vec![existing_job]);
        state.settings.download_directory = download_dir.display().to_string();

        let result = state
            .enqueue_download_in_memory(
                "https://example.com/file.zip",
                EnqueueOptions {
                    duplicate_policy: DuplicatePolicy::Allow,
                    ..Default::default()
                },
            )
            .expect("duplicate copy should enqueue");

        assert_eq!(result.status, EnqueueStatus::Queued);
        assert_eq!(state.jobs.len(), 2);
        assert_eq!(state.jobs[1].filename, "file.zip");
        assert_eq!(
            target_parent_folder(&state.jobs[1].target_path),
            "Compressed"
        );
        assert!(state.jobs[1].target_path.ends_with("file.zip"));

        let _ = std::fs::remove_dir_all(download_dir);
    }

    #[test]
    fn enqueue_options_use_directory_override_without_saving_default() {
        let default_dir = test_runtime_dir("default-dir");
        let override_dir = test_runtime_dir("override-dir");
        let mut state = runtime_state_with_jobs(Vec::new());
        state.settings.download_directory = default_dir.display().to_string();

        let result = state
            .enqueue_download_in_memory(
                "https://example.com/report.pdf",
                EnqueueOptions {
                    directory_override: Some(override_dir.display().to_string()),
                    ..Default::default()
                },
            )
            .expect("download should enqueue into override directory");

        assert_eq!(result.status, EnqueueStatus::Queued);
        assert!(state.jobs[0]
            .target_path
            .starts_with(&override_dir.display().to_string()));
        assert_eq!(target_parent_folder(&state.jobs[0].target_path), "Document");
        assert_eq!(
            state.settings.download_directory,
            default_dir.display().to_string()
        );

        let _ = std::fs::remove_dir_all(default_dir);
        let _ = std::fs::remove_dir_all(override_dir);
    }

    #[test]
    fn validate_settings_creates_download_category_directories() {
        let download_dir = test_runtime_dir("category-settings");
        let mut settings = Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        };

        validate_settings(&mut settings).expect("settings should validate");

        for folder in [
            "Document",
            "Program",
            "Picture",
            "Video",
            "Compressed",
            "Music",
            "Other",
        ] {
            assert!(
                download_dir.join(folder).is_dir(),
                "{folder} category directory should exist"
            );
        }

        let _ = std::fs::remove_dir_all(download_dir);
    }

    #[test]
    fn enqueue_routes_downloads_into_category_directories() {
        let download_dir = test_runtime_dir("category-routing");
        let mut state = runtime_state_with_jobs(Vec::new());
        state.settings.download_directory = download_dir.display().to_string();

        for url in [
            "https://example.com/archive.zip",
            "https://example.com/setup.exe",
            "https://example.com/photo.jpg",
            "https://example.com/movie.mp4",
            "https://example.com/song.flac",
            "https://example.com/blob.custom",
        ] {
            state
                .enqueue_download_in_memory(url, EnqueueOptions::default())
                .expect("download should enqueue");
        }

        let folders = state
            .jobs
            .iter()
            .map(|job| target_parent_folder(&job.target_path))
            .collect::<Vec<_>>();

        assert_eq!(
            folders,
            vec![
                "Compressed",
                "Program",
                "Picture",
                "Video",
                "Music",
                "Other"
            ]
        );

        let _ = std::fs::remove_dir_all(download_dir);
    }

    #[test]
    fn prepare_download_prompt_marks_duplicate_job() {
        let mut existing_job = download_job(
            "job_12",
            JobState::Downloading,
            ResumeSupport::Supported,
            20,
        );
        existing_job.url = "https://example.com/archive.zip".into();
        existing_job.filename = "archive.zip".into();

        let download_dir = test_runtime_dir("prompt-category");
        let mut state = runtime_state_with_jobs(vec![existing_job]);
        state.settings.download_directory = download_dir.display().to_string();
        let prompt = state
            .prepare_download_prompt(
                "prompt_1",
                "https://example.com/archive.zip",
                None,
                Some("archive.zip".into()),
                Some(4096),
            )
            .expect("prompt should be prepared");

        assert_eq!(prompt.id, "prompt_1");
        assert_eq!(prompt.filename, "archive.zip");
        assert_eq!(prompt.total_bytes, Some(4096));
        assert_eq!(
            prompt.duplicate_job.as_ref().map(|job| job.id.as_str()),
            Some("job_12")
        );
        assert_eq!(target_parent_folder(&prompt.target_path), "Compressed");

        let _ = std::fs::remove_dir_all(download_dir);
    }

    #[test]
    fn destination_write_probe_reports_blocked_probe_path() {
        let test_dir = test_runtime_dir("destination-write-probe");
        let probe_name = "blocked-probe";
        std::fs::create_dir(test_dir.join(probe_name)).unwrap();

        let error =
            verify_download_directory_writable_with_probe_name(&test_dir, probe_name).unwrap_err();

        assert!(matches!(
            error.code,
            "DESTINATION_INVALID" | "PERMISSION_DENIED"
        ));
        assert!(error.message.contains("not writable"));

        let _ = std::fs::remove_dir_all(test_dir);
    }

    #[test]
    fn download_filename_metadata_updates_display_name_without_moving_partial_file() {
        let mut job = download_job(
            "job_11",
            JobState::Downloading,
            ResumeSupport::Supported,
            10,
        );
        job.filename = "download.bin".into();
        job.target_path = "C:/Downloads/download.bin".into();
        job.temp_path = "C:/Downloads/download.bin.part".into();

        apply_download_filename(&mut job, "server-report.pdf");

        assert_eq!(job.filename, "server-report.pdf");
        assert_eq!(job.target_path, "C:/Downloads/download.bin");
        assert_eq!(job.temp_path, "C:/Downloads/download.bin.part");
    }

    #[test]
    fn preflight_metadata_updates_job_size_resume_and_filename() {
        let mut job = download_job("job_12", JobState::Starting, ResumeSupport::Unknown, 0);
        job.filename = "download.bin".into();
        job.total_bytes = 0;

        apply_preflight_metadata_to_job(
            &mut job,
            Some(4_096),
            ResumeSupport::Supported,
            Some("server-report.pdf".into()),
        );

        assert_eq!(job.filename, "server-report.pdf");
        assert_eq!(job.total_bytes, 4_096);
        assert_eq!(job.resume_support, ResumeSupport::Supported);
        assert_eq!(job.progress, 0.0);
    }

    #[tokio::test]
    async fn reveal_completed_job_errors_when_file_is_missing_even_if_parent_exists() {
        let download_dir = test_runtime_dir("reveal-missing-completed");
        let target_path = download_dir.join("missing.zip");
        let mut job = download_job("job_20", JobState::Completed, ResumeSupport::Supported, 100);
        job.target_path = target_path.display().to_string();
        job.temp_path = format!("{}.part", job.target_path);
        let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

        let error = state.resolve_revealable_path("job_20").await.unwrap_err();

        assert_eq!(error.code, "INTERNAL_ERROR");
        assert!(error
            .message
            .contains("Downloaded file is missing from disk"));
        assert!(error.message.contains(&target_path.display().to_string()));

        let _ = std::fs::remove_dir_all(download_dir);
    }

    #[tokio::test]
    async fn reveal_completed_job_returns_existing_target_file() {
        let download_dir = test_runtime_dir("reveal-completed-existing");
        let target_path = download_dir.join("file.zip");
        std::fs::write(&target_path, b"downloaded").unwrap();
        let mut job = download_job("job_21", JobState::Completed, ResumeSupport::Supported, 100);
        job.target_path = target_path.display().to_string();
        job.temp_path = format!("{}.part", job.target_path);
        let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

        let resolved = state.resolve_revealable_path("job_21").await.unwrap();

        assert_eq!(resolved, target_path);

        let _ = std::fs::remove_dir_all(download_dir);
    }

    #[tokio::test]
    async fn reveal_interrupted_job_returns_existing_partial_file() {
        let download_dir = test_runtime_dir("reveal-partial-existing");
        let target_path = download_dir.join("file.zip");
        let temp_path = download_dir.join("file.zip.part");
        std::fs::write(&temp_path, b"partial").unwrap();
        let mut job = download_job("job_22", JobState::Failed, ResumeSupport::Supported, 50);
        job.target_path = target_path.display().to_string();
        job.temp_path = temp_path.display().to_string();
        let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

        let resolved = state.resolve_revealable_path("job_22").await.unwrap();

        assert_eq!(resolved, temp_path);

        let _ = std::fs::remove_dir_all(download_dir);
    }

    #[tokio::test]
    async fn reveal_unfinished_job_without_artifact_returns_parent_directory() {
        let download_dir = test_runtime_dir("reveal-parent-for-unfinished");
        let target_path = download_dir.join("future.zip");
        let mut job = download_job("job_23", JobState::Queued, ResumeSupport::Unknown, 0);
        job.target_path = target_path.display().to_string();
        job.temp_path = format!("{}.part", job.target_path);
        let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

        let resolved = state.resolve_revealable_path("job_23").await.unwrap();

        assert_eq!(resolved, download_dir);

        let _ = std::fs::remove_dir_all(resolved);
    }

    #[test]
    fn snapshot_marks_completed_artifact_existence() {
        let download_dir = test_runtime_dir("snapshot-artifact-existence");
        let existing_path = download_dir.join("exists.pdf");
        std::fs::write(&existing_path, b"done").unwrap();
        let missing_path = download_dir.join("missing.zip");

        let mut existing_job =
            download_job("job_24", JobState::Completed, ResumeSupport::Supported, 100);
        existing_job.target_path = existing_path.display().to_string();
        let mut missing_job =
            download_job("job_25", JobState::Completed, ResumeSupport::Supported, 100);
        missing_job.target_path = missing_path.display().to_string();

        let state = runtime_state_with_jobs(vec![existing_job, missing_job]);
        let snapshot = state.snapshot();

        assert_eq!(snapshot.jobs[0].artifact_exists, Some(true));
        assert_eq!(snapshot.jobs[1].artifact_exists, Some(false));

        let _ = std::fs::remove_dir_all(download_dir);
    }

    #[test]
    fn normalize_job_populates_missing_created_at() {
        let mut job = download_job("job_26", JobState::Queued, ResumeSupport::Unknown, 0);
        job.created_at = 0;

        let normalized = normalize_job(job, &Settings::default());

        assert!(normalized.created_at > 0);
    }

    #[test]
    fn pause_all_jobs_only_pauses_schedulable_jobs() {
        let mut state = runtime_state_with_jobs(vec![
            download_job("job_1", JobState::Queued, ResumeSupport::Unknown, 0),
            download_job("job_2", JobState::Starting, ResumeSupport::Unknown, 0),
            download_job("job_3", JobState::Downloading, ResumeSupport::Supported, 10),
            download_job("job_4", JobState::Completed, ResumeSupport::Supported, 100),
            download_job("job_5", JobState::Failed, ResumeSupport::Supported, 20),
        ]);

        state.pause_all_jobs();

        assert_eq!(state.jobs[0].state, JobState::Paused);
        assert_eq!(state.jobs[1].state, JobState::Paused);
        assert_eq!(state.jobs[2].state, JobState::Paused);
        assert_eq!(state.jobs[2].speed, 0);
        assert_eq!(state.jobs[2].eta, 0);
        assert_eq!(state.jobs[3].state, JobState::Completed);
        assert_eq!(state.jobs[4].state, JobState::Failed);
    }

    #[test]
    fn resume_all_jobs_requeues_interrupted_jobs_and_clears_failures() {
        let mut failed_job = download_job("job_2", JobState::Failed, ResumeSupport::Supported, 20);
        failed_job.error = Some("server closed the connection".into());
        failed_job.failure_category = Some(FailureCategory::Network);
        failed_job.retry_attempts = 2;

        let mut state = runtime_state_with_jobs(vec![
            download_job("job_1", JobState::Paused, ResumeSupport::Unknown, 0),
            failed_job,
            download_job("job_3", JobState::Canceled, ResumeSupport::Unknown, 0),
            download_job("job_4", JobState::Completed, ResumeSupport::Supported, 100),
            download_job("job_5", JobState::Downloading, ResumeSupport::Supported, 10),
        ]);

        state.resume_all_jobs();

        assert_eq!(state.jobs[0].state, JobState::Queued);
        assert_eq!(state.jobs[1].state, JobState::Queued);
        assert_eq!(state.jobs[1].error, None);
        assert_eq!(state.jobs[1].failure_category, None);
        assert_eq!(state.jobs[1].retry_attempts, 0);
        assert_eq!(state.jobs[2].state, JobState::Queued);
        assert_eq!(state.jobs[3].state, JobState::Completed);
        assert_eq!(state.jobs[4].state, JobState::Downloading);
    }

    #[test]
    fn normalize_download_url_trims_pasted_whitespace() {
        let normalized =
            normalize_download_url(" \n https://example.com/file.zip?from=clipboard \t ").unwrap();

        assert_eq!(normalized, "https://example.com/file.zip?from=clipboard");
    }

    #[test]
    fn normalize_download_url_rejects_urls_over_protocol_limit() {
        let long_url = format!("https://example.com/{}", "a".repeat(2_048));

        let error = normalize_download_url(&long_url).unwrap_err();

        assert_eq!(error.code, "URL_TOO_LONG");
    }

    #[test]
    fn sanitize_filename_falls_back_for_dot_only_names() {
        assert_eq!(sanitize_filename("."), "download.bin");
        assert_eq!(sanitize_filename(".."), "download.bin");
        assert_eq!(sanitize_filename("  ...  "), "download.bin");
    }

    #[test]
    fn sanitize_filename_avoids_windows_reserved_device_names() {
        assert_eq!(sanitize_filename("CON"), "CON_");
        assert_eq!(sanitize_filename("con.txt"), "con.txt_");
    }

    #[test]
    fn filename_from_hint_cannot_escape_download_directory_with_parent_segment() {
        let filename = filename_from_hint(Some(".."), "https://example.com/archive.zip");

        assert_eq!(filename, "download.bin");
    }

    #[test]
    fn filename_from_url_decodes_percent_encoded_path_segment() {
        let filename = filename_from_hint(
            None,
            "https://example.com/%5BNanakoRaws%5D%20Tensei%20Shitara%20Slime%20Datta%20Ken%20S4%20-%2002%20%28AT-X%20TV%201080p%20HEVC%20AAC%29.mkv",
        );

        assert_eq!(
            filename,
            "[NanakoRaws] Tensei Shitara Slime Datta Ken S4 - 02 (AT-X TV 1080p HEVC AAC).mkv"
        );
    }

    #[test]
    fn filename_from_browser_hint_decodes_percent_encoded_name() {
        let filename = filename_from_hint(
            Some("%5BASW%5D%20Re%20Zero%20kara%20Hajimeru%20Isekai%20Seikatsu.mkv"),
            "https://example.com/download",
        );

        assert_eq!(filename, "[ASW] Re Zero kara Hajimeru Isekai Seikatsu.mkv");
    }

    #[test]
    fn legacy_default_download_directory_is_replaced_on_load() {
        assert!(should_reset_download_directory("C:/Downloads", false, true));
        assert!(should_reset_download_directory(
            "C:\\Downloads",
            false,
            true
        ));
        assert!(should_reset_download_directory(
            "C:\\Users\\You\\Downloads",
            false,
            true
        ));
        assert!(!should_reset_download_directory(
            "D:/Custom Downloads",
            false,
            true
        ));
    }

    #[test]
    fn normalize_extension_settings_cleans_ignored_file_extensions() {
        let mut settings = ExtensionIntegrationSettings {
            ignored_file_extensions: vec![
                " .ZIP ".into(),
                "zip".into(),
                "tar.gz".into(),
                ".exe".into(),
                "invalid/path".into(),
                String::new(),
            ],
            ..ExtensionIntegrationSettings::default()
        };

        normalize_extension_settings(&mut settings);

        assert_eq!(settings.listen_port, 1420);
        assert_eq!(
            settings.ignored_file_extensions,
            vec!["zip", "tar.gz", "exe"]
        );
    }

    #[test]
    fn normalize_extension_settings_defaults_invalid_listen_port() {
        let mut settings = ExtensionIntegrationSettings {
            listen_port: 70_000,
            ..ExtensionIntegrationSettings::default()
        };

        normalize_extension_settings(&mut settings);

        assert_eq!(settings.listen_port, 1420);
    }

    #[test]
    fn expected_sha256_is_validated_and_normalized() {
        let mixed_case = "A".repeat(64);

        assert_eq!(
            normalize_expected_sha256(Some(mixed_case)).unwrap().as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );

        let error = normalize_expected_sha256(Some("abc123".into())).unwrap_err();
        assert_eq!(error.code, "INVALID_INTEGRITY_HASH");
        assert!(error.message.contains("64 hexadecimal characters"));
    }

    #[tokio::test]
    async fn complete_job_with_matching_sha256_marks_integrity_verified() {
        let download_dir = test_runtime_dir("integrity-match");
        let target_path = download_dir.join("hello.txt");
        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let mut job = download_job("job_30", JobState::Downloading, ResumeSupport::Supported, 0);
        job.target_path = target_path.display().to_string();
        job.temp_path = format!("{}.part", job.target_path);
        job.integrity_check = Some(IntegrityCheck {
            algorithm: IntegrityAlgorithm::Sha256,
            expected: expected.into(),
            actual: None,
            status: IntegrityStatus::Pending,
        });
        let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

        state
            .complete_job_with_integrity("job_30", 5, &target_path, Some(expected.into()))
            .await
            .unwrap();

        let runtime = state.inner.read().await;
        let job = &runtime.jobs[0];
        assert_eq!(job.state, JobState::Completed);
        assert_eq!(
            job.integrity_check.as_ref().map(|check| check.status),
            Some(IntegrityStatus::Verified)
        );
        assert_eq!(
            job.integrity_check
                .as_ref()
                .and_then(|check| check.actual.as_deref()),
            Some(expected)
        );

        let _ = std::fs::remove_dir_all(download_dir);
    }

    #[tokio::test]
    async fn complete_job_with_mismatched_sha256_marks_integrity_failed() {
        let download_dir = test_runtime_dir("integrity-mismatch");
        let target_path = download_dir.join("hello.txt");
        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let actual = "486ea46224d1bb4fb680f34f7c9ad96a8f24ec88be73ea8e5a6c65260e9cb8a7";
        let mut job = download_job("job_31", JobState::Downloading, ResumeSupport::Supported, 0);
        job.target_path = target_path.display().to_string();
        job.temp_path = format!("{}.part", job.target_path);
        job.integrity_check = Some(IntegrityCheck {
            algorithm: IntegrityAlgorithm::Sha256,
            expected: expected.into(),
            actual: None,
            status: IntegrityStatus::Pending,
        });
        let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

        state
            .complete_job_with_integrity("job_31", 5, &target_path, Some(actual.into()))
            .await
            .unwrap();

        let runtime = state.inner.read().await;
        let job = &runtime.jobs[0];
        assert_eq!(job.state, JobState::Failed);
        assert_eq!(job.failure_category, Some(FailureCategory::Integrity));
        assert!(job.error.as_deref().unwrap_or_default().contains("SHA-256"));
        assert_eq!(
            job.integrity_check.as_ref().map(|check| check.status),
            Some(IntegrityStatus::Failed)
        );
        assert_eq!(
            job.integrity_check
                .as_ref()
                .and_then(|check| check.actual.as_deref()),
            Some(actual)
        );

        let _ = std::fs::remove_dir_all(download_dir);
    }

    #[tokio::test]
    async fn diagnostics_keep_newest_hundred_events() {
        let download_dir = test_runtime_dir("diagnostic-events");
        let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);

        for index in 0..105 {
            state
                .record_diagnostic_event(
                    DiagnosticLevel::Info,
                    "test",
                    format!("event {index}"),
                    None,
                )
                .await
                .unwrap();
        }

        let snapshot = state
            .diagnostics_snapshot(HostRegistrationDiagnostics {
                status: HostRegistrationStatus::Configured,
                entries: Vec::new(),
            })
            .await;

        assert_eq!(snapshot.recent_events.len(), 100);
        assert_eq!(snapshot.recent_events[0].message, "event 5");
        assert_eq!(snapshot.recent_events[99].message, "event 104");

        let _ = std::fs::remove_dir_all(download_dir);
    }

    #[test]
    fn restart_reset_clears_partial_progress_and_failure_metadata() {
        let mut job = DownloadJob {
            id: "job_1".into(),
            url: "https://example.com/file.zip".into(),
            filename: "file.zip".into(),
            source: None,
            transfer_kind: TransferKind::Http,
            integrity_check: None,
            state: JobState::Failed,
            created_at: 1,
            progress: 42.0,
            total_bytes: 100,
            downloaded_bytes: 42,
            speed: 2048,
            eta: 12,
            error: Some("server closed the connection".into()),
            failure_category: Some(FailureCategory::Network),
            resume_support: ResumeSupport::Supported,
            retry_attempts: 2,
            target_path: "C:/Downloads/file.zip".into(),
            temp_path: "C:/Downloads/file.zip.part".into(),
            artifact_exists: None,
            bulk_archive: None,
        };

        reset_job_for_restart(&mut job);

        assert_eq!(job.state, JobState::Queued);
        assert_eq!(job.progress, 0.0);
        assert_eq!(job.total_bytes, 0);
        assert_eq!(job.downloaded_bytes, 0);
        assert_eq!(job.speed, 0);
        assert_eq!(job.eta, 0);
        assert_eq!(job.error, None);
        assert_eq!(job.failure_category, None);
        assert_eq!(job.resume_support, ResumeSupport::Unknown);
        assert_eq!(job.retry_attempts, 0);
    }

    #[test]
    fn bulk_archive_status_updates_all_archive_members() {
        let archive = BulkArchiveInfo {
            id: "bulk_1".into(),
            name: "bundle.zip".into(),
            archive_status: BulkArchiveStatus::Pending,
            output_path: None,
            error: None,
        };
        let mut first = download_job("job_1", JobState::Completed, ResumeSupport::Supported, 100);
        let mut second = download_job("job_2", JobState::Completed, ResumeSupport::Supported, 100);
        first.bulk_archive = Some(archive.clone());
        second.bulk_archive = Some(archive);
        let mut state = runtime_state_with_jobs(vec![
            first,
            second,
            download_job("job_3", JobState::Completed, ResumeSupport::Supported, 100),
        ]);

        state.mark_bulk_archive_status_in_memory(
            "bulk_1",
            BulkArchiveStatus::Compressing,
            Some("C:/Downloads/bundle.zip".into()),
            None,
        );

        let mut archive_members = state
            .jobs
            .iter()
            .filter_map(|job| job.bulk_archive.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(archive_members.len(), 2);
        assert!(archive_members
            .iter()
            .all(|archive| archive.archive_status == BulkArchiveStatus::Compressing));
        assert!(archive_members
            .iter()
            .all(|archive| archive.output_path.as_deref() == Some("C:/Downloads/bundle.zip")));

        state.mark_bulk_archive_status_in_memory(
            "bulk_1",
            BulkArchiveStatus::Failed,
            Some("C:/Downloads/bundle.zip".into()),
            Some("zip failed".into()),
        );
        archive_members = state
            .jobs
            .iter()
            .filter_map(|job| job.bulk_archive.as_ref())
            .collect::<Vec<_>>();
        assert!(archive_members
            .iter()
            .all(|archive| archive.archive_status == BulkArchiveStatus::Failed));
        assert!(archive_members
            .iter()
            .all(|archive| archive.error.as_deref() == Some("zip failed")));

        state.mark_bulk_archive_status_in_memory(
            "bulk_1",
            BulkArchiveStatus::Completed,
            Some("C:/Downloads/bundle.zip".into()),
            None,
        );
        archive_members = state
            .jobs
            .iter()
            .filter_map(|job| job.bulk_archive.as_ref())
            .collect::<Vec<_>>();
        assert!(archive_members
            .iter()
            .all(|archive| archive.archive_status == BulkArchiveStatus::Completed));
        assert!(archive_members
            .iter()
            .all(|archive| archive.error.is_none()));
    }

    #[tokio::test]
    async fn remove_active_job_rejects_without_freeing_worker_slot() {
        let download_dir = test_runtime_dir("remove-active-job");
        let mut active_job =
            download_job("job_1", JobState::Downloading, ResumeSupport::Supported, 10);
        active_job.target_path = download_dir.join("active.zip").display().to_string();
        active_job.temp_path = download_dir.join("active.zip.part").display().to_string();
        let mut queued_job = download_job("job_2", JobState::Queued, ResumeSupport::Unknown, 0);
        queued_job.target_path = download_dir.join("queued.zip").display().to_string();
        queued_job.temp_path = download_dir.join("queued.zip.part").display().to_string();
        let state = shared_state_with_jobs(
            download_dir.join("state.json"),
            vec![active_job, queued_job],
        );
        {
            let mut runtime = state.inner.write().await;
            runtime.settings.max_concurrent_downloads = 1;
            runtime.active_workers.insert("job_1".into());
        }

        let error = state.remove_job("job_1").await.unwrap_err();

        assert_eq!(error.code, "INTERNAL_ERROR");
        assert!(error.message.contains("Pause or cancel"));
        let runtime = state.inner.read().await;
        assert!(runtime.active_workers.contains("job_1"));
        assert_eq!(runtime.jobs.len(), 2);
        drop(runtime);

        let (_, tasks) = state
            .claim_schedulable_jobs()
            .await
            .expect("claiming jobs should still work");
        assert!(tasks.is_empty());

        let _ = std::fs::remove_dir_all(download_dir);
    }

    fn download_job(
        id: &str,
        state: JobState,
        resume_support: ResumeSupport,
        downloaded_bytes: u64,
    ) -> DownloadJob {
        DownloadJob {
            id: id.into(),
            url: format!("https://example.com/{id}.zip"),
            filename: format!("{id}.zip"),
            source: None,
            transfer_kind: TransferKind::Http,
            integrity_check: None,
            state,
            created_at: 1,
            progress: 0.0,
            total_bytes: 100,
            downloaded_bytes,
            speed: 0,
            eta: 0,
            error: None,
            failure_category: None,
            resume_support,
            retry_attempts: 0,
            target_path: format!("C:/Downloads/{id}.zip"),
            temp_path: format!("C:/Downloads/{id}.zip.part"),
            artifact_exists: None,
            bulk_archive: None,
        }
    }

    fn runtime_state_with_jobs(jobs: Vec<DownloadJob>) -> RuntimeState {
        RuntimeState {
            connection_state: ConnectionState::Connected,
            jobs,
            settings: Settings::default(),
            main_window: None,
            diagnostic_events: Vec::new(),
            next_job_number: 99,
            active_workers: HashSet::new(),
            last_host_contact: None,
        }
    }

    fn shared_state_with_jobs(storage_path: PathBuf, jobs: Vec<DownloadJob>) -> SharedState {
        SharedState {
            inner: Arc::new(RwLock::new(runtime_state_with_jobs(jobs))),
            storage_path: Arc::new(storage_path),
        }
    }

    fn test_runtime_dir(name: &str) -> PathBuf {
        let dir = std::env::current_dir()
            .unwrap()
            .join("test-runtime")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn target_parent_folder(target_path: &str) -> String {
        PathBuf::from(target_path)
            .parent()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}
