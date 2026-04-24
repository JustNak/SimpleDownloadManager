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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeSupport {
    Unknown,
    Supported,
    Unsupported,
}

impl Default for ResumeSupport {
    fn default() -> Self {
        Self::Unknown
    }
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub download_directory: String,
    pub max_concurrent_downloads: u32,
    #[serde(default = "default_auto_retry_attempts")]
    pub auto_retry_attempts: u32,
    pub notifications_enabled: bool,
    pub theme: Theme,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub jobs: Vec<DownloadJob>,
    pub settings: Settings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_directory: "C:/Downloads".into(),
            max_concurrent_downloads: 3,
            auto_retry_attempts: default_auto_retry_attempts(),
            notifications_enabled: true,
            theme: Theme::System,
        }
    }
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            jobs: Vec::new(),
            settings: Settings::default(),
        }
    }
}

fn default_auto_retry_attempts() -> u32 {
    3
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
        assert_eq!(state.jobs[0].resume_support, ResumeSupport::Unknown);
        assert_eq!(state.jobs[0].failure_category, None);
        assert_eq!(state.jobs[0].retry_attempts, 0);
    }

    #[test]
    fn default_settings_enable_limited_auto_retry() {
        let settings = Settings::default();

        assert_eq!(settings.auto_retry_attempts, 3);
    }
}
