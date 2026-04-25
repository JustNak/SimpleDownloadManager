use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJob {
    pub id: String,
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub source: Option<DownloadSource>,
    pub state: JobState,
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
    #[serde(default)]
    pub bulk_archive: Option<BulkArchiveInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BulkArchiveInfo {
    pub id: String,
    pub name: String,
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
    pub notifications_enabled: bool,
    pub theme: Theme,
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_directory: default_download_directory(),
            max_concurrent_downloads: 3,
            auto_retry_attempts: default_auto_retry_attempts(),
            speed_limit_kib_per_second: 0,
            notifications_enabled: true,
            theme: Theme::System,
            accent_color: default_accent_color(),
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
            excluded_hosts: Vec::new(),
            ignored_file_extensions: Vec::new(),
        }
    }
}

fn default_auto_retry_attempts() -> u32 {
    3
}

fn default_accent_color() -> String {
    "#3b82f6".into()
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

    let temp_path = state_temp_path(path);
    let backup_path = state_backup_path(path);

    std::fs::write(&temp_path, serialized)
        .map_err(|error| format!("Could not write persisted state: {error}"))?;

    if backup_path.exists() {
        std::fs::remove_file(&backup_path)
            .map_err(|error| format!("Could not clear persisted state backup: {error}"))?;
    }

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

    if backup_path.exists() {
        std::fs::remove_file(&backup_path)
            .map_err(|error| format!("Could not remove persisted state backup: {error}"))?;
    }

    Ok(())
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
        assert_eq!(state.settings.accent_color, "#3b82f6");
        assert_eq!(state.jobs[0].resume_support, ResumeSupport::Unknown);
        assert_eq!(state.jobs[0].failure_category, None);
        assert_eq!(state.jobs[0].retry_attempts, 0);
    }

    #[test]
    fn default_settings_enable_limited_auto_retry() {
        let settings = Settings::default();

        assert!(settings.download_directory.ends_with("Downloads"));
        assert_eq!(settings.auto_retry_attempts, 3);
        assert_eq!(settings.speed_limit_kib_per_second, 0);
        assert_eq!(settings.accent_color, "#3b82f6");
        assert!(!settings.start_on_startup);
        assert_eq!(settings.startup_launch_mode, StartupLaunchMode::Open);
    }

    #[test]
    fn settings_serialize_startup_preferences_as_camel_case() {
        let mut settings = Settings::default();
        settings.start_on_startup = true;
        settings.startup_launch_mode = StartupLaunchMode::Tray;

        let value = serde_json::to_value(settings).expect("settings should serialize");

        assert_eq!(value["startOnStartup"], true);
        assert_eq!(value["startupLaunchMode"], "tray");
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
        assert_eq!(settings.extension_integration.listen_port, 1420);
        assert!(settings.extension_integration.excluded_hosts.is_empty());
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
        assert_eq!(state.settings.extension_integration.listen_port, 1420);
        assert!(state
            .settings
            .extension_integration
            .excluded_hosts
            .is_empty());
        assert!(state
            .settings
            .extension_integration
            .ignored_file_extensions
            .is_empty());
    }
}
