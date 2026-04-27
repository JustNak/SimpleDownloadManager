use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const UPDATE_INSTALL_PROGRESS_EVENT: &str = "app://update-install-progress";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateMetadata {
    pub version: String,
    pub current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
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

    fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            code: "INTERNAL_ERROR",
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum UpdateInstallProgressEvent {
    Started { content_length: Option<u64> },
    Progress { chunk_length: usize },
    Finished,
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
}

pub fn metadata_for_update(update: Option<&Update>) -> Option<AppUpdateMetadata> {
    update.map(|update| AppUpdateMetadata {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        date: update.date.map(|date| date.to_string()),
        body: update.body.clone(),
    })
}

#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    pending_update: State<'_, PendingUpdateState>,
) -> Result<Option<AppUpdateMetadata>, UpdateCommandError> {
    let update = app
        .updater()
        .map_err(UpdateCommandError::updater)?
        .check()
        .await
        .map_err(UpdateCommandError::updater)?;
    let metadata = metadata_for_update(update.as_ref());
    pending_update.replace(update)?;
    Ok(metadata)
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    pending_update: State<'_, PendingUpdateState>,
) -> Result<(), UpdateCommandError> {
    let Some(update) = pending_update.take()? else {
        return Err(UpdateCommandError::no_pending_update());
    };

    let progress_app = app.clone();
    let finished_app = app.clone();
    let mut emitted_started = false;

    update
        .download_and_install(
            move |chunk_length, content_length| {
                if !emitted_started {
                    let _ = progress_app.emit(
                        UPDATE_INSTALL_PROGRESS_EVENT,
                        UpdateInstallProgressEvent::Started { content_length },
                    );
                    emitted_started = true;
                }
                let _ = progress_app.emit(
                    UPDATE_INSTALL_PROGRESS_EVENT,
                    UpdateInstallProgressEvent::Progress { chunk_length },
                );
            },
            move || {
                let _ = finished_app.emit(
                    UPDATE_INSTALL_PROGRESS_EVENT,
                    UpdateInstallProgressEvent::Finished,
                );
            },
        )
        .await
        .map_err(UpdateCommandError::updater)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_metadata_none_is_clean_no_update_result() {
        assert_eq!(metadata_for_update(None), None);
    }

    #[test]
    fn install_without_pending_update_returns_user_readable_error() {
        let error = UpdateCommandError::no_pending_update();

        assert_eq!(error.code, "NO_PENDING_UPDATE");
        assert_eq!(error.message, "There is no pending update to install.");
    }

    #[test]
    fn update_errors_serialize_for_frontend_display() {
        let error = UpdateCommandError::updater("network unavailable");

        let serialized = serde_json::to_value(error).expect("error should serialize");

        assert_eq!(serialized["code"], "UPDATER_ERROR");
        assert_eq!(serialized["message"], "network unavailable");
    }
}
