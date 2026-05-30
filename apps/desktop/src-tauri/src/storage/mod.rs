use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static PERSIST_STATE_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const MAX_STATE_BACKUPS: usize = 20;
const MAX_STATE_BACKUP_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

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
pub enum RemovalState {
    Removing,
    CleanupFailed,
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
    #[serde(alias = "browser_blob")]
    BrowserAdopted,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<TorrentRuntimeDiagnostics>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentPeerDiagnostics {
    pub state: String,
    pub fetched_bytes: u64,
    pub errors: u32,
    pub downloaded_pieces: u32,
    pub connection_attempts: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentRuntimeDiagnostics {
    pub queued_peers: u32,
    pub connecting_peers: u32,
    pub live_peers: u32,
    pub seen_peers: u32,
    pub dead_peers: u32,
    pub not_needed_peers: u32,
    pub contributing_peers: u32,
    pub peer_errors: u32,
    #[serde(default)]
    pub peers_with_errors: u32,
    #[serde(default)]
    pub peer_connection_attempts: u32,
    pub session_download_speed: u64,
    pub session_upload_speed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dht_nodes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dht_warmup_age_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_cache_hits: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milliseconds_since_metadata_resolved: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_live_peer_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_contributing_peer_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_payload_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dht_nodes_at_metadata_resolved: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_peer_discovery_assist_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_piece_download_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen_port: Option<u16>,
    pub listener_fallback: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peer_samples: Vec<TorrentPeerDiagnostics>,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HosterPreflightStatus {
    #[default]
    Unchecked,
    Checking,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HosterPreflightInfo {
    pub status: HosterPreflightStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removal_state: Option<RemovalState>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_segments: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_segments: Option<u32>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub failure_category: Option<FailureCategory>,
    #[serde(default)]
    pub resume_support: ResumeSupport,
    #[serde(default)]
    pub retry_attempts: u32,
    #[serde(default)]
    pub auto_restart_attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_from_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hoster_preflight: Option<HosterPreflightInfo>,
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
    #[serde(default, skip_serializing_if = "is_bulk_output_archive")]
    pub output_kind: BulkArchiveOutputKind,
    #[serde(default, skip_serializing_if = "is_bulk_archive_pending")]
    pub archive_status: BulkArchiveStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_extraction: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finalize_total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finalize_processed_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finalize_mode: Option<BulkFinalizeMode>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkArchiveOutputKind {
    #[default]
    Archive,
    Folder,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkArchiveStatus {
    #[default]
    Pending,
    Extracting,
    Combining,
    CreatingFolder,
    Compressing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkFinalizeMode {
    Move,
    Extract,
    Zip,
}

impl BulkArchiveStatus {
    pub fn is_finalizing(self) -> bool {
        matches!(
            self,
            BulkArchiveStatus::Extracting
                | BulkArchiveStatus::Combining
                | BulkArchiveStatus::CreatingFolder
                | BulkArchiveStatus::Compressing
        )
    }
}

fn is_bulk_output_archive(kind: &BulkArchiveOutputKind) -> bool {
    *kind == BulkArchiveOutputKind::Archive
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSettings {
    pub theme: Theme,
    pub accent_color: String,
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
    Balanced,
    #[default]
    Fast,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectedDownloadAuthScope {
    Off,
    Allowlist,
    #[default]
    LegacyGlobal,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TorrentPeerConnectionWatchdogMode {
    #[default]
    Assist,
    Diagnose,
    Recover,
}

impl<'de> Deserialize<'de> for TorrentPeerConnectionWatchdogMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "assist" => Ok(Self::Assist),
            "diagnose" => Ok(Self::Diagnose),
            "recover" | "experimental" => Ok(Self::Recover),
            _ => Err(serde::de::Error::unknown_variant(
                &value,
                &["assist", "diagnose", "recover"],
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentSettings {
    #[serde(default = "default_torrent_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub download_directory: String,
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
    #[serde(default)]
    pub peer_connection_watchdog_mode: TorrentPeerConnectionWatchdogMode,
    #[serde(default = "default_torrent_trackers")]
    pub custom_trackers: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkStartBehavior {
    #[default]
    ReviewThenStart,
    StartImmediately,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkHosterFairnessMode {
    #[default]
    Adaptive,
    Safe,
    Off,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkHosterAccelerationMode {
    #[default]
    Safe,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkDownloadSettings {
    #[serde(default)]
    pub output_directory: String,
    #[serde(default = "default_bulk_max_concurrent_downloads")]
    pub max_concurrent_downloads: u32,
    #[serde(default)]
    pub speed_limit_kib_per_second: u32,
    #[serde(default)]
    pub download_performance_mode: DownloadPerformanceMode,
    #[serde(default)]
    pub hoster_fairness_mode: BulkHosterFairnessMode,
    #[serde(default)]
    pub hoster_acceleration_mode: BulkHosterAccelerationMode,
    #[serde(default)]
    pub auto_retry_override_enabled: bool,
    #[serde(default = "default_auto_retry_attempts")]
    pub auto_retry_attempts: u32,
    #[serde(default)]
    pub start_behavior: BulkStartBehavior,
    #[serde(default)]
    pub expand_active_rows_by_default: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BulkDownloadSettingsWire {
    output_directory: Option<String>,
    max_concurrent_downloads: Option<u32>,
    speed_limit_kib_per_second: Option<u32>,
    download_performance_mode: Option<DownloadPerformanceMode>,
    hoster_fairness_mode: Option<BulkHosterFairnessMode>,
    hoster_acceleration_mode: Option<BulkHosterAccelerationMode>,
    auto_retry_override_enabled: Option<bool>,
    auto_retry_attempts: Option<u32>,
    start_behavior: Option<BulkStartBehavior>,
    expand_active_rows_by_default: Option<bool>,
}

impl BulkDownloadSettingsWire {
    fn into_settings(
        self,
        download_directory: &str,
        fallback_speed_limit_kib_per_second: u32,
        fallback_download_performance_mode: DownloadPerformanceMode,
    ) -> BulkDownloadSettings {
        let defaults = BulkDownloadSettings::for_download_directory(download_directory);
        BulkDownloadSettings {
            output_directory: self.output_directory.unwrap_or(defaults.output_directory),
            max_concurrent_downloads: self
                .max_concurrent_downloads
                .unwrap_or(defaults.max_concurrent_downloads),
            speed_limit_kib_per_second: self
                .speed_limit_kib_per_second
                .unwrap_or(fallback_speed_limit_kib_per_second),
            download_performance_mode: self
                .download_performance_mode
                .unwrap_or(fallback_download_performance_mode),
            hoster_fairness_mode: self
                .hoster_fairness_mode
                .unwrap_or(defaults.hoster_fairness_mode),
            hoster_acceleration_mode: self
                .hoster_acceleration_mode
                .unwrap_or(defaults.hoster_acceleration_mode),
            auto_retry_override_enabled: self
                .auto_retry_override_enabled
                .unwrap_or(defaults.auto_retry_override_enabled),
            auto_retry_attempts: self
                .auto_retry_attempts
                .unwrap_or(defaults.auto_retry_attempts),
            start_behavior: self.start_behavior.unwrap_or(defaults.start_behavior),
            expand_active_rows_by_default: self
                .expand_active_rows_by_default
                .unwrap_or(defaults.expand_active_rows_by_default),
        }
    }
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
    #[serde(default = "default_captured_file_extensions")]
    pub captured_file_extensions: Vec<String>,
    #[serde(default)]
    pub download_capture_debug_logging: bool,
    #[serde(default = "default_authenticated_handoff_enabled")]
    pub authenticated_handoff_enabled: bool,
    #[serde(default = "default_protected_download_auth_scope")]
    pub protected_download_auth_scope: ProtectedDownloadAuthScope,
    #[serde(default = "default_authenticated_handoff_hosts")]
    pub authenticated_handoff_hosts: Vec<String>,
}

const DEFAULT_EXCLUDED_HOSTS: &[&str] = &["web.telegram.org"];
const DEFAULT_PROTECTED_DOWNLOAD_AUTH_HOSTS: &[&str] = &[];
const DEFAULT_CAPTURED_FILE_EXTENSIONS: &[&str] = &[
    "7z", "apk", "bz2", "cab", "csv", "deb", "dmg", "doc", "docx", "exe", "gz", "iso", "jar",
    "msi", "pdf", "ppt", "pptx", "rar", "rpm", "tar", "tgz", "torrent", "txz", "xls", "xlsx", "xz",
    "zip", "zst",
];

fn default_authenticated_handoff_enabled() -> bool {
    true
}

fn default_protected_download_auth_scope() -> ProtectedDownloadAuthScope {
    ProtectedDownloadAuthScope::LegacyGlobal
}

fn default_authenticated_handoff_hosts() -> Vec<String> {
    DEFAULT_PROTECTED_DOWNLOAD_AUTH_HOSTS
        .iter()
        .map(|host| (*host).to_string())
        .collect()
}

fn default_captured_file_extensions() -> Vec<String> {
    DEFAULT_CAPTURED_FILE_EXTENSIONS
        .iter()
        .map(|extension| (*extension).to_string())
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub download_directory: String,
    pub max_concurrent_downloads: u32,
    pub auto_retry_attempts: u32,
    pub speed_limit_kib_per_second: u32,
    pub download_performance_mode: DownloadPerformanceMode,
    pub torrent: TorrentSettings,
    pub bulk: BulkDownloadSettings,
    pub notifications_enabled: bool,
    pub notification_sounds_enabled: bool,
    pub theme: Theme,
    pub accent_color: String,
    pub show_details_on_click: bool,
    pub queue_row_size: QueueRowSize,
    pub start_on_startup: bool,
    pub startup_launch_mode: StartupLaunchMode,
    pub extension_integration: ExtensionIntegrationSettings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SettingsWire {
    download_directory: String,
    max_concurrent_downloads: u32,
    auto_retry_attempts: u32,
    speed_limit_kib_per_second: u32,
    download_performance_mode: DownloadPerformanceMode,
    torrent: TorrentSettings,
    bulk: BulkDownloadSettingsWire,
    notifications_enabled: bool,
    notification_sounds_enabled: bool,
    theme: Theme,
    accent_color: String,
    show_details_on_click: bool,
    queue_row_size: QueueRowSize,
    start_on_startup: bool,
    startup_launch_mode: StartupLaunchMode,
    extension_integration: ExtensionIntegrationSettings,
}

impl Default for SettingsWire {
    fn default() -> Self {
        let settings = Settings::default();
        Self {
            download_directory: settings.download_directory,
            max_concurrent_downloads: settings.max_concurrent_downloads,
            auto_retry_attempts: settings.auto_retry_attempts,
            speed_limit_kib_per_second: settings.speed_limit_kib_per_second,
            download_performance_mode: settings.download_performance_mode,
            torrent: settings.torrent,
            bulk: BulkDownloadSettingsWire::default(),
            notifications_enabled: settings.notifications_enabled,
            notification_sounds_enabled: settings.notification_sounds_enabled,
            theme: settings.theme,
            accent_color: settings.accent_color,
            show_details_on_click: settings.show_details_on_click,
            queue_row_size: settings.queue_row_size,
            start_on_startup: settings.start_on_startup,
            startup_launch_mode: settings.startup_launch_mode,
            extension_integration: settings.extension_integration,
        }
    }
}

impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = SettingsWire::deserialize(deserializer)?;
        let mut bulk = wire.bulk.into_settings(
            &wire.download_directory,
            wire.speed_limit_kib_per_second,
            wire.download_performance_mode,
        );
        normalize_bulk_settings_for_download_directory(&mut bulk, &wire.download_directory);

        Ok(Self {
            download_directory: wire.download_directory,
            max_concurrent_downloads: wire.max_concurrent_downloads,
            auto_retry_attempts: wire.auto_retry_attempts,
            speed_limit_kib_per_second: wire.speed_limit_kib_per_second,
            download_performance_mode: wire.download_performance_mode,
            torrent: wire.torrent,
            bulk,
            notifications_enabled: wire.notifications_enabled,
            notification_sounds_enabled: wire.notification_sounds_enabled,
            theme: wire.theme,
            accent_color: wire.accent_color,
            show_details_on_click: wire.show_details_on_click,
            queue_row_size: wire.queue_row_size,
            start_on_startup: wire.start_on_startup,
            startup_launch_mode: wire.startup_launch_mode,
            extension_integration: wire.extension_integration,
        })
    }
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
    pub torrent_diagnostics: Vec<TorrentJobDiagnostics>,
    pub recent_events: Vec<DiagnosticEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsExport {
    #[serde(flatten)]
    pub snapshot: DiagnosticsSnapshot,
    pub event_history: Vec<DiagnosticEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentJobDiagnostics {
    pub job_id: String,
    pub filename: String,
    pub info_hash: Option<String>,
    pub diagnostics: TorrentRuntimeDiagnostics,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_recovery: Option<StartupRecoverySummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupRecoveryStatus {
    None,
    Recovered,
    NeedsLocalRecovery,
    ResetToDefaults,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupRecoverySummary {
    pub status: StartupRecoveryStatus,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantined_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRecoveryCandidate {
    pub id: String,
    pub path: String,
    pub filename: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRecoveryPreview {
    pub root: String,
    pub candidates: Vec<LocalRecoveryCandidate>,
    pub skipped_count: usize,
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

#[derive(Debug, Clone)]
pub struct PersistedStateLoadResult {
    pub state: PersistedState,
    pub recovery_events: Vec<String>,
    pub allow_initial_persist: bool,
    pub startup_recovery: Option<StartupRecoverySummary>,
}

impl Default for Settings {
    fn default() -> Self {
        let download_directory = default_download_directory();
        Self {
            bulk: BulkDownloadSettings::for_download_directory(&download_directory),
            download_directory,
            max_concurrent_downloads: 3,
            auto_retry_attempts: default_auto_retry_attempts(),
            speed_limit_kib_per_second: 0,
            download_performance_mode: DownloadPerformanceMode::Fast,
            torrent: TorrentSettings::default(),
            notifications_enabled: true,
            notification_sounds_enabled: true,
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

impl BulkDownloadSettings {
    pub fn for_download_directory(download_directory: &str) -> Self {
        Self {
            output_directory: default_bulk_download_directory_for(download_directory),
            ..Self::default()
        }
    }
}

impl Default for BulkDownloadSettings {
    fn default() -> Self {
        Self {
            output_directory: String::new(),
            max_concurrent_downloads: default_bulk_max_concurrent_downloads(),
            speed_limit_kib_per_second: 0,
            download_performance_mode: DownloadPerformanceMode::Fast,
            hoster_fairness_mode: BulkHosterFairnessMode::Adaptive,
            hoster_acceleration_mode: BulkHosterAccelerationMode::Safe,
            auto_retry_override_enabled: false,
            auto_retry_attempts: default_auto_retry_attempts(),
            start_behavior: BulkStartBehavior::ReviewThenStart,
            expand_active_rows_by_default: false,
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
            captured_file_extensions: default_captured_file_extensions(),
            download_capture_debug_logging: false,
            authenticated_handoff_enabled: default_authenticated_handoff_enabled(),
            protected_download_auth_scope: default_protected_download_auth_scope(),
            authenticated_handoff_hosts: default_authenticated_handoff_hosts(),
        }
    }
}

impl Default for TorrentSettings {
    fn default() -> Self {
        Self {
            enabled: default_torrent_enabled(),
            download_directory: String::new(),
            seed_mode: TorrentSeedMode::Forever,
            seed_ratio_limit: default_seed_ratio_limit(),
            seed_time_limit_minutes: default_seed_time_limit_minutes(),
            upload_limit_kib_per_second: 0,
            port_forwarding_enabled: false,
            port_forwarding_port: default_torrent_port_forwarding_port(),
            peer_connection_watchdog_mode: TorrentPeerConnectionWatchdogMode::Assist,
            custom_trackers: default_torrent_trackers(),
        }
    }
}

fn default_auto_retry_attempts() -> u32 {
    3
}

fn default_bulk_max_concurrent_downloads() -> u32 {
    4
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

fn default_torrent_trackers() -> Vec<String> {
    crate::torrent::FALLBACK_TORRENT_TRACKERS
        .iter()
        .map(|tracker| (*tracker).to_string())
        .collect()
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

pub fn default_torrent_download_directory_for(download_directory: &str) -> String {
    Path::new(download_directory.trim())
        .join("Torrent")
        .display()
        .to_string()
}

pub fn default_bulk_download_directory_for(download_directory: &str) -> String {
    let trimmed = download_directory.trim().trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        return "Bulk".into();
    }

    let separator = if trimmed.contains('/') && !trimmed.contains('\\') {
        "/"
    } else if trimmed.contains('\\') {
        "\\"
    } else {
        std::path::MAIN_SEPARATOR_STR
    };

    format!("{trimmed}{separator}Bulk")
}

pub fn normalize_bulk_settings_for_download_directory(
    settings: &mut BulkDownloadSettings,
    download_directory: &str,
) {
    let output_directory = settings.output_directory.trim();
    let default_output_directory = default_bulk_download_directory_for(download_directory);
    let profile_default_output_directory =
        default_bulk_download_directory_for(&default_download_directory());
    settings.output_directory = if output_directory.is_empty()
        || equivalent_settings_path(output_directory, &profile_default_output_directory)
    {
        default_output_directory
    } else {
        output_directory.to_string()
    };
    settings.max_concurrent_downloads = settings.max_concurrent_downloads.max(1);
    settings.speed_limit_kib_per_second = settings.speed_limit_kib_per_second.min(1_048_576);
    settings.auto_retry_attempts = settings.auto_retry_attempts.min(10);
}

fn equivalent_settings_path(left: &str, right: &str) -> bool {
    fn normalize(value: &str) -> String {
        let normalized = value
            .trim()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string();

        if cfg!(windows) {
            normalized.to_ascii_lowercase()
        } else {
            normalized
        }
    }

    normalize(left) == normalize(right)
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

    read_persisted_state_file(path)
}

pub fn load_persisted_state_with_recovery(path: &Path) -> PersistedStateLoadResult {
    if !path.exists() {
        return load_recovery_candidate_or_default(
            path,
            None,
            None,
            "Persisted state is missing; restored download data from a recovery snapshot.",
            true,
        );
    }

    match read_persisted_state_file(path) {
        Ok(state) => PersistedStateLoadResult {
            state,
            recovery_events: Vec::new(),
            allow_initial_persist: true,
            startup_recovery: None,
        },
        Err(error) => {
            let quarantine_result = quarantine_invalid_persisted_state(path);
            let quarantine_succeeded = quarantine_result.is_ok();
            let (quarantined_path, quarantine_event) = match quarantine_result {
                Ok(quarantine_path) => (
                    Some(quarantine_path.clone()),
                    format!(
                        "{error}; quarantined unreadable state as {}",
                        quarantine_path.display()
                    ),
                ),
                Err(quarantine_error) => (
                    None,
                    format!("{error}; could not quarantine unreadable state: {quarantine_error}"),
                ),
            };
            load_recovery_candidate_or_default(
                path,
                quarantined_path,
                Some(quarantine_event),
                "Download data was restored from a recovery snapshot after state.json could not be read.",
                quarantine_succeeded,
            )
        }
    }
}

pub fn persist_state(path: &Path, state: &PersistedState) -> Result<(), String> {
    let serialized = serde_json::to_string_pretty(state)
        .map_err(|error| format!("Could not serialize persisted state: {error}"))?;
    parse_persisted_state_bytes(serialized.as_bytes())
        .map_err(|error| format!("Serialized persisted state did not validate: {error}"))?;
    let _guard = persist_state_write_lock()
        .lock()
        .map_err(|error| format!("Could not lock persisted state writer: {error}"))?;

    let temp_path = state_temp_path(path);
    let backup_path = state_backup_path(path);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create persisted state directory: {error}"))?;
    }

    {
        let mut temp_file = std::fs::File::create(&temp_path)
            .map_err(|error| format!("Could not write persisted state: {error}"))?;
        temp_file
            .write_all(serialized.as_bytes())
            .map_err(|error| format!("Could not write persisted state: {error}"))?;
        temp_file
            .sync_all()
            .map_err(|error| format!("Could not flush persisted state: {error}"))?;
    }
    read_persisted_state_file(&temp_path)
        .map_err(|error| format!("Could not validate persisted state temp file: {error}"))?;

    if path.exists() {
        remove_file_if_exists(&backup_path, "Could not clear persisted state backup")?;
        std::fs::rename(path, &backup_path)
            .map_err(|error| format!("Could not back up persisted state: {error}"))?;
    }

    if let Err(error) = std::fs::rename(&temp_path, path) {
        if !path.exists() && backup_path.exists() {
            let _ = std::fs::rename(&backup_path, path);
        }

        return Err(format!("Could not finalize persisted state: {error}"));
    }

    write_recovery_copies(path)?;

    Ok(())
}

pub async fn persist_state_blocking(path: &Path, state: &PersistedState) -> Result<(), String> {
    let path = path.to_path_buf();
    let state = state.clone();
    tokio::task::spawn_blocking(move || persist_state(&path, &state))
        .await
        .map_err(|error| format!("Persisted state writer task failed: {error}"))?
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

fn read_persisted_state_file(path: &Path) -> Result<PersistedState, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("Could not read persisted state: {error}"))?;
    parse_persisted_state_bytes(&bytes)
        .map_err(|error| format!("Could not parse persisted state: {error}"))
}

fn parse_persisted_state_bytes(bytes: &[u8]) -> Result<PersistedState, String> {
    if bytes.first() == Some(&0) || (!bytes.is_empty() && bytes.iter().all(|byte| *byte == 0)) {
        return Err("state.json is zero-filled or starts with NUL bytes".into());
    }

    let content = std::str::from_utf8(bytes)
        .map_err(|error| format!("persisted state is not UTF-8 JSON: {error}"))?;
    serde_json::from_str::<PersistedState>(content).map_err(|error| error.to_string())
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

fn state_last_good_path(path: &Path) -> PathBuf {
    state_sibling_path(path, "last-good")
}

fn state_backups_dir(path: &Path) -> PathBuf {
    let stem = state_path_stem(path);
    path.with_file_name(format!("{stem}.backups"))
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

fn state_snapshot_path(path: &Path, timestamp: u128, attempt: u32) -> PathBuf {
    let stem = state_path_stem(path);
    let extension = path.extension().and_then(|value| value.to_str());
    let unique = if attempt == 0 {
        timestamp.to_string()
    } else {
        format!("{timestamp}-{attempt}")
    };
    let filename = match extension {
        Some(extension) if !extension.is_empty() => {
            format!("{stem}-{unique}.{extension}")
        }
        _ => format!("{stem}-{unique}"),
    };

    state_backups_dir(path).join(filename)
}

fn state_sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let stem = state_path_stem(path);
    let extension = path.extension().and_then(|value| value.to_str());
    let filename = match extension {
        Some(extension) if !extension.is_empty() => format!("{stem}.{suffix}.{extension}"),
        _ => format!("{stem}.{suffix}"),
    };

    path.with_file_name(filename)
}

fn state_path_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("state")
        .to_string()
}

fn write_recovery_copies(path: &Path) -> Result<(), String> {
    let last_good_path = state_last_good_path(path);
    std::fs::copy(path, &last_good_path)
        .map_err(|error| format!("Could not update last-good persisted state: {error}"))?;

    let backup_dir = state_backups_dir(path);
    std::fs::create_dir_all(&backup_dir)
        .map_err(|error| format!("Could not create persisted state backup directory: {error}"))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let mut snapshot_path = None;
    for attempt in 0..16 {
        let candidate = state_snapshot_path(path, timestamp, attempt);
        if !candidate.exists() {
            snapshot_path = Some(candidate);
            break;
        }
    }
    let snapshot_path = snapshot_path.ok_or_else(|| {
        "Could not find an available persisted state backup filename.".to_string()
    })?;
    std::fs::copy(path, &snapshot_path)
        .map_err(|error| format!("Could not write persisted state backup snapshot: {error}"))?;
    prune_state_snapshot_backups(&backup_dir)?;

    Ok(())
}

fn load_recovery_candidate_or_default(
    primary_path: &Path,
    quarantined_path: Option<PathBuf>,
    primary_recovery_event: Option<String>,
    recovered_message: &str,
    can_persist_recovered_state: bool,
) -> PersistedStateLoadResult {
    let mut recovery_events = primary_recovery_event.into_iter().collect::<Vec<_>>();
    for candidate_path in recovery_candidate_paths(primary_path) {
        match read_persisted_state_file(&candidate_path) {
            Ok(state) => {
                let source_path = candidate_path.display().to_string();
                let mut message = recovered_message.to_string();
                if !can_persist_recovered_state {
                    message.push_str(
                        " The original unreadable state file could not be quarantined, so the recovered state was not written over it.",
                    );
                }
                let startup_recovery = StartupRecoverySummary {
                    status: StartupRecoveryStatus::Recovered,
                    message: message.clone(),
                    source_path: Some(source_path.clone()),
                    quarantined_path: quarantined_path
                        .as_ref()
                        .map(|path| path.display().to_string()),
                };
                recovery_events.push(format!("{message} Source: {source_path}"));
                return PersistedStateLoadResult {
                    state,
                    recovery_events,
                    allow_initial_persist: can_persist_recovered_state,
                    startup_recovery: Some(startup_recovery),
                };
            }
            Err(error) => {
                recovery_events.push(format!(
                    "Could not load persisted state recovery candidate {}: {error}",
                    candidate_path.display()
                ));
            }
        }
    }

    let had_primary_failure = quarantined_path.is_some() || !recovery_events.is_empty();
    if had_primary_failure {
        let message = "Download history could not be read and no valid backup was available. Review local files to restore completed items.".to_string();
        recovery_events.push(format!("{message} local file recovery is available."));
        PersistedStateLoadResult {
            state: PersistedState::default(),
            recovery_events,
            allow_initial_persist: false,
            startup_recovery: Some(StartupRecoverySummary {
                status: StartupRecoveryStatus::NeedsLocalRecovery,
                message,
                source_path: None,
                quarantined_path: quarantined_path.map(|path| path.display().to_string()),
            }),
        }
    } else {
        PersistedStateLoadResult {
            state: PersistedState::default(),
            recovery_events,
            allow_initial_persist: true,
            startup_recovery: None,
        }
    }
}

fn recovery_candidate_paths(primary_path: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![
        state_backup_path(primary_path),
        state_last_good_path(primary_path),
    ];
    candidates.extend(newest_state_snapshot_paths(primary_path));
    candidates
}

fn newest_state_snapshot_paths(primary_path: &Path) -> Vec<PathBuf> {
    let backup_dir = state_backups_dir(primary_path);
    let entries = match std::fs::read_dir(&backup_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut paths = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH);
            Some((path, modified))
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| right.0.cmp(&left.0)));
    paths.into_iter().map(|(path, _)| path).collect()
}

fn prune_state_snapshot_backups(backup_dir: &Path) -> Result<(), String> {
    let entries = match std::fs::read_dir(backup_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "Could not inspect persisted state backup directory: {error}"
            ))
        }
    };
    let now = SystemTime::now();
    let mut valid_snapshots = Vec::new();

    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Could not inspect persisted state backup: {error}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if read_persisted_state_file(&path).is_err() {
            remove_file_if_exists(&path, "Could not remove invalid persisted state backup")?;
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH);
        valid_snapshots.push((path, modified));
    }

    valid_snapshots.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| right.0.cmp(&left.0)));
    for (index, (path, modified)) in valid_snapshots.into_iter().enumerate() {
        let too_many = index >= MAX_STATE_BACKUPS;
        let too_old = now
            .duration_since(modified)
            .map(|age| age > MAX_STATE_BACKUP_AGE)
            .unwrap_or(false);
        if too_many || too_old {
            remove_file_if_exists(&path, "Could not prune old persisted state backup")?;
        }
    }

    Ok(())
}

fn quarantine_invalid_persisted_state(path: &Path) -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();

    for attempt in 0..16 {
        let quarantine_path = state_invalid_path(path, timestamp, attempt);
        if quarantine_path.exists() {
            continue;
        }

        return std::fs::rename(path, &quarantine_path)
            .map(|()| quarantine_path.clone())
            .map_err(|error| format!("Could not move invalid persisted state: {error}"));
    }

    Err("Could not find an available quarantine filename.".into())
}

fn state_invalid_path(path: &Path, timestamp: u128, attempt: u32) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("state");
    let extension = path.extension().and_then(|value| value.to_str());
    let unique = if attempt == 0 {
        timestamp.to_string()
    } else {
        format!("{timestamp}-{attempt}")
    };
    let filename = match extension {
        Some(extension) if !extension.is_empty() => {
            format!("{stem}.invalid-{unique}.{extension}")
        }
        _ => format!("{stem}.invalid-{unique}"),
    };

    path.with_file_name(filename)
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
    fn load_persisted_state_with_recovery_quarantines_invalid_json() {
        let dir = test_runtime_dir("persist-invalid-json-recovery");
        let path = dir.join("state.json");
        std::fs::write(&path, "{not-json").unwrap();

        let result = load_persisted_state_with_recovery(&path);

        assert!(result.state.jobs.is_empty());
        assert!(!result.allow_initial_persist);
        assert_eq!(
            result
                .startup_recovery
                .as_ref()
                .map(|recovery| recovery.status),
            Some(StartupRecoveryStatus::NeedsLocalRecovery)
        );
        assert!(result
            .recovery_events
            .iter()
            .any(|event| event.contains("Could not parse persisted state")));
        assert!(
            !path.exists(),
            "invalid state.json should be moved aside before a replacement is persisted"
        );
        let quarantined = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("state.invalid-") && name.ends_with(".json")
                    })
            })
            .expect("invalid persisted state should be quarantined");
        assert_eq!(std::fs::read_to_string(quarantined).unwrap(), "{not-json");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_persisted_state_with_recovery_restores_last_good_after_zero_filled_primary() {
        let dir = test_runtime_dir("persist-zero-filled-last-good-recovery");
        let path = dir.join("state.json");
        let mut last_good_state = PersistedState::default();
        last_good_state.settings.max_concurrent_downloads = 7;
        std::fs::write(&path, vec![0; 128]).unwrap();
        std::fs::write(
            state_last_good_path(&path),
            serde_json::to_string_pretty(&last_good_state).unwrap(),
        )
        .unwrap();

        let result = load_persisted_state_with_recovery(&path);

        assert_eq!(result.state.settings.max_concurrent_downloads, 7);
        assert!(result.allow_initial_persist);
        let recovery = result
            .startup_recovery
            .expect("zero-filled primary should surface recovery details");
        assert_eq!(recovery.status, StartupRecoveryStatus::Recovered);
        assert!(recovery
            .source_path
            .as_deref()
            .is_some_and(|path| path.ends_with("state.last-good.json")));
        assert!(recovery.quarantined_path.is_some());
        assert!(
            !path.exists(),
            "zero-filled state.json should be moved aside before restoring from last-good"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_persisted_state_with_recovery_requests_local_recovery_without_valid_backups() {
        let dir = test_runtime_dir("persist-zero-filled-needs-local-recovery");
        let path = dir.join("state.json");
        std::fs::write(&path, vec![0; 64]).unwrap();

        let result = load_persisted_state_with_recovery(&path);

        assert!(result.state.jobs.is_empty());
        assert!(!result.allow_initial_persist);
        let recovery = result
            .startup_recovery
            .expect("unrecoverable state should surface recovery details");
        assert_eq!(recovery.status, StartupRecoveryStatus::NeedsLocalRecovery);
        assert!(recovery.quarantined_path.is_some());
        assert!(result
            .recovery_events
            .iter()
            .any(|event| event.contains("local file recovery")));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn persist_state_retains_last_good_and_snapshot_backup() {
        let dir = test_runtime_dir("persist-retained-backups");
        let path = dir.join("state.json");
        let mut state = PersistedState::default();
        state.settings.max_concurrent_downloads = 11;

        persist_state(&path, &state).unwrap();

        let last_good = load_persisted_state(&state_last_good_path(&path))
            .expect("last-good state should be valid JSON");
        assert_eq!(last_good.settings.max_concurrent_downloads, 11);

        let backup_dir = state_backups_dir(&path);
        let snapshots = std::fs::read_dir(&backup_dir)
            .expect("snapshot backup directory should exist")
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(snapshots.len(), 1);
        let snapshot = load_persisted_state(&snapshots[0])
            .expect("snapshot backup should be valid persisted state");
        assert_eq!(snapshot.settings.max_concurrent_downloads, 11);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn persist_state_blocking_writes_recoverable_state() {
        let dir = test_runtime_dir("persist-blocking");
        let path = dir.join("state.json");
        let mut state = PersistedState::default();
        state.settings.max_concurrent_downloads = 9;

        persist_state_blocking(&path, &state).await.unwrap();

        let loaded = load_persisted_state(&path).expect("persisted state should be readable");
        assert_eq!(loaded.settings.max_concurrent_downloads, 9);
        assert!(
            !state_temp_path(&path).exists(),
            "blocking persist should still finalize through the atomic state path"
        );

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
            DownloadPerformanceMode::Fast
        );
        assert_eq!(state.settings.accent_color, "#3b82f6");
        assert_eq!(state.jobs[0].resume_support, ResumeSupport::Unknown);
        assert_eq!(state.jobs[0].failure_category, None);
        assert_eq!(state.jobs[0].retry_attempts, 0);
        assert_eq!(state.jobs[0].auto_restart_attempts, 0);
        assert_eq!(state.jobs[0].resolved_from_url, None);
        assert_eq!(state.jobs[0].hoster_preflight, None);
        assert_eq!(state.jobs[0].transfer_kind, TransferKind::Http);
        assert_eq!(state.jobs[0].integrity_check, None);
        assert!(state.settings.notification_sounds_enabled);
        assert!(state.diagnostic_events.is_empty());
    }

    #[test]
    fn persisted_jobs_round_trip_hoster_preflight_metadata() {
        let state = serde_json::from_str::<PersistedState>(
            r#"{
              "jobs": [{
                "id": "job_1",
                "url": "https://fuckingfast.co/ecw0lw398okf#archive.part01.rar",
                "filename": "archive.part01.rar",
                "state": "paused",
                "progress": 0.0,
                "totalBytes": 0,
                "downloadedBytes": 0,
                "speed": 0,
                "eta": 0,
                "resolvedFromUrl": "https://fuckingfast.co/ecw0lw398okf#archive.part01.rar",
                "hosterPreflight": {
                  "status": "ready",
                  "message": "Validated source page"
                },
                "targetPath": "C:/Downloads/archive.part01.rar",
                "tempPath": "C:/Downloads/archive.part01.rar.part"
              }],
              "settings": {
                "downloadDirectory": "C:/Downloads",
                "maxConcurrentDownloads": 3,
                "notificationsEnabled": true,
                "theme": "system"
              }
            }"#,
        )
        .expect("persisted hoster preflight metadata should parse");

        let preflight = state.jobs[0]
            .hoster_preflight
            .as_ref()
            .expect("hoster preflight metadata should be present");
        assert_eq!(preflight.status, HosterPreflightStatus::Ready);
        assert_eq!(preflight.message.as_deref(), Some("Validated source page"));

        let serialized = serde_json::to_value(&state.jobs[0]).expect("job should serialize");
        assert_eq!(serialized["hosterPreflight"]["status"], "ready");
        assert_eq!(
            serialized["hosterPreflight"]["message"],
            "Validated source page"
        );
    }

    #[test]
    fn persisted_bulk_archives_default_to_archive_output_kind() {
        let state = serde_json::from_str::<PersistedState>(
            r#"{
              "jobs": [{
                "id": "job_1",
                "url": "https://example.com/file.zip",
                "filename": "file.zip",
                "state": "completed",
                "progress": 100.0,
                "totalBytes": 100,
                "downloadedBytes": 100,
                "speed": 0,
                "eta": 0,
                "targetPath": "C:/Downloads/file.zip",
                "tempPath": "C:/Downloads/file.zip.part",
                "bulkArchive": {
                  "id": "bulk_1",
                  "name": "bulk-download.zip",
                  "archiveStatus": "completed",
                  "outputPath": "C:/Downloads/bulk-download.zip"
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
        .expect("old persisted bulk archive metadata should still parse");

        assert_eq!(
            state.jobs[0].bulk_archive.as_ref().unwrap().output_kind,
            BulkArchiveOutputKind::Archive
        );
    }

    #[test]
    fn persisted_bulk_archives_parse_combining_metadata() {
        let state = serde_json::from_str::<PersistedState>(
            r#"{
              "jobs": [{
                "id": "job_1",
                "url": "https://example.com/file.zip",
                "filename": "file.zip",
                "state": "completed",
                "progress": 100.0,
                "totalBytes": 100,
                "downloadedBytes": 100,
                "speed": 0,
                "eta": 0,
                "targetPath": "C:/Downloads/file.zip",
                "tempPath": "C:/Downloads/file.zip.part",
                "bulkArchive": {
                  "id": "bulk_1",
                  "name": "bulk-download.zip",
                  "archiveStatus": "combining",
                  "requiresExtraction": true
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
        .expect("combining bulk archive metadata should parse");

        let archive = state.jobs[0].bulk_archive.as_ref().unwrap();
        assert_eq!(archive.archive_status, BulkArchiveStatus::Combining);
        assert_eq!(archive.requires_extraction, Some(true));
    }

    #[test]
    fn persisted_browser_blob_transfer_kind_migrates_to_browser_adopted() {
        let state = serde_json::from_str::<PersistedState>(
            r#"{
              "jobs": [{
                "id": "job_1",
                "url": "https://example.com/file.zip",
                "filename": "file.zip",
                "transferKind": "browser_blob",
                "state": "completed",
                "progress": 100,
                "totalBytes": 100,
                "downloadedBytes": 100,
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
        )
        .expect("legacy adopted browser rows should still parse");

        assert_eq!(state.jobs[0].transfer_kind, TransferKind::BrowserAdopted);
        let serialized = serde_json::to_value(&state.jobs[0]).expect("job should serialize");
        assert_eq!(serialized["transferKind"], "browser_adopted");
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
        assert_eq!(settings.bulk.max_concurrent_downloads, 4);
        assert_eq!(settings.bulk.speed_limit_kib_per_second, 0);
        assert_eq!(
            settings.bulk.download_performance_mode,
            DownloadPerformanceMode::Fast
        );
        assert_eq!(
            settings.bulk.hoster_fairness_mode,
            BulkHosterFairnessMode::Adaptive
        );
        assert_eq!(
            settings.bulk.hoster_acceleration_mode,
            BulkHosterAccelerationMode::Safe
        );
        assert_eq!(settings.bulk.auto_retry_attempts, 3);
        assert!(!settings.bulk.auto_retry_override_enabled);
        assert_eq!(
            settings.bulk.start_behavior,
            BulkStartBehavior::ReviewThenStart
        );
        assert!(!settings.bulk.expand_active_rows_by_default);
        assert!(
            settings.bulk.output_directory.ends_with("Downloads/Bulk")
                || settings.bulk.output_directory.ends_with(r"Downloads\Bulk"),
            "bulk output directory should default below the main download directory"
        );
        assert_eq!(settings.speed_limit_kib_per_second, 0);
        assert_eq!(
            settings.download_performance_mode,
            DownloadPerformanceMode::Fast
        );
        assert_eq!(settings.accent_color, "#3b82f6");
        assert!(settings.show_details_on_click);
        assert_eq!(settings.queue_row_size, QueueRowSize::Medium);
        assert!(!settings.start_on_startup);
        assert_eq!(settings.startup_launch_mode, StartupLaunchMode::Open);
        assert!(settings.notification_sounds_enabled);
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
        assert_eq!(value["downloadPerformanceMode"], "fast");
        assert_eq!(value["bulk"]["speedLimitKibPerSecond"], 0);
        assert_eq!(value["bulk"]["downloadPerformanceMode"], "fast");
        assert_eq!(value["bulk"]["hosterFairnessMode"], "adaptive");
        assert_eq!(value["bulk"]["hosterAccelerationMode"], "safe");
        assert_eq!(value["showDetailsOnClick"], true);
        assert_eq!(value["queueRowSize"], "medium");
        assert_eq!(value["notificationSoundsEnabled"], true);
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
        assert!(settings.notification_sounds_enabled);
        assert_eq!(settings.bulk.max_concurrent_downloads, 4);
        assert_eq!(
            settings.bulk.start_behavior,
            BulkStartBehavior::ReviewThenStart
        );
        assert_eq!(settings.bulk.output_directory, "C:/Downloads/Bulk");
    }

    #[test]
    fn legacy_bulk_settings_copy_global_runtime_tuning() {
        let settings = serde_json::from_str::<Settings>(
            r#"{
              "downloadDirectory": "C:/Downloads",
              "maxConcurrentDownloads": 3,
              "autoRetryAttempts": 4,
              "speedLimitKibPerSecond": 512,
              "downloadPerformanceMode": "fast",
              "bulk": {
                "outputDirectory": "C:/Downloads/Bulk",
                "maxConcurrentDownloads": 2,
                "autoRetryOverrideEnabled": false,
                "autoRetryAttempts": 3,
                "startBehavior": "review_then_start",
                "expandActiveRowsByDefault": false
              },
              "notificationsEnabled": true,
              "theme": "system"
            }"#,
        )
        .expect("legacy settings should parse");

        assert_eq!(settings.speed_limit_kib_per_second, 512);
        assert_eq!(
            settings.download_performance_mode,
            DownloadPerformanceMode::Fast
        );
        assert_eq!(settings.bulk.speed_limit_kib_per_second, 512);
        assert_eq!(
            settings.bulk.download_performance_mode,
            DownloadPerformanceMode::Fast
        );
        assert_eq!(
            settings.bulk.hoster_fairness_mode,
            BulkHosterFairnessMode::Adaptive
        );
        assert_eq!(
            settings.bulk.hoster_acceleration_mode,
            BulkHosterAccelerationMode::Safe
        );
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
        assert_eq!(
            settings.extension_integration.protected_download_auth_scope,
            ProtectedDownloadAuthScope::LegacyGlobal
        );
        assert_eq!(settings.extension_integration.listen_port, 1420);
        assert_eq!(
            settings.extension_integration.excluded_hosts,
            vec!["web.telegram.org".to_string()]
        );
        assert!(settings
            .extension_integration
            .authenticated_handoff_hosts
            .is_empty());
        assert!(settings
            .extension_integration
            .ignored_file_extensions
            .is_empty());
        assert_eq!(
            settings.extension_integration.captured_file_extensions,
            default_captured_file_extensions()
        );
        assert!(
            !settings
                .extension_integration
                .download_capture_debug_logging
        );
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
        assert_eq!(
            state
                .settings
                .extension_integration
                .protected_download_auth_scope,
            ProtectedDownloadAuthScope::LegacyGlobal
        );
        assert_eq!(state.settings.extension_integration.listen_port, 1420);
        assert_eq!(
            state.settings.extension_integration.excluded_hosts,
            vec!["web.telegram.org".to_string()]
        );
        assert!(state
            .settings
            .extension_integration
            .authenticated_handoff_hosts
            .is_empty());
        assert!(state
            .settings
            .extension_integration
            .ignored_file_extensions
            .is_empty());
        assert_eq!(
            state
                .settings
                .extension_integration
                .captured_file_extensions,
            default_captured_file_extensions()
        );
        assert!(
            !state
                .settings
                .extension_integration
                .download_capture_debug_logging
        );
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
