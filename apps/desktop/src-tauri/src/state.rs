use crate::storage::{
    load_persisted_state, persist_state, ConnectionState, DesktopSnapshot, DownloadJob,
    DiagnosticsSnapshot, DownloadSource, HostRegistrationDiagnostics, JobState, PersistedState,
    QueueSummary, Settings,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use url::Url;

#[derive(Debug, Clone)]
pub struct BackendError {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub target_path: PathBuf,
    pub temp_path: PathBuf,
}

#[derive(Debug)]
struct RuntimeState {
    connection_state: ConnectionState,
    jobs: Vec<DownloadJob>,
    settings: Settings,
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
            .or_else(|| std::env::current_dir().ok().map(|path| path.join("SimpleDownloadManager")))
            .unwrap_or_else(|| std::env::temp_dir().join("SimpleDownloadManager"));

        std::fs::create_dir_all(&base_dir)
            .map_err(|error| format!("Could not create app data directory: {error}"))?;

        let storage_path = base_dir.join("state.json");
        let storage_exists = storage_path.exists();
        let mut persisted = load_persisted_state(&storage_path)?;

        if persisted.settings.download_directory.trim().is_empty()
            || (data_dir_override.is_some() && !storage_exists)
        {
            persisted.settings.download_directory = default_download_directory(&base_dir);
        }

        let jobs = persisted
            .jobs
            .into_iter()
            .map(|job| normalize_job(job, &persisted.settings))
            .collect::<Vec<_>>();
        let next_job_number = next_job_number(&jobs);

        let state = Self {
            inner: Arc::new(RwLock::new(RuntimeState {
                connection_state: ConnectionState::Checking,
                jobs,
                settings: persisted.settings,
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
        }
    }

    pub async fn save_settings(&self, settings: Settings) -> Result<DesktopSnapshot, String> {
        if settings.download_directory.trim().is_empty() {
            return Err("Download directory cannot be empty.".into());
        }

        std::fs::create_dir_all(&settings.download_directory)
            .map_err(|error| format!("Could not create download directory: {error}"))?;

        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.settings = settings;
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn enqueue_download(
        &self,
        url: String,
        source: Option<DownloadSource>,
    ) -> Result<(DesktopSnapshot, String), BackendError> {
        validate_download_url(&url)?;

        let (snapshot, persisted, job_id) = {
            let mut state = self.inner.write().await;

            if state.jobs.iter().any(|job| job.url == url) {
                return Err(BackendError {
                    code: "DUPLICATE_JOB",
                    message: "This URL is already in the queue.".into(),
                });
            }

            let download_dir = PathBuf::from(&state.settings.download_directory);
            if state.settings.download_directory.trim().is_empty() {
                return Err(BackendError {
                    code: "DESTINATION_NOT_CONFIGURED",
                    message: "Configure a download directory before adding downloads.".into(),
                });
            }

            std::fs::create_dir_all(&download_dir).map_err(|error| BackendError {
                code: "DESTINATION_INVALID",
                message: format!("Could not create the download directory: {error}"),
            })?;

            let filename = derive_filename(&url);
            let (target_path, temp_path) = allocate_target_paths(&download_dir, &filename, &state.jobs);
            let job_id = format!("job_{}", state.next_job_number);
            state.next_job_number += 1;

            state.jobs.push(DownloadJob {
                id: job_id.clone(),
                url: url.clone(),
                filename,
                source,
                state: JobState::Queued,
                progress: 0.0,
                total_bytes: 0,
                downloaded_bytes: 0,
                speed: 0,
                eta: 0,
                error: None,
                target_path: target_path.display().to_string(),
                temp_path: temp_path.display().to_string(),
            });

            (state.snapshot(), state.persisted(), job_id)
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok((snapshot, job_id))
    }

    pub async fn pause_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let job = find_job_mut(&mut state.jobs, id)?;
            if matches!(job.state, JobState::Queued | JobState::Starting | JobState::Downloading) {
                job.state = JobState::Paused;
                job.speed = 0;
                job.eta = 0;
            }
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn resume_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let job = find_job_mut(&mut state.jobs, id)?;
            if matches!(job.state, JobState::Paused | JobState::Failed | JobState::Canceled) {
                job.state = JobState::Queued;
                job.error = None;
                job.speed = 0;
                job.eta = 0;
            }
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
            let job = find_job_mut(&mut state.jobs, id)?;
            job.state = JobState::Canceled;
            job.progress = 0.0;
            job.total_bytes = 0;
            job.downloaded_bytes = 0;
            job.speed = 0;
            job.eta = 0;
            job.error = None;
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn retry_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let job = find_job_mut(&mut state.jobs, id)?;
            job.state = JobState::Queued;
            job.speed = 0;
            job.eta = 0;
            job.error = None;
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
                }
            }

            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn clear_completed_jobs(&self) -> Result<DesktopSnapshot, BackendError> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            state.jobs.retain(|job| !matches!(job.state, JobState::Completed | JobState::Canceled));
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn remove_job(&self, id: &str) -> Result<DesktopSnapshot, BackendError> {
        let paths_to_cleanup = {
            let state = self.inner.read().await;
            state
                .jobs
                .iter()
                .find(|job| job.id == id)
                .map(|job| (PathBuf::from(&job.temp_path), job.state))
        };

        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let initial_len = state.jobs.len();
            state.jobs.retain(|job| job.id != id);
            state.active_workers.remove(id);

            if state.jobs.len() == initial_len {
                return Err(BackendError {
                    code: "INTERNAL_ERROR",
                    message: "Job not found.".into(),
                });
            }

            (state.snapshot(), state.persisted())
        };

        if let Some((temp_path, job_state)) = paths_to_cleanup {
            if job_state != JobState::Completed {
                let _ = std::fs::remove_file(temp_path);
            }
        }

        persist_state(&self.storage_path, &persisted).map_err(internal_error)?;
        Ok(snapshot)
    }

    pub async fn claim_schedulable_jobs(&self) -> Result<(DesktopSnapshot, Vec<DownloadTask>), String> {
        let (snapshot, persisted, tasks) = {
            let mut state = self.inner.write().await;
            let available_slots = state
                .settings
                .max_concurrent_downloads
                .max(1)
                .saturating_sub(state.active_workers.len() as u32) as usize;

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
                        target_path: PathBuf::from(&job.target_path),
                        temp_path: PathBuf::from(&job.temp_path),
                    };
                    let task_id = task.id.clone();
                    let _ = job;
                    state.active_workers.insert(task_id);
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

    pub async fn sync_downloaded_bytes(&self, id: &str, downloaded_bytes: u64) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            job.downloaded_bytes = downloaded_bytes;
            if job.total_bytes > 0 {
                job.progress = (downloaded_bytes as f64 / job.total_bytes as f64 * 100.0).clamp(0.0, 100.0);
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
            job.progress = if job.total_bytes == 0 {
                0.0
            } else {
                (job.downloaded_bytes as f64 / job.total_bytes as f64 * 100.0).clamp(0.0, 100.0)
            };
            job.error = None;
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
                job.progress = (job.downloaded_bytes as f64 / job.total_bytes as f64 * 100.0).clamp(0.0, 100.0);
                let remaining = job.total_bytes.saturating_sub(job.downloaded_bytes);
                job.eta = if speed == 0 { 0 } else { ((remaining as f64) / (speed as f64)).ceil() as u64 };
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

    pub async fn complete_job(
        &self,
        id: &str,
        total_bytes: u64,
        target_path: &Path,
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
            state.active_workers.remove(id);
            (state.snapshot(), state.persisted())
        };

        persist_state(&self.storage_path, &persisted)?;
        Ok(snapshot)
    }

    pub async fn fail_job(&self, id: &str, message: impl Into<String>) -> Result<DesktopSnapshot, String> {
        let (snapshot, persisted) = {
            let mut state = self.inner.write().await;
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == id) else {
                return Err("Job not found.".into());
            };

            job.state = JobState::Failed;
            job.speed = 0;
            job.eta = 0;
            job.error = Some(message.into());
            state.active_workers.remove(id);
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
            let job = state.jobs.iter().find(|job| job.id == id).ok_or_else(|| BackendError {
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
                message: "The downloaded file is not available on disk yet.".into(),
            })
        }
    }

    pub async fn resolve_revealable_path(&self, id: &str) -> Result<PathBuf, BackendError> {
        let (target_path, temp_path) = {
            let state = self.inner.read().await;
            let job = state.jobs.iter().find(|job| job.id == id).ok_or_else(|| BackendError {
                code: "INTERNAL_ERROR",
                message: "Job not found.".into(),
            })?;

            (PathBuf::from(&job.target_path), PathBuf::from(&job.temp_path))
        };

        if target_path.exists() {
            return Ok(target_path);
        }

        if temp_path.exists() {
            return Ok(temp_path);
        }

        if let Some(parent) = target_path.parent() {
            if parent.exists() {
                return Ok(parent.to_path_buf());
            }
        }

        Err(BackendError {
            code: "INTERNAL_ERROR",
            message: "No local path is available for this job yet.".into(),
        })
    }

    fn persist_current_state_sync(&self) -> Result<(), String> {
        let state = self.inner.blocking_read();
        persist_state(&self.storage_path, &state.persisted())
    }
}

impl RuntimeState {
    fn snapshot(&self) -> DesktopSnapshot {
        DesktopSnapshot {
            connection_state: self.connection_state,
            jobs: self.jobs.clone(),
            settings: self.settings.clone(),
        }
    }

    fn persisted(&self) -> PersistedState {
        PersistedState {
            jobs: self.jobs.clone(),
            settings: self.settings.clone(),
        }
    }

    fn queue_summary(&self) -> QueueSummary {
        QueueSummary {
            total: self.jobs.len(),
            active: self
                .jobs
                .iter()
                .filter(|job| matches!(job.state, JobState::Queued | JobState::Starting | JobState::Downloading | JobState::Paused))
                .count(),
            queued: self.jobs.iter().filter(|job| job.state == JobState::Queued).count(),
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
            failed: self.jobs.iter().filter(|job| job.state == JobState::Failed).count(),
        }
    }
}

fn normalize_job(mut job: DownloadJob, settings: &Settings) -> DownloadJob {
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

    job
}

fn default_download_directory(base_dir: &Path) -> String {
    base_dir.join("downloads").display().to_string()
}

fn validate_download_url(raw_url: &str) -> Result<(), BackendError> {
    let parsed = Url::parse(raw_url).map_err(|_| BackendError {
        code: "INVALID_URL",
        message: "URL is not valid.".into(),
    })?;

    match parsed.scheme() {
        "http" | "https" => Ok(()),
        _ => Err(BackendError {
            code: "UNSUPPORTED_SCHEME",
            message: "Only http and https URLs are supported.".into(),
        }),
    }
}

fn allocate_target_paths(download_dir: &Path, filename: &str, jobs: &[DownloadJob]) -> (PathBuf, PathBuf) {
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
    let occupied_temps = jobs.iter().map(|job| job.temp_path.clone()).collect::<HashSet<_>>();

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
        .and_then(|segments| segments.last())
        .filter(|segment| !segment.is_empty())
        .unwrap_or("download.bin");

    sanitize_filename(candidate)
}

fn sanitize_filename(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => character,
        })
        .collect();

    if sanitized.trim().is_empty() {
        "download.bin".into()
    } else {
        sanitized
    }
}

fn next_job_number(jobs: &[DownloadJob]) -> u64 {
    jobs.iter()
        .filter_map(|job| job.id.strip_prefix("job_"))
        .filter_map(|value| value.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

fn find_job_mut<'a>(jobs: &'a mut [DownloadJob], id: &str) -> Result<&'a mut DownloadJob, BackendError> {
    jobs.iter_mut().find(|job| job.id == id).ok_or_else(|| BackendError {
        code: "INTERNAL_ERROR",
        message: "Job not found.".into(),
    })
}

fn internal_error(error: String) -> BackendError {
    BackendError {
        code: "INTERNAL_ERROR",
        message: error,
    }
}
