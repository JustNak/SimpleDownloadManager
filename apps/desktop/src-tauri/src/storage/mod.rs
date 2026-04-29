use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static PERSIST_STATE_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Checking,
    Connected,
    HostMissing,
    AppMissing,
    AppUnreachable,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Starting,
    Downloading,
    Seeding,
    Paused,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    Network,
    Http,
    Server,
    Disk,
    Permission,
    Resume,
    Integrity,
    Torrent,
    Internal,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeSupport {
    #[default]
    Unknown,
    Supported,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferKind {
    #[default]
    Http,
    Torrent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityAlgorithm {
    Sha256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    Pending,
    Verified,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityCheck {
    pub algorithm: IntegrityAlgorithm,
    pub expected: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    pub status: IntegrityStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_id: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_files: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peers: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seeds: Option<u32>,
    #[serde(default)]
    pub uploaded_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_runtime_uploaded_bytes: Option<u64>,
    #[serde(default)]
    pub fetched_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_runtime_fetched_bytes: Option<u64>,
    #[serde(default)]
    pub ratio: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seeding_started_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSource {
    pub entry_point: String,
    pub browser: String,
    pub extension_version: String,
    #[serde(default)]
    pub page_url: Option<String>,
    #[serde(default)]
    pub page_title: Option<String>,
    #[serde(default)]
    pub referrer: Option<String>,
    #[serde(default)]
    pub incognito: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HandoffAuthHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HandoffAuth {
    pub headers: Vec<HandoffAuthHeader>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJob {
    pub id: String,
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub source: Option<DownloadSource>,
    #[serde(default)]
    pub transfer_kind: TransferKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity_check: Option<IntegrityCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent: Option<TorrentInfo>,
    pub state: JobState,
    #[serde(default)]
    pub created_at: u64,
    pub progress: f64,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub downloaded_bytes: u64,
    #[serde(default)]
    pub speed: u64,
    #[serde(default)]
    pub eta: u64,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub failure_category: Option<FailureCategory>,
    #[serde(default)]
    pub resume_support: ResumeSupport,
    #[serde(default)]
    pub retry_attempts: u32,
    #[serde(default)]
    pub target_path: String,
    #[serde(default)]
    pub temp_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_exists: Option<bool>,
    #[serde(default)]
    pub bulk_archive: Option<BulkArchiveInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BulkArchiveInfo {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "is_bulk_archive_pending")]
    pub archive_status: BulkArchiveStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkArchiveStatus {
    #[default]
    Pending,
    Compressing,
    Completed,
    Failed,
}

fn is_bulk_archive_pending(status: &BulkArchiveStatus) -> bool {
    *status == BulkArchiveStatus::Pending
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadPrompt {
    pub id: String,
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub source: Option<DownloadSource>,
    pub total_bytes: Option<u64>,
    pub default_directory: String,
    pub target_path: String,
    pub duplicate_job: Option<DownloadJob>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
    OledDark,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadHandoffMode {
    Off,
    Ask,
    Auto,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupLaunchMode {
    #[default]
    Open,
    Tray,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadPerformanceMode {
    Stable,
    #[default]
    Balanced,
    Fast,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueRowSize {
    Compact,
    Small,
    #[default]
    Medium,
    Large,
    Damn,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TorrentSeedMode {
    #[default]
    Forever,
    Ratio,
    Time,
    RatioOrTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentSettings {
    #[serde(default = "default_torrent_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub seed_mode: TorrentSeedMode,
    #[serde(default = "default_seed_ratio_limit")]
    pub seed_ratio_limit: f64,
    #[serde(default = "default_seed_time_limit_minutes")]
    pub seed_time_limit_minutes: u32,
    #[serde(default)]
    pub upload_limit_kib_per_second: u32,
    #[serde(default)]
    pub port_forwarding_enabled: bool,
    #[serde(default = "default_torrent_port_forwarding_port")]
    pub port_forwarding_port: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionIntegrationSettings {
    pub enabled: bool,
    pub download_handoff_mode: DownloadHandoffMode,
    #[serde(default = "default_extension_listen_port")]
    pub listen_port: u32,
    pub context_menu_enabled: bool,
    pub show_progress_after_handoff: bool,
    pub show_badge_status: bool,
    pub excluded_hosts: Vec<String>,
    #[serde(default)]
    pub ignored_file_extensions: Vec<String>,
    #[serde(default = "default_authenticated_handoff_enabled")]
    pub authenticated_handoff_enabled: bool,
    #[serde(default)]
    pub authenticated_handoff_hosts: Vec<String>,
}

const DEFAULT_EXCLUDED_HOSTS: &[&str] = &["web.telegram.org"];

fn default_authenticated_handoff_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub download_directory: String,
    pub max_concurrent_downloads: u32,
    #[serde(default = "default_auto_retry_attempts")]
    pub auto_retry_attempts: u32,
    #[serde(default)]
    pub speed_limit_kib_per_second: u32,
    #[serde(default)]
    pub download_performance_mode: DownloadPerformanceMode,
    #[serde(default)]
    pub torrent: TorrentSettings,
    pub notifications_enabled: bool,
    pub theme: Theme,
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    #[serde(default = "default_show_details_on_click")]
    pub show_details_on_click: bool,
    #[serde(default)]
    pub queue_row_size: QueueRowSize,
    #[serde(default)]
    pub start_on_startup: bool,
    #[serde(default)]
    pub startup_launch_mode: StartupLaunchMode,
    #[serde(default)]
    pub extension_integration: ExtensionIntegrationSettings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSummary {
    pub total: usize,
    pub active: usize,
    pub attention: usize,
    pub queued: usize,
    pub downloading: usize,
    pub completed: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSnapshot {
    pub connection_state: ConnectionState,
    pub queue_summary: QueueSummary,
    pub last_host_contact_seconds_ago: Option<u64>,
    pub host_registration: HostRegistrationDiagnostics,
    pub recent_events: Vec<DiagnosticEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    pub timestamp: u64,
    pub level: DiagnosticLevel,
    pub category: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostRegistrationDiagnostics {
    pub status: HostRegistrationStatus,
    pub entries: Vec<HostRegistrationEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRegistrationStatus {
    Configured,
    Missing,
    Broken,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostRegistrationEntry {
    pub browser: String,
    pub registry_path: String,
    pub manifest_path: Option<String>,
    pub manifest_exists: bool,
    pub host_binary_path: Option<String>,
    pub host_binary_exists: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSnapshot {
    pub connection_state: ConnectionState,
    pub jobs: Vec<DownloadJob>,
    pub settings: Settings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MainWindowState {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub maximized: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub jobs: Vec<DownloadJob>,
    pub settings: Settings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_window: Option<MainWindowState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostic_events: Vec<DiagnosticEvent>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_directory: default_download_directory(),
            max_concurrent_downloads: 3,
            auto_retry_attempts: default_auto_retry_attempts(),
            speed_limit_kib_per_second: 0,
            download_performance_mode: DownloadPerformanceMode::Balanced,
            torrent: TorrentSettings::default(),
            notifications_enabled: true,
            theme: Theme::System,
            accent_color: default_accent_color(),
            show_details_on_click: default_show_details_on_click(),
            queue_row_size: QueueRowSize::Medium,
            start_on_startup: false,
            startup_launch_mode: StartupLaunchMode::Open,
            extension_integration: ExtensionIntegrationSettings::default(),
        }
    }
}

impl Default for ExtensionIntegrationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            download_handoff_mode: DownloadHandoffMode::Ask,
            listen_port: default_extension_listen_port(),
            context_menu_enabled: true,
            show_progress_after_handoff: true,
            show_badge_status: true,
            excluded_hosts: DEFAULT_EXCLUDED_HOSTS
                .iter()
                .map(|host| (*host).to_string())
                .collect(),
            ignored_file_extensions: Vec::new(),
            authenticated_handoff_enabled: default_authenticated_handoff_enabled(),
            authenticated_handoff_hosts: Vec::new(),
        }
    }
}

impl Default for TorrentSettings {
    fn default() -> Self {
        Self {
            enabled: default_torrent_enabled(),
            seed_mode: TorrentSeedMode::Forever,
            seed_ratio_limit: default_seed_ratio_limit(),
            seed_time_limit_minutes: default_seed_time_limit_minutes(),
            upload_limit_kib_per_second: 0,
            port_forwarding_enabled: false,
            port_forwarding_port: default_torrent_port_forwarding_port(),
        }
    }
}

fn default_auto_retry_attempts() -> u32 {
    3
}

fn default_torrent_enabled() -> bool {
    true
}

fn default_seed_ratio_limit() -> f64 {
    1.0
}

fn default_seed_time_limit_minutes() -> u32 {
    60
}

pub fn default_torrent_port_forwarding_port() -> u32 {
    42_000
}

fn default_accent_color() -> String {
    "#3b82f6".into()
}

fn default_show_details_on_click() -> bool {
    true
}

pub fn default_extension_listen_port() -> u32 {
    1420
}

pub fn default_download_directory() -> String {
    default_download_directory_path().display().to_string()
}

fn default_download_directory_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(user_profile) = std::env::var_os("USERPROFILE") {
            return download_directory_for_user_profile(Path::new(&user_profile));
        }
    }

    dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|path| path.join("Downloads")))
        .unwrap_or_else(|| PathBuf::from("Downloads"))
}

fn download_directory_for_user_profile(user_profile: &Path) -> PathBuf {
    user_profile.join("Downloads")
}

pub fn load_persisted_state(path: &Path) -> Result<PersistedState, String> {
    recover_backup_state(path)?;

    if !path.exists() {
        return Ok(PersistedState::default());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("Could not read persisted state: {error}"))?;

    serde_json::from_str::<PersistedState>(&content)
        .map_err(|error| format!("Could not parse persisted state: {error}"))
}

pub fn persist_state(path: &Path, state: &PersistedState) -> Result<(), String> {
    let serialized = serde_json::to_string_pretty(state)
        .map_err(|error| format!("Could not serialize persisted state: {error}"))?;
    let _guard = persist_state_write_lock()
        .lock()
        .map_err(|error| format!("Could not lock persisted state writer: {error}"))?;

    let temp_path = state_temp_path(path);
    let backup_path = state_backup_path(path);

    std::fs::write(&temp_path, serialized)
        .map_err(|error| format!("Could not write persisted state: {error}"))?;

    remove_file_if_exists(&backup_path, "Could not clear persisted state backup")?;

    if path.exists() {
        std::fs::rename(path, &backup_path)
            .map_err(|error| format!("Could not back up persisted state: {error}"))?;
    }

    if let Err(error) = std::fs::rename(&temp_path, path) {
        if !path.exists() && backup_path.exists() {
            let _ = std::fs::rename(&backup_path, path);
        }

        return Err(format!("Could not finalize persisted state: {error}"));
    }

    remove_file_if_exists(&backup_path, "Could not remove persisted state backup")?;

    Ok(())
}

fn persist_state_write_lock() -> &'static Mutex<()> {
    PERSIST_STATE_WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

fn remove_file_if_exists(path: &Path, context: &str) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{context}: {error}")),
    }
}

fn state_temp_path(path: &Path) -> PathBuf {
    let mut extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();

    if extension.is_empty() {
        extension = "tmp".into();
    } else {
        extension.push_str(".tmp");
    }

    path.with_extension(extension)
}

fn state_backup_path(path: &Path) -> PathBuf {
    let mut extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();

    if extension.is_empty() {
        extension = "bak".into();
    } else {
        extension.push_str(".bak");
    }

    path.with_extension(extension)
}

fn recover_backup_state(path: &Path) -> Result<(), String> {
    let backup_path = state_backup_path(path);

    if path.exists() || !backup_path.exists() {
        return Ok(());
    }

    std::fs::rename(&backup_path, path)
        .map_err(|error| format!("Could not restore persisted state backup: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persist_state_serializes_concurrent_writes_to_same_path() {
        let dir = test_runtime_dir("persist-concurrent");
        let path = dir.join("state.json");
        let workers = 8;
        let iterations = 20;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(workers));

        let handles = (0..workers)
            .map(|worker| {
                let path = path.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || -> Result<(), String> {
                    barrier.wait();
                    for iteration in 0..iterations {
                        let mut state = PersistedState::default();
                        state.settings.max_concurrent_downloads =
                            1 + ((worker + iteration) % 12) as u32;
                        persist_state(&path, &state)?;
                    }
                    Ok(())
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle
                .join()
                .expect("writer thread should not panic")
                .unwrap();
        }

        load_persisted_state(&path).expect("final state should stay readable");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn remove_file_if_exists_ignores_missing_backup_cleanup() {
        let dir = test_runtime_dir("persist-missing-backup-cleanup");
        let backup_path = dir.join("state.json.bak");

        remove_file_if_exists(&backup_path, "Could not remove persisted state backup")
            .expect("missing cleanup target should not be fatal");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn persisted_state_defaults_reliability_fields_for_existing_files() {
        let state = serde_json::from_str::<PersistedState>(
            r#"{
              "jobs": [{
                "id": "job_1",
                "url": "https://example.com/file.zip",
                "filename": "file.zip",
                "state": "failed",
                "progress": 12.0,
                "totalBytes": 100,
                "downloadedBytes": 12,
                "speed": 0,
                "eta": 0,
                "error": "Download failed",
                "targetPath": "C:/Downloads/file.zip",
                "tempPath": "C:/Downloads/file.zip.part"
              }],
              "settings": {
                "downloadDirectory": "C:/Downloads",
                "maxConcurrentDownloads": 3,
                "notificationsEnabled": true,
                "theme": "system"
              }
            }"#,
        )
        .expect("old persisted state should still parse");

        assert_eq!(state.settings.auto_retry_attempts, 3);
        assert_eq!(state.settings.speed_limit_kib_per_second, 0);
        assert_eq!(
            state.settings.download_performance_mode,
            DownloadPerformanceMode::Balanced
        );
        assert_eq!(state.settings.accent_color, "#3b82f6");
        assert_eq!(state.jobs[0].resume_support, ResumeSupport::Unknown);
        assert_eq!(state.jobs[0].failure_category, None);
        assert_eq!(state.jobs[0].retry_attempts, 0);
        assert_eq!(state.jobs[0].transfer_kind, TransferKind::Http);
        assert_eq!(state.jobs[0].integrity_check, None);
        assert!(state.diagnostic_events.is_empty());
    }

    #[test]
    fn persisted_jobs_reject_unknown_future_transfer_kind() {
        let state = serde_json::from_str::<PersistedState>(
            r#"{
              "jobs": [{
                "id": "job_1",
                "url": "https://example.com/file.zip",
                "filename": "file.zip",
                "transferKind": "future_kind",
                "state": "queued",
                "progress": 0,
                "totalBytes": 0,
                "downloadedBytes": 0,
                "speed": 0,
                "eta": 0,
                "targetPath": "C:/Downloads/file.zip",
                "tempPath": "C:/Downloads/file.zip.part"
              }],
              "settings": {
                "downloadDirectory": "C:/Downloads",
                "maxConcurrentDownloads": 3,
                "notificationsEnabled": true,
                "theme": "system"
              }
            }"#,
        );

        assert!(
            state.is_err(),
            "unknown transfer kinds should not silently run"
        );
    }

    #[test]
    fn torrent_jobs_persist_metadata_and_seeding_state() {
        let state = serde_json::from_str::<PersistedState>(
            r#"{
              "jobs": [{
                "id": "job_7",
                "url": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example",
                "filename": "Example",
                "transferKind": "torrent",
                "state": "seeding",
                "progress": 100,
                "totalBytes": 1024,
                "downloadedBytes": 1024,
                "speed": 0,
                "eta": 0,
                "targetPath": "C:/Downloads/Example",
                "tempPath": "C:/Downloads/.torrent-state/job_7",
                "torrent": {
                  "infoHash": "0123456789abcdef0123456789abcdef01234567",
                  "name": "Example",
                  "totalFiles": 2,
                  "peers": 3,
                  "seeds": 4,
                  "uploadedBytes": 2048,
                  "fetchedBytes": 1536,
                  "ratio": 2.0,
                  "seedingStartedAt": 123456
                }
              }],
              "settings": {
                "downloadDirectory": "C:/Downloads",
                "maxConcurrentDownloads": 3,
                "notificationsEnabled": true,
                "theme": "system"
              }
            }"#,
        )
        .expect("torrent job should parse");

        let job = &state.jobs[0];
        assert_eq!(job.transfer_kind, TransferKind::Torrent);
        assert_eq!(job.state, JobState::Seeding);
        let torrent = job.torrent.as_ref().expect("torrent metadata");
        assert_eq!(
            torrent.info_hash.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(torrent.total_files, Some(2));
        assert_eq!(torrent.uploaded_bytes, 2048);
        assert_eq!(torrent.last_runtime_uploaded_bytes, None);
        assert_eq!(torrent.fetched_bytes, 1536);
        assert_eq!(torrent.last_runtime_fetched_bytes, None);
        assert_eq!(torrent.ratio, 2.0);
        assert_eq!(torrent.seeding_started_at, Some(123456));
    }

    #[test]
    fn torrent_jobs_round_trip_runtime_transfer_counters() {
        let state = serde_json::from_str::<PersistedState>(
            r#"{
              "jobs": [{
                "id": "job_8",
                "url": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example",
                "filename": "Example",
                "transferKind": "torrent",
                "state": "seeding",
                "progress": 100,
                "totalBytes": 1024,
                "downloadedBytes": 1024,
                "speed": 0,
                "eta": 0,
                "targetPath": "C:/Downloads/Example",
                "tempPath": "C:/Downloads/.torrent-state/job_8",
                "torrent": {
                  "infoHash": "0123456789abcdef0123456789abcdef01234567",
                  "uploadedBytes": 2048,
                  "lastRuntimeUploadedBytes": 512,
                  "fetchedBytes": 4096,
                  "lastRuntimeFetchedBytes": 1024,
                  "ratio": 2.0
                }
              }],
              "settings": {
                "downloadDirectory": "C:/Downloads",
                "maxConcurrentDownloads": 3,
                "notificationsEnabled": true,
                "theme": "system"
              }
            }"#,
        )
        .expect("torrent job should parse");

        let serialized = serde_json::to_value(&state).expect("state should serialize");

        assert_eq!(
            serialized["jobs"][0]["torrent"]["lastRuntimeUploadedBytes"],
            serde_json::json!(512)
        );
        assert_eq!(
            serialized["jobs"][0]["torrent"]["fetchedBytes"],
            serde_json::json!(4096)
        );
        assert_eq!(
            serialized["jobs"][0]["torrent"]["lastRuntimeFetchedBytes"],
            serde_json::json!(1024)
        );
    }

    #[test]
    fn default_settings_enable_limited_auto_retry() {
        let settings = Settings::default();

        assert!(settings.download_directory.ends_with("Downloads"));
        assert_eq!(settings.auto_retry_attempts, 3);
        assert_eq!(settings.speed_limit_kib_per_second, 0);
        assert_eq!(
            settings.download_performance_mode,
            DownloadPerformanceMode::Balanced
        );
        assert_eq!(settings.accent_color, "#3b82f6");
        assert!(settings.show_details_on_click);
        assert_eq!(settings.queue_row_size, QueueRowSize::Medium);
        assert!(!settings.start_on_startup);
        assert_eq!(settings.startup_launch_mode, StartupLaunchMode::Open);
    }

    #[test]
    fn settings_serialize_startup_preferences_as_camel_case() {
        let settings = Settings {
            start_on_startup: true,
            startup_launch_mode: StartupLaunchMode::Tray,
            ..Settings::default()
        };

        let value = serde_json::to_value(settings).expect("settings should serialize");

        assert_eq!(value["startOnStartup"], true);
        assert_eq!(value["startupLaunchMode"], "tray");
        assert_eq!(value["downloadPerformanceMode"], "balanced");
        assert_eq!(value["showDetailsOnClick"], true);
        assert_eq!(value["queueRowSize"], "medium");
    }

    #[test]
    fn missing_view_settings_default_for_existing_users() {
        let settings = serde_json::from_str::<Settings>(
            r#"{
              "downloadDirectory": "C:/Downloads",
              "maxConcurrentDownloads": 3,
              "notificationsEnabled": true,
              "theme": "system"
            }"#,
        )
        .expect("legacy settings should parse");

        assert!(settings.show_details_on_click);
        assert_eq!(settings.queue_row_size, QueueRowSize::Medium);
    }

    #[test]
    fn user_profile_download_directory_targets_downloads_folder() {
        let path = download_directory_for_user_profile(Path::new(r"C:\Users\Alice"));

        assert_eq!(path, PathBuf::from(r"C:\Users\Alice\Downloads"));
    }

    #[test]
    fn default_settings_enable_browser_handoff_prompt_controls() {
        let settings = Settings::default();

        assert!(settings.extension_integration.enabled);
        assert_eq!(
            settings.extension_integration.download_handoff_mode,
            DownloadHandoffMode::Ask
        );
        assert!(settings.extension_integration.context_menu_enabled);
        assert!(settings.extension_integration.show_progress_after_handoff);
        assert!(settings.extension_integration.show_badge_status);
        assert!(settings.extension_integration.authenticated_handoff_enabled);
        assert_eq!(settings.extension_integration.listen_port, 1420);
        assert_eq!(
            settings.extension_integration.excluded_hosts,
            vec!["web.telegram.org".to_string()]
        );
        assert!(settings
            .extension_integration
            .ignored_file_extensions
            .is_empty());
    }

    #[test]
    fn persisted_state_defaults_extension_settings_for_existing_files() {
        let state = serde_json::from_str::<PersistedState>(
            r#"{
              "jobs": [],
              "settings": {
                "downloadDirectory": "C:/Downloads",
                "maxConcurrentDownloads": 3,
                "autoRetryAttempts": 3,
                "speedLimitKibPerSecond": 0,
                "notificationsEnabled": true,
                "theme": "system"
              }
            }"#,
        )
        .expect("old persisted state should still parse");

        assert!(state.settings.extension_integration.enabled);
        assert_eq!(
            state.settings.extension_integration.download_handoff_mode,
            DownloadHandoffMode::Ask
        );
        assert!(state.settings.extension_integration.context_menu_enabled);
        assert!(
            state
                .settings
                .extension_integration
                .show_progress_after_handoff
        );
        assert!(state.settings.extension_integration.show_badge_status);
        assert!(
            state
                .settings
                .extension_integration
                .authenticated_handoff_enabled
        );
        assert_eq!(state.settings.extension_integration.listen_port, 1420);
        assert_eq!(
            state.settings.extension_integration.excluded_hosts,
            vec!["web.telegram.org".to_string()]
        );
        assert!(state
            .settings
            .extension_integration
            .ignored_file_extensions
            .is_empty());
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
}
