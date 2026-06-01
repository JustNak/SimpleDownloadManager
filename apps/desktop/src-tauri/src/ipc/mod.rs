use crate::commands::emit_snapshot;
use crate::download::schedule_downloads;
use crate::prompts::{PromptDecision, PromptDuplicateAction, PromptRegistry, PROMPT_CHANGED_EVENT};
use crate::state::{
    BackendError, DuplicatePolicy, EnqueueOptions, EnqueueResult, EnqueueStatus, SharedState,
};
use crate::storage::{
    AppearanceSettings, ConnectionState, DiagnosticLevel, DownloadSource,
    ExtensionIntegrationSettings, HandoffAuth, HostRegistrationDiagnostics, HostRegistrationEntry,
    HostRegistrationStatus, QueueSummary, TransferKind,
};
use crate::windows::{
    focus_job_in_main_window_async, focus_main_window_async, show_download_prompt_window,
    show_progress_window_for_transfer_kind, DOWNLOAD_PROMPT_WINDOW,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use url::Url;

#[cfg(windows)]
use winreg::enums::HKEY_CURRENT_USER;

#[cfg(windows)]
use winreg::RegKey;

pub const PIPE_NAME: &str = r"\\.\pipe\myapp.downloads.v1";
const PROTOCOL_VERSION: u32 = 1;
const MAX_REQUEST_ID_LENGTH: usize = 128;
const MAX_URL_LENGTH: usize = 2048;
const MAX_METADATA_LENGTH: usize = 512;
const MAX_LOCAL_PATH_LENGTH: usize = 32 * 1024;
const SIDE_EFFECT_REQUEST_LIMIT: usize = 30;
const SIDE_EFFECT_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10);
const DOWNLOAD_PROMPT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
static SIDE_EFFECT_REQUEST_TIMES: OnceLock<Mutex<VecDeque<Instant>>> = OnceLock::new();
#[cfg(windows)]
const HOST_CONTACT_TTL: Duration = Duration::from_secs(20);
#[cfg(windows)]
const DIAGNOSTIC_POLL_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(windows)]
const MAX_PIPE_REQUEST_BYTES: usize = 1024 * 1024;
#[cfg(windows)]
const PIPE_READ_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(windows)]
const PIPE_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(windows)]
const PIPE_MAX_INSTANCES: usize = 4;
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
    suggested_filename: Option<String>,
    total_bytes: Option<u64>,
    transfer_kind: Option<TransferKind>,
    handoff_auth: Option<HandoffAuth>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptDownloadPayload {
    url: String,
    source: EnqueueSource,
    suggested_filename: Option<String>,
    total_bytes: Option<u64>,
    transfer_kind: Option<TransferKind>,
    handoff_auth: Option<HandoffAuth>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdoptBrowserDownloadPayload {
    url: String,
    source: EnqueueSource,
    local_path: String,
    suggested_filename: Option<String>,
    total_bytes: Option<u64>,
    mime_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SideEffectClientTrust {
    TrustedNativeHost {
        process_id: u32,
        image_path: PathBuf,
    },
    TrustedDesktop {
        process_id: u32,
        image_path: PathBuf,
    },
    Untrusted {
        process_id: u32,
        image_path: Option<PathBuf>,
    },
    IdentityUnavailable(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenAppPayload {
    reason: String,
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

type ValidationResult = Result<(), Box<HostResponse>>;
type ValidationParseResult<T> = Result<T, Box<HostResponse>>;

impl HostResponse {
    fn ready(
        request_id: String,
        app_state: &str,
        connection_state: ConnectionState,
        queue_summary: QueueSummary,
        extension_settings: ExtensionIntegrationSettings,
        appearance_settings: AppearanceSettings,
    ) -> Self {
        Self {
            ok: true,
            request_id,
            message_type: "ready".into(),
            payload: Some(json!({
                "appState": app_state,
                "connectionState": connection_state,
                "queueSummary": queue_summary,
                "extensionSettings": extension_settings,
                "appearanceSettings": appearance_settings,
            })),
            code: None,
            message: None,
        }
    }

    fn enqueue_result(request_id: String, result: EnqueueResult) -> Self {
        Self {
            ok: true,
            request_id,
            message_type: result.status.as_protocol_value().into(),
            payload: Some(json!({
                "jobId": result.job_id,
                "filename": result.filename,
                "status": result.status.as_protocol_value(),
            })),
            code: None,
            message: None,
        }
    }

    fn existing_job(request_id: String, job_id: String, filename: String) -> Self {
        Self {
            ok: true,
            request_id,
            message_type: "duplicate_existing_job".into(),
            payload: Some(json!({
                "jobId": job_id,
                "filename": filename,
                "status": "duplicate_existing_job",
            })),
            code: None,
            message: None,
        }
    }

    fn prompt_dismissed(request_id: String) -> Self {
        Self {
            ok: true,
            request_id,
            message_type: "prompt_dismissed".into(),
            payload: Some(json!({
                "status": "dismissed",
            })),
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
pub fn start_named_pipe_listener(app: AppHandle, state: SharedState, prompts: PromptRegistry) {
    let listener_app = app.clone();
    let listener_state = state.clone();
    let listener_prompts = prompts.clone();
    tauri::async_runtime::spawn(async move {
        refresh_connection_diagnostics(&listener_app, &listener_state).await;

        let mut first_pipe_instance = true;
        loop {
            if let Err(error) = accept_single_connection(
                listener_app.clone(),
                listener_state.clone(),
                listener_prompts.clone(),
                first_pipe_instance,
            )
            .await
            {
                eprintln!("named pipe listener error: {error}");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            } else {
                first_pipe_instance = false;
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
pub fn start_named_pipe_listener(_app: AppHandle, _state: SharedState, _prompts: PromptRegistry) {}

#[cfg(windows)]
async fn accept_single_connection(
    app: AppHandle,
    state: SharedState,
    prompts: PromptRegistry,
    first_pipe_instance: bool,
) -> Result<(), String> {
    use tokio::io::{AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ServerOptions;

    let mut server_options = ServerOptions::new();
    server_options
        .reject_remote_clients(true)
        .max_instances(PIPE_MAX_INSTANCES);
    if first_pipe_instance {
        server_options.first_pipe_instance(true);
    }

    let server = server_options
        .create(PIPE_NAME)
        .map_err(|error| format!("Could not create named pipe server: {error}"))?;

    server
        .connect()
        .await
        .map_err(|error| format!("Could not accept named pipe connection: {error}"))?;
    let client_trust = side_effect_client_trust_for_pipe(&server);

    tauri::async_runtime::spawn(async move {
        let result: Result<(), String> = async {
            let (reader, mut writer) = tokio::io::split(server);
            let mut reader = BufReader::new(reader);
            let request_line =
                tokio::time::timeout(PIPE_READ_TIMEOUT, read_limited_request_line(&mut reader))
                    .await
                    .map_err(|_| "Timed out reading named pipe payload.".to_string())??;

            if request_line.trim().is_empty() {
                return Ok(());
            }

            let request = serde_json::from_str::<HostRequest>(&request_line)
                .map_err(|error| format!("Could not parse host request: {error}"))?;

            let response =
                handle_request(app, state.clone(), prompts, request, &client_trust).await;
            if !response.ok {
                let _ = state
                    .record_diagnostic_event(
                        DiagnosticLevel::Warning,
                        "native_host",
                        response
                            .message
                            .clone()
                            .unwrap_or_else(|| "Native host request was rejected.".into()),
                        None,
                    )
                    .await;
            }
            let response_json = serde_json::to_string(&response)
                .map_err(|error| format!("Could not serialize host response: {error}"))?;

            tokio::time::timeout(PIPE_WRITE_TIMEOUT, async {
                writer
                    .write_all(response_json.as_bytes())
                    .await
                    .map_err(|error| format!("Could not write named pipe response: {error}"))?;

                writer.write_all(b"\n").await.map_err(|error| {
                    format!("Could not write named pipe response terminator: {error}")
                })?;

                writer
                    .flush()
                    .await
                    .map_err(|error| format!("Could not flush named pipe response: {error}"))
            })
            .await
            .map_err(|_| "Timed out writing named pipe response.".to_string())??;

            Ok(())
        }
        .await;

        if let Err(error) = result {
            eprintln!("named pipe request error: {error}");
        }
    });

    Ok(())
}

#[cfg(windows)]
async fn read_limited_request_line<R>(reader: &mut R) -> Result<String, String>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;

    let mut request = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|error| format!("Could not read named pipe payload: {error}"))?;

        if available.is_empty() {
            break;
        }

        let newline_index = available.iter().position(|byte| *byte == b'\n');
        let read_len = newline_index
            .map(|index| index.saturating_add(1))
            .unwrap_or(available.len());

        if request.len().saturating_add(read_len) > MAX_PIPE_REQUEST_BYTES {
            return Err(format!(
                "Named pipe payload exceeds {MAX_PIPE_REQUEST_BYTES} bytes."
            ));
        }

        request.extend_from_slice(&available[..read_len]);
        reader.consume(read_len);

        if newline_index.is_some() {
            break;
        }
    }

    String::from_utf8(request)
        .map_err(|error| format!("Named pipe payload was not valid UTF-8: {error}"))
}

async fn handle_request(
    app: AppHandle,
    state: SharedState,
    prompts: PromptRegistry,
    request: HostRequest,
    client_trust: &SideEffectClientTrust,
) -> HostResponse {
    if let Err(response) = validate_host_request(&request) {
        return *response;
    }

    if is_side_effect_rate_limited_at(&request.message_type, Instant::now()) {
        return HostResponse::error(
            request.request_id,
            "rejected",
            "RATE_LIMITED",
            "Too many extension bridge requests. Try again shortly.".into(),
        );
    }

    if let Err(message) = authorize_side_effect_client(&request.message_type, client_trust) {
        return HostResponse::error(request.request_id, "rejected", "PERMISSION_DENIED", message);
    }

    if should_register_host_contact_before_response(&request.message_type) {
        register_host_contact(&app, &state).await;
    }

    match request.message_type.as_str() {
        "ping" | "get_status" => {
            let connection_state = register_host_contact(&app, &state).await;
            let queue_summary = state.queue_summary().await;
            let extension_settings = state.extension_integration_settings().await;
            let appearance_settings = state.appearance_settings().await;
            HostResponse::ready(
                request.request_id,
                "running",
                connection_state,
                queue_summary,
                extension_settings,
                appearance_settings,
            )
        }
        "open_app" | "show_window" => {
            focus_main_window_async(&app).await;
            let connection_state = register_host_contact(&app, &state).await;
            let queue_summary = state.queue_summary().await;
            let extension_settings = state.extension_integration_settings().await;
            let appearance_settings = state.appearance_settings().await;
            HostResponse::ready(
                request.request_id,
                "launched",
                connection_state,
                queue_summary,
                extension_settings,
                appearance_settings,
            )
        }
        "save_extension_settings" => {
            let extension_settings =
                match serde_json::from_value::<ExtensionIntegrationSettings>(request.payload) {
                    Ok(settings) => settings,
                    Err(error) => {
                        return HostResponse::error(
                            request.request_id,
                            "invalid_payload",
                            "INVALID_PAYLOAD",
                            format!("Could not parse extension settings: {error}"),
                        )
                    }
                };

            match state
                .save_extension_integration_settings(extension_settings)
                .await
            {
                Ok(snapshot) => {
                    emit_snapshot(&app, &snapshot);
                    let connection_state = register_host_contact(&app, &state).await;
                    let queue_summary = state.queue_summary().await;
                    let extension_settings = state.extension_integration_settings().await;
                    let appearance_settings = state.appearance_settings().await;
                    HostResponse::ready(
                        request.request_id,
                        "running",
                        connection_state,
                        queue_summary,
                        extension_settings,
                        appearance_settings,
                    )
                }
                Err(message) => HostResponse::error(
                    request.request_id,
                    "blocked_by_policy",
                    "INTERNAL_ERROR",
                    message,
                ),
            }
        }
        "prompt_download" => {
            let payload = match serde_json::from_value::<PromptDownloadPayload>(request.payload) {
                Ok(payload) => payload,
                Err(error) => {
                    return HostResponse::error(
                        request.request_id,
                        "invalid_payload",
                        "INVALID_PAYLOAD",
                        format!("Could not parse prompt payload: {error}"),
                    )
                }
            };
            let source: DownloadSource = payload.source.into();
            let transfer_kind = handoff_transfer_kind(
                payload.transfer_kind,
                &payload.url,
                payload.suggested_filename.as_deref(),
            );

            if transfer_kind == TransferKind::Torrent {
                return enqueue_handoff_download(
                    &app,
                    state,
                    HandoffDownloadRequest {
                        request_id: request.request_id,
                        url: payload.url,
                        source,
                        filename_hint: payload.suggested_filename,
                        transfer_kind,
                        handoff_auth: payload.handoff_auth,
                    },
                )
                .await;
            }

            let prompt = match state
                .prepare_download_prompt(
                    request.request_id.clone(),
                    &payload.url,
                    Some(source.clone()),
                    payload.suggested_filename,
                    payload.total_bytes,
                )
                .await
            {
                Ok(prompt) => prompt,
                Err(error) => return map_backend_error(request.request_id, error),
            };

            run_prompt_download(
                &app,
                state,
                prompts,
                request.request_id,
                prompt,
                source,
                payload.handoff_auth,
            )
            .await
        }
        "adopt_browser_download" => {
            let payload =
                match serde_json::from_value::<AdoptBrowserDownloadPayload>(request.payload) {
                    Ok(payload) => payload,
                    Err(error) => {
                        return HostResponse::error(
                            request.request_id,
                            "invalid_payload",
                            "INVALID_PAYLOAD",
                            format!("Could not parse browser adoption payload: {error}"),
                        )
                    }
                };
            let source: DownloadSource = payload.source.into();
            match state
                .adopt_browser_download(
                    payload.url,
                    source,
                    payload.local_path,
                    payload.suggested_filename,
                    payload.total_bytes,
                    payload.mime_type,
                )
                .await
            {
                Ok(result) => {
                    let host_snapshot = state.register_host_contact().await;
                    emit_snapshot(&app, &result.snapshot);
                    emit_snapshot(&app, &host_snapshot);
                    HostResponse::enqueue_result(request.request_id, result)
                }
                Err(error) => map_backend_error(request.request_id, error),
            }
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

            let source: DownloadSource = payload.source.into();
            let transfer_kind = handoff_transfer_kind(
                payload.transfer_kind,
                &payload.url,
                payload.suggested_filename.as_deref(),
            );

            if transfer_kind == TransferKind::Torrent {
                return enqueue_handoff_download(
                    &app,
                    state,
                    HandoffDownloadRequest {
                        request_id: request.request_id,
                        url: payload.url,
                        source,
                        filename_hint: payload.suggested_filename,
                        transfer_kind,
                        handoff_auth: payload.handoff_auth,
                    },
                )
                .await;
            }

            if source.entry_point == "browser_download" && transfer_kind == TransferKind::Http {
                let prompt = match state
                    .prepare_download_prompt(
                        request.request_id.clone(),
                        &payload.url,
                        Some(source.clone()),
                        payload.suggested_filename.clone(),
                        payload.total_bytes,
                    )
                    .await
                {
                    Ok(prompt) => prompt,
                    Err(error) => return map_backend_error(request.request_id, error),
                };

                if prompt_has_duplicate(&prompt) {
                    return run_prompt_download(
                        &app,
                        state,
                        prompts,
                        request.request_id,
                        prompt,
                        source,
                        payload.handoff_auth,
                    )
                    .await;
                }
            }

            match state
                .enqueue_download_with_options(
                    payload.url,
                    EnqueueOptions {
                        source: Some(source),
                        filename_hint: payload.suggested_filename,
                        handoff_auth: payload.handoff_auth,
                        transfer_kind: Some(transfer_kind),
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(result) => {
                    let host_snapshot = state.register_host_contact().await;
                    emit_snapshot(&app, &result.snapshot);
                    emit_snapshot(&app, &host_snapshot);
                    if result.status == EnqueueStatus::Queued {
                        schedule_downloads(app, state);
                    }
                    HostResponse::enqueue_result(request.request_id, result)
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

fn validate_host_request(request: &HostRequest) -> ValidationResult {
    if request.protocol_version != PROTOCOL_VERSION {
        return Err(Box::new(HostResponse::error(
            safe_response_request_id(&request.request_id),
            "invalid_payload",
            "HOST_PROTOCOL_MISMATCH",
            format!(
                "Expected protocol version {PROTOCOL_VERSION}, got {}.",
                request.protocol_version
            ),
        )));
    }

    if !is_valid_request_id(&request.request_id) {
        return Err(validation_error(
            &request.request_id,
            "INVALID_PAYLOAD",
            "Request id is not supported.",
        ));
    }

    if !is_supported_request_type(&request.message_type) {
        return Err(validation_error(
            &request.request_id,
            "INVALID_PAYLOAD",
            "Unsupported host request type.",
        ));
    }

    match request.message_type.as_str() {
        "ping" | "get_status" => validate_empty_payload(request),
        "open_app" | "show_window" => validate_open_app_payload(request),
        "save_extension_settings" => validate_extension_settings_payload(request),
        "enqueue_download" => validate_enqueue_request_payload(request),
        "prompt_download" => validate_prompt_download_request_payload(request),
        "adopt_browser_download" => validate_adopt_browser_download_payload(request),
        _ => unreachable!("supported request type checked above"),
    }
}

fn validate_empty_payload(request: &HostRequest) -> ValidationResult {
    if request.payload.is_object() {
        Ok(())
    } else {
        Err(validation_error(
            &request.request_id,
            "INVALID_PAYLOAD",
            "Payload must be an object.",
        ))
    }
}

fn validate_open_app_payload(request: &HostRequest) -> ValidationResult {
    let payload = parse_payload::<OpenAppPayload>(request, "open app")?;
    if matches!(payload.reason.as_str(), "user_request" | "reconnect") {
        Ok(())
    } else {
        Err(validation_error(
            &request.request_id,
            "INVALID_PAYLOAD",
            "Open app reason is not supported.",
        ))
    }
}

fn validate_extension_settings_payload(request: &HostRequest) -> ValidationResult {
    let _settings = parse_payload::<ExtensionIntegrationSettings>(request, "extension settings")?;
    Ok(())
}

fn validate_enqueue_request_payload(request: &HostRequest) -> ValidationResult {
    let payload = parse_payload::<EnqueuePayload>(request, "enqueue")?;
    validate_handoff_url(&request.request_id, &payload.url)?;
    validate_request_source(&request.request_id, &payload.source)?;
    validate_metadata_field(
        &request.request_id,
        "suggestedFilename",
        payload.suggested_filename.as_deref(),
    )
}

fn validate_prompt_download_request_payload(request: &HostRequest) -> ValidationResult {
    let payload = parse_payload::<PromptDownloadPayload>(request, "prompt")?;
    validate_handoff_url(&request.request_id, &payload.url)?;
    validate_request_source(&request.request_id, &payload.source)?;
    validate_metadata_field(
        &request.request_id,
        "suggestedFilename",
        payload.suggested_filename.as_deref(),
    )
}

fn validate_adopt_browser_download_payload(request: &HostRequest) -> ValidationResult {
    let payload = parse_payload::<AdoptBrowserDownloadPayload>(request, "browser adoption")?;
    validate_handoff_url(&request.request_id, &payload.url)?;
    validate_request_source(&request.request_id, &payload.source)?;
    if payload.source.entry_point != "browser_download" {
        return Err(validation_error(
            &request.request_id,
            "INVALID_PAYLOAD",
            "Browser download adoption is only supported for browser downloads.",
        ));
    }
    validate_local_path_field(&request.request_id, &payload.local_path)?;
    validate_metadata_field(
        &request.request_id,
        "suggestedFilename",
        payload.suggested_filename.as_deref(),
    )?;
    validate_metadata_field(
        &request.request_id,
        "mimeType",
        payload.mime_type.as_deref(),
    )
}

fn parse_payload<T>(request: &HostRequest, label: &str) -> ValidationParseResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value::<T>(request.payload.clone()).map_err(|error| {
        validation_error(
            &request.request_id,
            "INVALID_PAYLOAD",
            format!("Could not parse {label} payload: {error}"),
        )
    })
}

fn validate_handoff_url(request_id: &str, raw_url: &str) -> ValidationResult {
    let trimmed_url = raw_url.trim();
    if trimmed_url.is_empty() {
        return Err(validation_error(
            request_id,
            "INVALID_URL",
            "URL is required.",
        ));
    }

    if trimmed_url.len() > MAX_URL_LENGTH {
        return Err(validation_error(
            request_id,
            "URL_TOO_LONG",
            format!("URL exceeds {MAX_URL_LENGTH} characters."),
        ));
    }

    let parsed = Url::parse(trimmed_url)
        .map_err(|_| validation_error(request_id, "INVALID_URL", "URL is not valid."))?;

    match parsed.scheme() {
        "http" | "https" | "magnet" => Ok(()),
        _ => Err(validation_error(
            request_id,
            "UNSUPPORTED_SCHEME",
            "Only http, https, and magnet URLs are supported.",
        )),
    }
}

fn validate_request_source(request_id: &str, source: &EnqueueSource) -> ValidationResult {
    if !matches!(
        source.entry_point.as_str(),
        "context_menu" | "popup" | "browser_download"
    ) {
        return Err(validation_error(
            request_id,
            "INVALID_PAYLOAD",
            "Source entry point is not supported.",
        ));
    }

    if !matches!(source.browser.as_str(), "chrome" | "edge" | "firefox") {
        return Err(validation_error(
            request_id,
            "INVALID_PAYLOAD",
            "Browser is not supported.",
        ));
    }

    validate_metadata_field(request_id, "entryPoint", Some(source.entry_point.as_str()))?;
    validate_metadata_field(request_id, "browser", Some(source.browser.as_str()))?;
    validate_metadata_field(
        request_id,
        "extensionVersion",
        Some(source.extension_version.as_str()),
    )?;
    validate_metadata_field(request_id, "pageUrl", source.page_url.as_deref())?;
    validate_metadata_field(request_id, "pageTitle", source.page_title.as_deref())?;
    validate_metadata_field(request_id, "referrer", source.referrer.as_deref())
}

fn validate_metadata_field(
    request_id: &str,
    field_name: &str,
    value: Option<&str>,
) -> ValidationResult {
    if value.is_some_and(|value| value.len() > MAX_METADATA_LENGTH) {
        return Err(validation_error(
            request_id,
            "METADATA_TOO_LARGE",
            format!("{field_name} exceeds {MAX_METADATA_LENGTH} characters."),
        ));
    }

    Ok(())
}

fn validate_local_path_field(request_id: &str, value: &str) -> ValidationResult {
    if value.trim().is_empty() {
        return Err(validation_error(
            request_id,
            "INVALID_PAYLOAD",
            "Browser download path is required.",
        ));
    }

    if value.len() > MAX_LOCAL_PATH_LENGTH {
        return Err(validation_error(
            request_id,
            "METADATA_TOO_LARGE",
            format!("localPath exceeds {MAX_LOCAL_PATH_LENGTH} characters."),
        ));
    }

    if value.chars().any(char::is_control) {
        return Err(validation_error(
            request_id,
            "INVALID_PAYLOAD",
            "Browser download path contains unsupported characters.",
        ));
    }

    Ok(())
}

fn is_valid_request_id(request_id: &str) -> bool {
    !request_id.is_empty()
        && request_id.len() <= MAX_REQUEST_ID_LENGTH
        && request_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':')
        })
}

fn is_supported_request_type(message_type: &str) -> bool {
    matches!(
        message_type,
        "ping"
            | "get_status"
            | "open_app"
            | "show_window"
            | "save_extension_settings"
            | "enqueue_download"
            | "prompt_download"
            | "adopt_browser_download"
    )
}

fn validation_error(
    request_id: &str,
    code: &'static str,
    message: impl Into<String>,
) -> Box<HostResponse> {
    Box::new(HostResponse::error(
        safe_response_request_id(request_id),
        "invalid_payload",
        code,
        message.into(),
    ))
}

fn safe_response_request_id(request_id: &str) -> String {
    if request_id.len() <= MAX_REQUEST_ID_LENGTH {
        request_id.to_string()
    } else {
        "invalid_request".into()
    }
}

fn is_side_effect_rate_limited_at(message_type: &str, now: Instant) -> bool {
    if !is_side_effect_request_type(message_type) {
        return false;
    }

    let mut request_times = side_effect_request_times()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    while request_times.front().is_some_and(|timestamp| {
        now.saturating_duration_since(*timestamp) > SIDE_EFFECT_RATE_LIMIT_WINDOW
    }) {
        request_times.pop_front();
    }

    if request_times.len() >= SIDE_EFFECT_REQUEST_LIMIT {
        return true;
    }

    request_times.push_back(now);
    false
}

fn is_side_effect_request_type(message_type: &str) -> bool {
    matches!(
        message_type,
        "open_app"
            | "show_window"
            | "save_extension_settings"
            | "enqueue_download"
            | "prompt_download"
            | "adopt_browser_download"
    )
}

fn authorize_side_effect_client(
    message_type: &str,
    trust: &SideEffectClientTrust,
) -> Result<(), String> {
    if !is_side_effect_request_type(message_type) {
        return Ok(());
    }

    match trust {
        SideEffectClientTrust::TrustedNativeHost { .. }
        | SideEffectClientTrust::TrustedDesktop { .. } => Ok(()),
        SideEffectClientTrust::Untrusted {
            process_id,
            image_path,
        } => Err(format!(
            "Rejected side-effecting native host request from untrusted local client {process_id}{}.",
            image_path
                .as_ref()
                .map(|path| format!(" ({})", path.display()))
                .unwrap_or_default()
        )),
        SideEffectClientTrust::IdentityUnavailable(message) => Err(format!(
            "Rejected side-effecting native host request because client identity could not be verified: {message}."
        )),
    }
}

#[cfg(windows)]
fn side_effect_client_trust_for_pipe(
    server: &tokio::net::windows::named_pipe::NamedPipeServer,
) -> SideEffectClientTrust {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;

    let mut process_id = 0_u32;
    let ok = unsafe {
        GetNamedPipeClientProcessId(server.as_raw_handle() as _, &mut process_id as *mut u32)
    };
    if ok == 0 || process_id == 0 {
        return SideEffectClientTrust::IdentityUnavailable(
            "GetNamedPipeClientProcessId failed".into(),
        );
    }

    match query_process_image_path(process_id) {
        Ok(image_path) => classify_side_effect_client_process(process_id, Some(image_path)),
        Err(error) => SideEffectClientTrust::IdentityUnavailable(error),
    }
}

#[cfg(windows)]
fn query_process_image_path(process_id: u32) -> Result<PathBuf, String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return Err(format!("OpenProcess failed with {}", unsafe {
            GetLastError()
        }));
    }

    let mut buffer = vec![0_u16; 32_768];
    let mut len = buffer.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len)
    };
    unsafe {
        CloseHandle(handle);
    }
    if ok == 0 {
        return Err(format!(
            "QueryFullProcessImageNameW failed with {}",
            unsafe { GetLastError() }
        ));
    }

    Ok(PathBuf::from(OsString::from_wide(&buffer[..len as usize])))
}

#[cfg(windows)]
fn classify_side_effect_client_process(
    process_id: u32,
    image_path: Option<PathBuf>,
) -> SideEffectClientTrust {
    let Some(image_path) = image_path else {
        return SideEffectClientTrust::Untrusted {
            process_id,
            image_path: None,
        };
    };

    if is_current_desktop_process(&image_path) {
        return SideEffectClientTrust::TrustedDesktop {
            process_id,
            image_path,
        };
    }

    if is_bundled_native_host_process(&image_path) {
        return SideEffectClientTrust::TrustedNativeHost {
            process_id,
            image_path,
        };
    }

    SideEffectClientTrust::Untrusted {
        process_id,
        image_path: Some(image_path),
    }
}

#[cfg(windows)]
fn is_current_desktop_process(image_path: &Path) -> bool {
    std::env::current_exe()
        .ok()
        .is_some_and(|current| same_path(&current, image_path))
}

#[cfg(windows)]
fn is_bundled_native_host_process(image_path: &Path) -> bool {
    let Some(file_name) = image_path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    if !matches!(
        file_name.to_ascii_lowercase().as_str(),
        "simple-download-manager-native-host.exe"
            | "simple-download-manager-native-host-x86_64-pc-windows-msvc.exe"
    ) {
        return false;
    }

    let Ok(current_exe) = std::env::current_exe() else {
        return true;
    };
    let Some(install_root) = current_exe.parent() else {
        return true;
    };

    image_path.starts_with(install_root)
        || install_root
            .parent()
            .is_some_and(|parent| image_path.starts_with(parent))
}

#[cfg(windows)]
fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn side_effect_request_times() -> &'static Mutex<VecDeque<Instant>> {
    SIDE_EFFECT_REQUEST_TIMES.get_or_init(|| Mutex::new(VecDeque::new()))
}

#[cfg(test)]
fn reset_side_effect_rate_limit_for_tests() {
    side_effect_request_times()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
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

        entries.push(read_host_registration_entry(
            browser,
            registry_path,
            Path::new(&manifest_path),
        )?);
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

    let content = match std::fs::read_to_string(manifest_path) {
        Ok(content) => content,
        Err(error) => {
            eprintln!("could not read native host manifest for diagnostics: {error}");
            return Ok(broken_host_registration_entry(
                browser,
                registry_path,
                manifest_path,
            ));
        }
    };
    let manifest: Value = match serde_json::from_str(&content) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("could not parse native host manifest for diagnostics: {error}");
            return Ok(broken_host_registration_entry(
                browser,
                registry_path,
                manifest_path,
            ));
        }
    };
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
fn broken_host_registration_entry(
    browser: &str,
    registry_path: &str,
    manifest_path: &Path,
) -> HostRegistrationEntry {
    HostRegistrationEntry {
        browser: browser.into(),
        registry_path: registry_path.into(),
        manifest_path: Some(manifest_path.display().to_string()),
        manifest_exists: true,
        host_binary_path: None,
        host_binary_exists: false,
    }
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

fn prompt_enqueue_details(
    default_filename: String,
    duplicate_action: PromptDuplicateAction,
    renamed_filename: Option<String>,
) -> Result<(String, DuplicatePolicy), BackendError> {
    match duplicate_action {
        PromptDuplicateAction::ReturnExisting => {
            Ok((default_filename, DuplicatePolicy::ReturnExisting))
        }
        PromptDuplicateAction::DownloadAnyway => Ok((default_filename, DuplicatePolicy::Allow)),
        PromptDuplicateAction::Overwrite => {
            Ok((default_filename, DuplicatePolicy::ReplaceExisting))
        }
        PromptDuplicateAction::Rename => {
            let filename = renamed_filename.unwrap_or_default();
            if filename.trim().is_empty() {
                return Err(BackendError {
                    code: "INVALID_PAYLOAD",
                    message: "Filename cannot be empty.".into(),
                });
            }
            Ok((filename, DuplicatePolicy::Allow))
        }
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

fn handoff_transfer_kind(
    explicit_transfer_kind: Option<TransferKind>,
    url: &str,
    suggested_filename: Option<&str>,
) -> TransferKind {
    explicit_transfer_kind.unwrap_or_else(|| {
        if handoff_url_is_torrent(url) || suggested_filename_is_torrent(suggested_filename) {
            TransferKind::Torrent
        } else {
            TransferKind::Http
        }
    })
}

fn handoff_url_is_torrent(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url.trim()) else {
        return false;
    };

    if parsed.scheme() == "magnet" {
        return true;
    }

    matches!(parsed.scheme(), "http" | "https")
        && parsed
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|segment| segment.to_ascii_lowercase().ends_with(".torrent"))
}

fn suggested_filename_is_torrent(suggested_filename: Option<&str>) -> bool {
    let Some(suggested_filename) = suggested_filename else {
        return false;
    };
    let normalized = suggested_filename.trim().replace('\\', "/");
    normalized
        .rsplit('/')
        .next()
        .is_some_and(|filename| filename.to_ascii_lowercase().ends_with(".torrent"))
}

struct HandoffDownloadRequest {
    request_id: String,
    url: String,
    source: DownloadSource,
    filename_hint: Option<String>,
    transfer_kind: TransferKind,
    handoff_auth: Option<HandoffAuth>,
}

async fn enqueue_handoff_download(
    app: &AppHandle,
    state: SharedState,
    request: HandoffDownloadRequest,
) -> HostResponse {
    let HandoffDownloadRequest {
        request_id,
        url,
        source,
        filename_hint,
        transfer_kind,
        handoff_auth,
    } = request;

    match state
        .enqueue_download_with_options(
            url,
            EnqueueOptions {
                source: Some(source),
                filename_hint,
                handoff_auth,
                transfer_kind: Some(transfer_kind),
                ..Default::default()
            },
        )
        .await
    {
        Ok(result) => {
            let show_progress =
                transfer_kind == TransferKind::Torrent && state.show_progress_after_handoff().await;
            let host_snapshot = state.register_host_contact().await;
            emit_snapshot(app, &result.snapshot);
            emit_snapshot(app, &host_snapshot);
            if result.status == EnqueueStatus::Queued {
                if show_progress {
                    let _ =
                        show_progress_window_for_transfer_kind(app, &result.job_id, transfer_kind)
                            .await;
                }
                schedule_downloads(app.clone(), state);
            }
            HostResponse::enqueue_result(request_id, result)
        }
        Err(error) => map_backend_error(request_id, error),
    }
}

fn should_register_host_contact_before_response(message_type: &str) -> bool {
    matches!(message_type, "prompt_download")
}

async fn run_prompt_download(
    app: &AppHandle,
    state: SharedState,
    prompts: PromptRegistry,
    request_id: String,
    prompt: crate::storage::DownloadPrompt,
    source: DownloadSource,
    handoff_auth: Option<HandoffAuth>,
) -> HostResponse {
    let receiver = prompts.enqueue(prompt.clone()).await;
    if let Err(error) = show_download_prompt_window(app).await {
        let _ = prompts.resolve(&prompt.id, PromptDecision::Cancel).await;
        return HostResponse::error(
            request_id,
            "blocked_by_policy",
            "INTERNAL_ERROR",
            format!("Could not open download prompt: {error}"),
        );
    }
    if let Some(active_prompt) = prompts.active_prompt().await {
        let _ = app.emit_to(DOWNLOAD_PROMPT_WINDOW, PROMPT_CHANGED_EVENT, active_prompt);
    }

    let (decision, next_prompt_after_timeout) =
        prompt_decision_or_timeout(&prompts, &prompt.id, receiver, DOWNLOAD_PROMPT_TIMEOUT).await;
    if let Some(next_prompt) = next_prompt_after_timeout {
        let _ = app.emit_to(DOWNLOAD_PROMPT_WINDOW, PROMPT_CHANGED_EVENT, next_prompt);
    }

    match decision {
        PromptDecision::Cancel => HostResponse::prompt_dismissed(request_id),
        PromptDecision::ShowExisting => {
            if let Some(job) = prompt.duplicate_job {
                focus_job_in_main_window_async(app, &job.id).await;
                HostResponse::existing_job(request_id, job.id, job.filename)
            } else {
                HostResponse::prompt_dismissed(request_id)
            }
        }
        PromptDecision::Download {
            directory_override,
            duplicate_action,
            renamed_filename,
        } => {
            let (filename_hint, duplicate_policy) =
                match prompt_enqueue_details(prompt.filename, duplicate_action, renamed_filename) {
                    Ok(details) => details,
                    Err(error) => return map_backend_error(request_id, error),
                };

            let result = state
                .enqueue_download_with_options(
                    prompt.url,
                    EnqueueOptions {
                        source: Some(source),
                        directory_override,
                        filename_hint: Some(filename_hint),
                        handoff_auth,
                        duplicate_policy,
                        ..Default::default()
                    },
                )
                .await;

            match result {
                Ok(result) => {
                    let show_progress = state.show_progress_after_handoff().await;
                    emit_snapshot(app, &result.snapshot);
                    if result.status == EnqueueStatus::Queued {
                        if show_progress {
                            let transfer_kind = result
                                .snapshot
                                .jobs
                                .iter()
                                .find(|job| job.id == result.job_id)
                                .map(|job| job.transfer_kind)
                                .unwrap_or_default();
                            let _ = show_progress_window_for_transfer_kind(
                                app,
                                &result.job_id,
                                transfer_kind,
                            )
                            .await;
                        }
                        schedule_downloads(app.clone(), state);
                    }
                    HostResponse::enqueue_result(request_id, result)
                }
                Err(error) => map_backend_error(request_id, error),
            }
        }
    }
}

async fn prompt_decision_or_timeout(
    prompts: &PromptRegistry,
    prompt_id: &str,
    mut receiver: tokio::sync::oneshot::Receiver<PromptDecision>,
    timeout: Duration,
) -> (PromptDecision, Option<crate::storage::DownloadPrompt>) {
    tokio::select! {
        decision = &mut receiver => (decision.unwrap_or(PromptDecision::Cancel), None),
        _ = tokio::time::sleep(timeout) => {
            let next_prompt = prompts
                .resolve(prompt_id, PromptDecision::Cancel)
                .await
                .ok()
                .flatten();
            (PromptDecision::Cancel, next_prompt)
        }
    }
}

fn prompt_has_duplicate(prompt: &crate::storage::DownloadPrompt) -> bool {
    prompt.duplicate_job.is_some()
        || prompt.duplicate_path.is_some()
        || prompt.duplicate_reason.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn host_request(message_type: &str, payload: Value) -> HostRequest {
        HostRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-1".into(),
            message_type: message_type.into(),
            payload,
        }
    }

    fn valid_source() -> Value {
        json!({
            "entryPoint": "browser_download",
            "browser": "firefox",
            "extensionVersion": "0.3.48-beta"
        })
    }

    fn valid_enqueue_payload() -> Value {
        json!({
            "url": "https://example.com/file.zip",
            "source": valid_source()
        })
    }

    fn test_runtime_path(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::current_dir()
            .unwrap()
            .join("test-runtime")
            .join(format!("{name}-{}-{nonce}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn prompt_download_requests_register_host_contact_before_user_decision() {
        assert!(super::should_register_host_contact_before_response(
            "prompt_download"
        ));
        assert!(!super::should_register_host_contact_before_response(
            "enqueue_download"
        ));
    }

    #[tokio::test]
    async fn prompt_decision_timeout_resolves_as_cancel_and_activates_next_prompt() {
        let prompts = PromptRegistry::default();
        let first = crate::storage::DownloadPrompt {
            id: "prompt_timeout".into(),
            url: "https://example.com/timeout.zip".into(),
            filename: "timeout.zip".into(),
            source: None,
            total_bytes: None,
            default_directory: "C:/Downloads".into(),
            target_path: "C:/Downloads/timeout.zip".into(),
            duplicate_job: None,
            duplicate_path: None,
            duplicate_filename: None,
            duplicate_reason: None,
        };
        let second = crate::storage::DownloadPrompt {
            id: "prompt_next".into(),
            url: "https://example.com/next.zip".into(),
            filename: "next.zip".into(),
            source: None,
            total_bytes: None,
            default_directory: "C:/Downloads".into(),
            target_path: "C:/Downloads/next.zip".into(),
            duplicate_job: None,
            duplicate_path: None,
            duplicate_filename: None,
            duplicate_reason: None,
        };
        let first_receiver = prompts.enqueue(first).await;
        let _second_receiver = prompts.enqueue(second).await;

        let (decision, next_prompt) = super::prompt_decision_or_timeout(
            &prompts,
            "prompt_timeout",
            first_receiver,
            Duration::from_millis(1),
        )
        .await;

        assert!(matches!(decision, PromptDecision::Cancel));
        assert_eq!(
            next_prompt.map(|prompt| prompt.id),
            Some("prompt_next".into())
        );
        assert_eq!(
            prompts.active_prompt().await.map(|prompt| prompt.id),
            Some("prompt_next".into())
        );
    }

    #[test]
    fn handoff_transfer_kind_uses_explicit_url_or_filename_torrent_signals() {
        assert_eq!(
            super::handoff_transfer_kind(
                None,
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
                None,
            ),
            TransferKind::Torrent
        );
        assert_eq!(
            super::handoff_transfer_kind(
                None,
                "https://example.com/download?id=opaque",
                Some(r"C:\Users\Me\linux.iso.torrent"),
            ),
            TransferKind::Torrent
        );
        assert_eq!(
            super::handoff_transfer_kind(
                Some(TransferKind::Http),
                "https://example.com/file.torrent",
                None,
            ),
            TransferKind::Http
        );
    }

    #[test]
    fn ready_response_includes_appearance_settings() {
        let response = HostResponse::ready(
            "request-1".into(),
            "running",
            ConnectionState::Connected,
            QueueSummary {
                total: 0,
                active: 0,
                attention: 0,
                queued: 0,
                downloading: 0,
                completed: 0,
                failed: 0,
            },
            ExtensionIntegrationSettings::default(),
            crate::storage::AppearanceSettings {
                theme: crate::storage::Theme::OledDark,
                accent_color: "#06b6d4".into(),
            },
        );

        let appearance = response
            .payload
            .as_ref()
            .and_then(|payload| payload.get("appearanceSettings"))
            .expect("ready response should include appearance settings");

        assert_eq!(
            appearance.get("theme").and_then(|value| value.as_str()),
            Some("oled_dark")
        );
        assert_eq!(
            appearance
                .get("accentColor")
                .and_then(|value| value.as_str()),
            Some("#06b6d4")
        );
    }

    #[test]
    fn host_request_validation_rejects_oversized_request_ids() {
        let mut request = host_request("ping", json!({}));
        request.request_id = "x".repeat(129);

        let error =
            validate_host_request(&request).expect_err("oversized request id should reject");

        assert_eq!(error.code, Some("INVALID_PAYLOAD"));
    }

    #[test]
    fn host_request_validation_rejects_unknown_source_values() {
        let mut payload = valid_enqueue_payload();
        payload["source"]["browser"] = json!("safari");
        let request = host_request("enqueue_download", payload);

        let error = validate_host_request(&request).expect_err("unknown source should reject");

        assert_eq!(error.code, Some("INVALID_PAYLOAD"));
    }

    #[test]
    fn host_request_validation_rejects_oversized_source_metadata() {
        let mut payload = valid_enqueue_payload();
        payload["source"]["pageTitle"] = json!("x".repeat(513));
        let request = host_request("enqueue_download", payload);

        let error = validate_host_request(&request).expect_err("large metadata should reject");

        assert_eq!(error.code, Some("METADATA_TOO_LARGE"));
    }

    #[test]
    fn host_request_validation_rejects_unknown_open_reasons() {
        let request = host_request("show_window", json!({ "reason": "scripted" }));

        let error = validate_host_request(&request).expect_err("unknown open reason should reject");

        assert_eq!(error.code, Some("INVALID_PAYLOAD"));
    }

    #[test]
    fn side_effect_rate_limit_rejects_excessive_requests() {
        reset_side_effect_rate_limit_for_tests();
        let now = Instant::now();

        for _ in 0..SIDE_EFFECT_REQUEST_LIMIT {
            assert!(!is_side_effect_rate_limited_at("enqueue_download", now));
        }

        assert!(is_side_effect_rate_limited_at("enqueue_download", now));
        assert!(!is_side_effect_rate_limited_at("ping", now));
        assert!(!is_side_effect_rate_limited_at(
            "enqueue_download",
            now + SIDE_EFFECT_RATE_LIMIT_WINDOW + Duration::from_millis(1)
        ));
    }

    #[test]
    fn side_effect_client_trust_rejects_untrusted_local_clients() {
        let trust = SideEffectClientTrust::Untrusted {
            process_id: 4242,
            image_path: Some(PathBuf::from(r"C:\Temp\unknown-client.exe")),
        };

        let error = authorize_side_effect_client("enqueue_download", &trust)
            .expect_err("unknown local clients should not be allowed to enqueue downloads");

        assert!(error.contains("unknown-client.exe"));
    }

    #[test]
    fn side_effect_client_trust_preserves_read_only_requests() {
        let trust = SideEffectClientTrust::Untrusted {
            process_id: 4242,
            image_path: Some(PathBuf::from(r"C:\Temp\unknown-client.exe")),
        };

        assert!(authorize_side_effect_client("ping", &trust).is_ok());
        assert!(authorize_side_effect_client("get_status", &trust).is_ok());
    }

    #[test]
    fn side_effect_client_trust_allows_bundled_native_host() {
        let trust = SideEffectClientTrust::TrustedNativeHost {
            process_id: 4242,
            image_path: PathBuf::from(
                r"C:\Program Files\Simple Download Manager\simple-download-manager-native-host.exe",
            ),
        };

        assert!(authorize_side_effect_client("prompt_download", &trust).is_ok());
    }

    #[test]
    fn prompt_dismissed_response_uses_dismissed_status() {
        let response = HostResponse::prompt_dismissed("request-1".into());

        assert!(response.ok);
        assert_eq!(response.message_type, "prompt_dismissed");
        assert_eq!(
            response
                .payload
                .as_ref()
                .and_then(|payload| payload.get("status"))
                .and_then(|status| status.as_str()),
            Some("dismissed")
        );
    }

    #[test]
    fn browser_download_payloads_accept_handoff_auth() {
        let enqueue = serde_json::from_value::<EnqueuePayload>(json!({
            "url": "https://canvas.example.edu/files/569/download?download_frd=1",
            "source": valid_source(),
            "suggestedFilename": "lecture.pdf",
            "handoffAuth": {
                "headers": [
                    { "name": "Cookie", "value": "canvas_session=abc" },
                    { "name": "Referer", "value": "https://canvas.example.edu/courses/1/files" }
                ]
            }
        }))
        .expect("enqueue payload should parse handoff auth");

        assert_eq!(
            enqueue.handoff_auth.as_ref().map(|auth| auth.headers.len()),
            Some(2)
        );

        let prompt = serde_json::from_value::<PromptDownloadPayload>(json!({
            "url": "https://canvas.example.edu/files/569/download?download_frd=1",
            "source": valid_source(),
            "suggestedFilename": "lecture.pdf",
            "handoffAuth": {
                "headers": [
                    { "name": "Cookie", "value": "canvas_session=abc" }
                ]
            }
        }))
        .expect("prompt payload should parse handoff auth");

        assert_eq!(
            prompt
                .handoff_auth
                .as_ref()
                .and_then(|auth| auth.headers.first())
                .map(|header| header.name.as_str()),
            Some("Cookie")
        );
    }

    #[cfg(windows)]
    #[test]
    fn invalid_native_host_manifest_is_reported_as_broken_entry() {
        let root = test_runtime_path("invalid-native-host-manifest");
        let manifest_path = root.join("simple-download-manager-invalid-manifest.json");
        std::fs::write(&manifest_path, "{not valid json").expect("write invalid manifest");

        let entry = super::read_host_registration_entry("Chrome", "Software\\Test", &manifest_path)
            .expect("invalid manifest should not fail diagnostics");

        assert!(entry.manifest_exists);
        assert_eq!(
            entry.manifest_path.as_deref(),
            Some(manifest_path.display().to_string().as_str())
        );
        assert_eq!(entry.host_binary_path, None);
        assert!(!entry.host_binary_exists);

        let _ = std::fs::remove_dir_all(root);
    }
}
