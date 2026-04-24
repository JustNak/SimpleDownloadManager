use crate::commands::emit_snapshot;
use crate::download::schedule_downloads;
use crate::state::{BackendError, SharedState};
use crate::storage::{
    ConnectionState, DownloadSource, HostRegistrationDiagnostics, HostRegistrationEntry,
    HostRegistrationStatus, QueueSummary,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

#[cfg(windows)]
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use winreg::enums::HKEY_CURRENT_USER;

#[cfg(windows)]
use winreg::RegKey;

pub const PIPE_NAME: &str = r"\\.\pipe\myapp.downloads.v1";
const PROTOCOL_VERSION: u32 = 1;
#[cfg(windows)]
const HOST_CONTACT_TTL: Duration = Duration::from_secs(20);
#[cfg(windows)]
const DIAGNOSTIC_POLL_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(windows)]
const NATIVE_HOST_REGISTRY_KEYS: [&str; 3] = [
    r"Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager",
    r"Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager",
    r"Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostRequest {
    protocol_version: u32,
    request_id: String,
    #[serde(rename = "type")]
    message_type: String,
    payload: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnqueueSource {
    entry_point: String,
    browser: String,
    extension_version: String,
    page_url: Option<String>,
    page_title: Option<String>,
    referrer: Option<String>,
    incognito: Option<bool>,
}

impl From<EnqueueSource> for DownloadSource {
    fn from(value: EnqueueSource) -> Self {
        Self {
            entry_point: value.entry_point,
            browser: value.browser,
            extension_version: value.extension_version,
            page_url: value.page_url,
            page_title: value.page_title,
            referrer: value.referrer,
            incognito: value.incognito,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnqueuePayload {
    url: String,
    source: EnqueueSource,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HostResponse {
    ok: bool,
    request_id: String,
    #[serde(rename = "type")]
    message_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl HostResponse {
    fn ready(request_id: String, app_state: &str, connection_state: ConnectionState, queue_summary: QueueSummary) -> Self {
        Self {
            ok: true,
            request_id,
            message_type: "ready".into(),
            payload: Some(json!({
                "appState": app_state,
                "connectionState": connection_state,
                "queueSummary": queue_summary,
            })),
            code: None,
            message: None,
        }
    }

    fn queued(request_id: String, job_id: String) -> Self {
        Self {
            ok: true,
            request_id,
            message_type: "queued".into(),
            payload: Some(json!({ "jobId": job_id, "status": "queued" })),
            code: None,
            message: None,
        }
    }

    fn error(request_id: String, message_type: &str, code: &'static str, message: String) -> Self {
        Self {
            ok: false,
            request_id,
            message_type: message_type.into(),
            payload: None,
            code: Some(code),
            message: Some(message),
        }
    }
}

#[cfg(windows)]
pub fn start_named_pipe_listener(app: AppHandle, state: SharedState) {
    let listener_app = app.clone();
    let listener_state = state.clone();
    tauri::async_runtime::spawn(async move {
        refresh_connection_diagnostics(&listener_app, &listener_state).await;

        loop {
            if let Err(error) = accept_single_connection(listener_app.clone(), listener_state.clone()).await {
                eprintln!("named pipe listener error: {error}");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    });

    let diagnostics_app = app;
    let diagnostics_state = state;
    tauri::async_runtime::spawn(async move {
        loop {
            refresh_connection_diagnostics(&diagnostics_app, &diagnostics_state).await;
            tokio::time::sleep(DIAGNOSTIC_POLL_INTERVAL).await;
        }
    });
}

#[cfg(not(windows))]
pub fn start_named_pipe_listener(_app: AppHandle, _state: SharedState) {}

#[cfg(windows)]
async fn accept_single_connection(app: AppHandle, state: SharedState) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ServerOptions;

    let server = ServerOptions::new()
        .create(PIPE_NAME)
        .map_err(|error| format!("Could not create named pipe server: {error}"))?;

    server
        .connect()
        .await
        .map_err(|error| format!("Could not accept named pipe connection: {error}"))?;

    tauri::async_runtime::spawn(async move {
        let result: Result<(), String> = async {
            let (reader, mut writer) = tokio::io::split(server);
            let mut reader = BufReader::new(reader);
            let mut request_line = String::new();
            reader
                .read_line(&mut request_line)
                .await
                .map_err(|error| format!("Could not read named pipe payload: {error}"))?;

            if request_line.trim().is_empty() {
                return Ok(());
            }

            let request = serde_json::from_str::<HostRequest>(&request_line)
                .map_err(|error| format!("Could not parse host request: {error}"))?;

            let response = handle_request(app, state, request).await;
            let response_json = serde_json::to_string(&response)
                .map_err(|error| format!("Could not serialize host response: {error}"))?;

            writer
                .write_all(response_json.as_bytes())
                .await
                .map_err(|error| format!("Could not write named pipe response: {error}"))?;

            writer
                .write_all(b"\n")
                .await
                .map_err(|error| format!("Could not write named pipe response terminator: {error}"))?;

            Ok(())
        }
        .await;

        if let Err(error) = result {
            eprintln!("named pipe request error: {error}");
        }
    });

    Ok(())
}

async fn handle_request(app: AppHandle, state: SharedState, request: HostRequest) -> HostResponse {
    if request.protocol_version != PROTOCOL_VERSION {
        return HostResponse::error(
            request.request_id,
            "invalid_payload",
            "HOST_PROTOCOL_MISMATCH",
            format!(
                "Expected protocol version {PROTOCOL_VERSION}, got {}.",
                request.protocol_version
            ),
        );
    }

    match request.message_type.as_str() {
        "ping" | "get_status" => {
            let connection_state = register_host_contact(&app, &state).await;
            let queue_summary = state.queue_summary().await;
            HostResponse::ready(request.request_id, "running", connection_state, queue_summary)
        }
        "open_app" | "show_window" => {
            focus_main_window(&app);
            let connection_state = register_host_contact(&app, &state).await;
            let queue_summary = state.queue_summary().await;
            HostResponse::ready(request.request_id, "launched", connection_state, queue_summary)
        }
        "enqueue_download" => {
            let payload = match serde_json::from_value::<EnqueuePayload>(request.payload) {
                Ok(payload) => payload,
                Err(error) => {
                    return HostResponse::error(
                        request.request_id,
                        "invalid_payload",
                        "INVALID_PAYLOAD",
                        format!("Could not parse enqueue payload: {error}"),
                    )
                }
            };

            match state
                .enqueue_download(payload.url, Some(payload.source.into()))
                .await
            {
                Ok((snapshot, job_id)) => {
                    let host_snapshot = state.register_host_contact().await;
                    emit_snapshot(&app, &snapshot);
                    emit_snapshot(&app, &host_snapshot);
                    schedule_downloads(app, state);
                    HostResponse::queued(request.request_id, job_id)
                }
                Err(error) => map_backend_error(request.request_id, error),
            }
        }
        _ => HostResponse::error(
            request.request_id,
            "invalid_payload",
            "INVALID_PAYLOAD",
            "Unsupported host request type.".into(),
        ),
    }
}

async fn register_host_contact(app: &AppHandle, state: &SharedState) -> ConnectionState {
    let snapshot = state.register_host_contact().await;
    let connection_state = snapshot.connection_state;
    emit_snapshot(app, &snapshot);
    connection_state
}

#[cfg(windows)]
async fn refresh_connection_diagnostics(app: &AppHandle, state: &SharedState) {
    let desired_state = if state.has_recent_host_contact(HOST_CONTACT_TTL).await {
        ConnectionState::Connected
    } else {
        match detect_native_host_registration() {
            Ok(HostRegistration::Configured) => ConnectionState::Checking,
            Ok(HostRegistration::Missing) => ConnectionState::HostMissing,
            Ok(HostRegistration::Broken) => ConnectionState::Error,
            Err(error) => {
                eprintln!("connection diagnostics error: {error}");
                ConnectionState::Error
            }
        }
    };

    if state.connection_state().await != desired_state {
        if let Ok(snapshot) = state.set_connection_state(desired_state).await {
            emit_snapshot(app, &snapshot);
        }
    }
}

#[cfg(not(windows))]
async fn refresh_connection_diagnostics(_app: &AppHandle, _state: &SharedState) {}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostRegistration {
    Configured,
    Missing,
    Broken,
}

#[cfg(windows)]
fn detect_native_host_registration() -> Result<HostRegistration, String> {
    Ok(gather_host_registration_diagnostics()?.status.into())
}

#[cfg(not(windows))]
pub fn gather_host_registration_diagnostics() -> Result<HostRegistrationDiagnostics, String> {
    Ok(HostRegistrationDiagnostics {
        status: HostRegistrationStatus::Missing,
        entries: Vec::new(),
    })
}

#[cfg(windows)]
pub fn gather_host_registration_diagnostics() -> Result<HostRegistrationDiagnostics, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut entries = Vec::new();

    for (browser, registry_path) in [
        ("Chrome", NATIVE_HOST_REGISTRY_KEYS[0]),
        ("Edge", NATIVE_HOST_REGISTRY_KEYS[1]),
        ("Firefox", NATIVE_HOST_REGISTRY_KEYS[2]),
    ] {
        let key = match hkcu.open_subkey(registry_path) {
            Ok(key) => key,
            Err(_) => {
                entries.push(HostRegistrationEntry {
                    browser: browser.into(),
                    registry_path: registry_path.into(),
                    manifest_path: None,
                    manifest_exists: false,
                    host_binary_path: None,
                    host_binary_exists: false,
                });
                continue;
            }
        };

        let manifest_path: String = match key.get_value("") {
            Ok(value) => value,
            Err(_) => {
                entries.push(HostRegistrationEntry {
                    browser: browser.into(),
                    registry_path: registry_path.into(),
                    manifest_path: None,
                    manifest_exists: false,
                    host_binary_path: None,
                    host_binary_exists: false,
                });
                continue;
            }
        };

        entries.push(read_host_registration_entry(browser, registry_path, Path::new(&manifest_path))?);
    }

    let status = if entries.iter().any(|entry| entry.host_binary_exists) {
        HostRegistrationStatus::Configured
    } else if entries.iter().any(|entry| entry.manifest_path.is_some()) {
        HostRegistrationStatus::Broken
    } else {
        HostRegistrationStatus::Missing
    };

    Ok(HostRegistrationDiagnostics { status, entries })
}

#[cfg(windows)]
fn read_host_registration_entry(
    browser: &str,
    registry_path: &str,
    manifest_path: &Path,
) -> Result<HostRegistrationEntry, String> {
    if !manifest_path.exists() {
        return Ok(HostRegistrationEntry {
            browser: browser.into(),
            registry_path: registry_path.into(),
            manifest_path: Some(manifest_path.display().to_string()),
            manifest_exists: false,
            host_binary_path: None,
            host_binary_exists: false,
        });
    }

    let content = std::fs::read_to_string(manifest_path)
        .map_err(|error| format!("Could not read native host manifest: {error}"))?;
    let manifest: Value = serde_json::from_str(&content)
        .map_err(|error| format!("Could not parse native host manifest: {error}"))?;
    let host_path = manifest
        .get("path")
        .and_then(|value| value.as_str())
        .map(PathBuf::from);
    let host_binary_exists = host_path
        .as_ref()
        .map(|value| value.exists())
        .unwrap_or(false);

    Ok(HostRegistrationEntry {
        browser: browser.into(),
        registry_path: registry_path.into(),
        manifest_path: Some(manifest_path.display().to_string()),
        manifest_exists: true,
        host_binary_path: host_path.as_ref().map(|value| value.display().to_string()),
        host_binary_exists,
    })
}

#[cfg(windows)]
impl From<HostRegistrationStatus> for HostRegistration {
    fn from(value: HostRegistrationStatus) -> Self {
        match value {
            HostRegistrationStatus::Configured => HostRegistration::Configured,
            HostRegistrationStatus::Missing => HostRegistration::Missing,
            HostRegistrationStatus::Broken => HostRegistration::Broken,
        }
    }
}

fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn map_backend_error(request_id: String, error: BackendError) -> HostResponse {
    let message_type = match error.code {
        "DUPLICATE_JOB" => "duplicate_existing_job",
        "INVALID_URL" | "UNSUPPORTED_SCHEME" => "invalid_url",
        _ => "blocked_by_policy",
    };

    HostResponse::error(request_id, message_type, error.code, error.message)
}
