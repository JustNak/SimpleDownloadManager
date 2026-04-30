use cargo_packager_updater::{
    check_update, semver::Version, Config, Update, WindowsConfig, WindowsUpdateInstallMode,
};
use simple_download_manager_desktop_core::contracts::{
    AppUpdateMetadata, UpdateInstallProgressEvent,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

pub const TAURI_TRANSITION_FEED_NAME: &str = "latest-alpha.json";
pub const SLINT_UPDATER_FEED_NAME: &str = "latest-alpha-slint.json";
pub const SLINT_UPDATER_ENDPOINT: &str = "https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-alpha/latest-alpha-slint.json";
pub const TAURI_UPDATER_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEE2NTM3MDYyRDQ3MDNEQ0YKUldUUFBYRFVZbkJUcG5yTXJ5ejlmTVI0aUJTWnlFRHBERDBvc05KQ0M0SVlmODh1b1JqWmhKcTkK";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFeedPlan {
    pub transition_feed_name: &'static str,
    pub slint_feed_name: &'static str,
}

impl Default for UpdateFeedPlan {
    fn default() -> Self {
        Self {
            transition_feed_name: TAURI_TRANSITION_FEED_NAME,
            slint_feed_name: SLINT_UPDATER_FEED_NAME,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCheckMode {
    Startup,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCommandError {
    pub code: &'static str,
    pub message: String,
}

impl UpdateCommandError {
    pub fn no_pending_update() -> Self {
        Self {
            code: "NO_PENDING_UPDATE",
            message: "There is no pending update to install.".into(),
        }
    }

    pub fn updater(error: impl std::fmt::Display) -> Self {
        Self {
            code: "UPDATER_ERROR",
            message: error.to_string(),
        }
    }

    pub fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            code: "INTERNAL_ERROR",
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for UpdateCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for UpdateCommandError {}

#[derive(Debug, Clone, PartialEq)]
pub struct AppUpdateState {
    pub status: String,
    pub available_update: Option<AppUpdateMetadata>,
    pub last_check_mode: Option<UpdateCheckMode>,
    pub error_message: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

impl Default for AppUpdateState {
    fn default() -> Self {
        Self {
            status: "idle".into(),
            available_update: None,
            last_check_mode: None,
            error_message: None,
            downloaded_bytes: 0,
            total_bytes: None,
        }
    }
}

#[derive(Clone, Default)]
pub struct UpdateStateStore {
    state: Arc<Mutex<AppUpdateState>>,
}

impl UpdateStateStore {
    pub fn snapshot(&self) -> AppUpdateState {
        self.state.lock().expect("update state lock").clone()
    }

    pub fn replace(&self, next: AppUpdateState) -> AppUpdateState {
        let mut state = self.state.lock().expect("update state lock");
        *state = next.clone();
        next
    }

    pub fn update(&self, update: impl FnOnce(&AppUpdateState) -> AppUpdateState) -> AppUpdateState {
        let mut state = self.state.lock().expect("update state lock");
        let next = update(&state);
        *state = next.clone();
        next
    }
}

#[derive(Default)]
pub struct PendingUpdateState {
    update: Mutex<Option<Update>>,
}

impl PendingUpdateState {
    fn replace(&self, update: Option<Update>) -> Result<(), UpdateCommandError> {
        let mut pending_update = self.update.lock().map_err(|error| {
            UpdateCommandError::internal(format!("Could not lock pending update: {error}"))
        })?;
        *pending_update = update;
        Ok(())
    }

    fn take(&self) -> Result<Option<Update>, UpdateCommandError> {
        let mut pending_update = self.update.lock().map_err(|error| {
            UpdateCommandError::internal(format!("Could not lock pending update: {error}"))
        })?;
        Ok(pending_update.take())
    }

    pub fn has_pending_update(&self) -> bool {
        self.update
            .lock()
            .map(|pending_update| pending_update.is_some())
            .unwrap_or(false)
    }
}

pub fn updater_config() -> Result<Config, UpdateCommandError> {
    let endpoint = SLINT_UPDATER_ENDPOINT.parse().map_err(|error| {
        UpdateCommandError::internal(format!("Invalid updater endpoint: {error}"))
    })?;

    Ok(Config {
        endpoints: vec![endpoint],
        pubkey: TAURI_UPDATER_PUBLIC_KEY.into(),
        windows: Some(WindowsConfig {
            installer_args: None,
            install_mode: Some(WindowsUpdateInstallMode::Passive),
        }),
    })
}

pub fn metadata_from_update_fields(
    version: impl Into<String>,
    current_version: impl Into<String>,
    date: Option<String>,
    body: Option<String>,
) -> AppUpdateMetadata {
    AppUpdateMetadata {
        version: version.into(),
        current_version: current_version.into(),
        date,
        body,
    }
}

pub fn metadata_for_update(update: Option<&Update>) -> Option<AppUpdateMetadata> {
    update.map(|update| {
        metadata_from_update_fields(
            update.version.to_string(),
            update.current_version.to_string(),
            update.date.map(|date| date.to_string()),
            update.body.clone(),
        )
    })
}

pub fn check_for_update_with(
    pending_update: &PendingUpdateState,
    check: impl FnOnce() -> Result<Option<Update>, UpdateCommandError>,
) -> Result<Option<AppUpdateMetadata>, UpdateCommandError> {
    let update = check()?;
    let metadata = metadata_for_update(update.as_ref());
    pending_update.replace(update)?;
    Ok(metadata)
}

pub fn check_for_update(
    pending_update: &PendingUpdateState,
) -> Result<Option<AppUpdateMetadata>, UpdateCommandError> {
    let current_version: Version = env!("CARGO_PKG_VERSION").parse().map_err(|error| {
        UpdateCommandError::internal(format!("Invalid package version: {error}"))
    })?;
    let config = updater_config()?;
    check_for_update_with(pending_update, || {
        check_update(current_version, config).map_err(UpdateCommandError::updater)
    })
}

pub fn install_update_with_progress(
    pending_update: &PendingUpdateState,
    on_progress: impl FnMut(UpdateInstallProgressEvent) + Send + 'static,
) -> Result<(), UpdateCommandError> {
    let Some(update) = pending_update.take()? else {
        return Err(UpdateCommandError::no_pending_update());
    };

    let on_progress = Arc::new(Mutex::new(on_progress));
    let emitted_started = Arc::new(AtomicBool::new(false));
    let progress_callback = on_progress.clone();
    let progress_started = emitted_started.clone();
    let finished_callback = on_progress;
    update
        .download_and_install_extended(
            move |chunk_length, content_length| {
                if !progress_started.swap(true, Ordering::SeqCst) {
                    if let Ok(mut on_progress) = progress_callback.lock() {
                        on_progress(UpdateInstallProgressEvent::Started { content_length });
                    }
                }
                if let Ok(mut on_progress) = progress_callback.lock() {
                    on_progress(UpdateInstallProgressEvent::Progress { chunk_length });
                }
            },
            move || {
                if let Ok(mut on_progress) = finished_callback.lock() {
                    on_progress(UpdateInstallProgressEvent::Finished);
                }
            },
        )
        .map_err(UpdateCommandError::updater)
}

pub fn start_update_check(state: &AppUpdateState, check_mode: UpdateCheckMode) -> AppUpdateState {
    AppUpdateState {
        status: "checking".into(),
        last_check_mode: Some(check_mode),
        error_message: None,
        downloaded_bytes: 0,
        total_bytes: None,
        ..state.clone()
    }
}

pub fn finish_update_check(
    state: &AppUpdateState,
    update: Option<AppUpdateMetadata>,
) -> AppUpdateState {
    AppUpdateState {
        status: if update.is_some() {
            "available".into()
        } else {
            "not_available".into()
        },
        available_update: update,
        error_message: None,
        downloaded_bytes: 0,
        total_bytes: None,
        ..state.clone()
    }
}

pub fn fail_update_check(state: &AppUpdateState, error: impl Into<String>) -> AppUpdateState {
    AppUpdateState {
        status: "error".into(),
        error_message: Some(error.into()),
        downloaded_bytes: 0,
        total_bytes: None,
        ..state.clone()
    }
}

pub fn finish_silent_startup_update_failure(state: &AppUpdateState) -> AppUpdateState {
    AppUpdateState {
        status: "idle".into(),
        last_check_mode: Some(UpdateCheckMode::Startup),
        error_message: None,
        downloaded_bytes: 0,
        total_bytes: None,
        ..state.clone()
    }
}

pub fn begin_update_install(state: &AppUpdateState) -> AppUpdateState {
    AppUpdateState {
        status: "downloading".into(),
        error_message: None,
        downloaded_bytes: 0,
        total_bytes: None,
        ..state.clone()
    }
}

pub fn fail_update_install(state: &AppUpdateState, error: impl Into<String>) -> AppUpdateState {
    AppUpdateState {
        status: "error".into(),
        error_message: Some(error.into()),
        ..state.clone()
    }
}

pub fn apply_install_progress_event(
    state: &AppUpdateState,
    event: UpdateInstallProgressEvent,
) -> AppUpdateState {
    match event {
        UpdateInstallProgressEvent::Started { content_length } => AppUpdateState {
            status: "downloading".into(),
            downloaded_bytes: 0,
            total_bytes: content_length,
            error_message: None,
            ..state.clone()
        },
        UpdateInstallProgressEvent::Progress { chunk_length } => AppUpdateState {
            status: "downloading".into(),
            downloaded_bytes: state.downloaded_bytes.saturating_add(chunk_length as u64),
            ..state.clone()
        },
        UpdateInstallProgressEvent::Finished => AppUpdateState {
            status: "installing".into(),
            ..state.clone()
        },
    }
}

pub fn progress_percent(downloaded_bytes: u64, total_bytes: Option<u64>) -> f64 {
    let Some(total_bytes) = total_bytes.filter(|total| *total > 0) else {
        return 0.0;
    };

    ((downloaded_bytes as f64 / total_bytes as f64) * 100.0).clamp(0.0, 100.0)
}

pub fn format_update_progress(downloaded_bytes: u64, total_bytes: Option<u64>) -> String {
    match total_bytes {
        Some(total_bytes) if total_bytes > 0 => {
            format!(
                "{} / {}",
                format_bytes(downloaded_bytes),
                format_bytes(total_bytes)
            )
        }
        _ => format!("{} downloaded", format_bytes(downloaded_bytes)),
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let value = bytes as f64;
    if value >= GIB {
        return format!("{:.1} GiB", value / GIB);
    }
    if value >= MIB {
        return format!("{:.1} MiB", value / MIB);
    }
    if value >= KIB {
        return format!("{:.1} KiB", value / KIB);
    }

    format!("{bytes} B")
}
