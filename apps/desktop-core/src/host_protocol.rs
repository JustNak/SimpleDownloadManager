use crate::contracts::{
    BrowserDownloadAccessError, BrowserDownloadAccessProbe, DesktopEvent, ShellServices,
};
use crate::prompts::{PromptDecision, PromptDuplicateAction, PromptRegistry};
use crate::state::{
    BackendError, DuplicatePolicy, EnqueueOptions, EnqueueResult, EnqueueStatus, SharedState,
};
use crate::storage::{
    ConnectionState, DiagnosticLevel, DownloadPrompt, DownloadSource, ExtensionIntegrationSettings,
    HandoffAuth, HandoffAuthHeader, HostRegistrationStatus, QueueSummary,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use url::Url;

pub const PROTOCOL_VERSION: u32 = 1;
pub const PROTECTED_DOWNLOAD_AUTH_REQUIRED_CODE: &str = "PROTECTED_DOWNLOAD_AUTH_REQUIRED";
const HOST_CONTACT_TTL: Duration = Duration::from_secs(20);
const MAX_REQUEST_ID_LENGTH: usize = 128;
const MAX_URL_LENGTH: usize = 2048;
const MAX_METADATA_LENGTH: usize = 512;
const MAX_HANDOFF_AUTH_HEADERS: usize = 16;
const MAX_HANDOFF_AUTH_HEADER_NAME_LENGTH: usize = 64;
const MAX_HANDOFF_AUTH_HEADER_VALUE_LENGTH: usize = 16 * 1024;
const SIDE_EFFECT_REQUEST_LIMIT: usize = 30;
const SIDE_EFFECT_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10);
static SIDE_EFFECT_REQUEST_TIMES: OnceLock<Mutex<VecDeque<Instant>>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostRequest {
    pub protocol_version: u32,
    pub request_id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub payload: Value,
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
    handoff_auth: Option<HandoffAuth>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptDownloadPayload {
    url: String,
    source: EnqueueSource,
    suggested_filename: Option<String>,
    total_bytes: Option<u64>,
    handoff_auth: Option<HandoffAuth>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenAppPayload {
    reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostResponse {
    pub ok: bool,
    pub request_id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub type ValidationResult = Result<(), Box<HostResponse>>;
type ValidationParseResult<T> = Result<T, Box<HostResponse>>;

impl HostResponse {
    pub fn ready(
        request_id: String,
        app_state: &str,
        connection_state: ConnectionState,
        queue_summary: QueueSummary,
        extension_settings: ExtensionIntegrationSettings,
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
            })),
            code: None,
            message: None,
        }
    }

    pub fn enqueue_result(request_id: String, result: EnqueueResult) -> Self {
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

    pub fn existing_job(request_id: String, job_id: String, filename: String) -> Self {
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

    pub fn prompt_canceled(request_id: String) -> Self {
        Self {
            ok: true,
            request_id,
            message_type: "prompt_canceled".into(),
            payload: Some(json!({
                "status": "canceled",
            })),
            code: None,
            message: None,
        }
    }

    pub fn prompt_dismissed(request_id: String) -> Self {
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

    pub fn error(
        request_id: String,
        message_type: &str,
        code: &'static str,
        message: String,
    ) -> Self {
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

pub async fn handle_host_request<S>(
    state: SharedState,
    prompts: PromptRegistry,
    shell: &S,
    request: HostRequest,
) -> HostResponse
where
    S: ShellServices + ?Sized,
{
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

    if should_register_host_contact_before_response(&request.message_type) {
        register_host_contact(shell, &state).await;
    }

    match request.message_type.as_str() {
        "ping" | "get_status" => {
            let connection_state = register_host_contact(shell, &state).await;
            ready_response(request.request_id, "running", &state, connection_state).await
        }
        "open_app" | "show_window" => {
            let _ = shell.focus_main_window().await;
            let connection_state = register_host_contact(shell, &state).await;
            ready_response(request.request_id, "launched", &state, connection_state).await
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
                    emit_snapshot(shell, &snapshot).await;
                    let connection_state = register_host_contact(shell, &state).await;
                    ready_response(request.request_id, "running", &state, connection_state).await
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
                shell,
                state,
                prompts,
                request.request_id,
                prompt,
                source,
                payload.handoff_auth,
            )
            .await
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
            if source.entry_point == "browser_download" {
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
                        shell,
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

            if let Err(error) = probe_browser_download_access(
                shell,
                &state,
                &source,
                &payload.url,
                payload.handoff_auth.clone(),
            )
            .await
            {
                return map_backend_error(request.request_id, error);
            }

            match state
                .enqueue_download_with_options(
                    payload.url,
                    EnqueueOptions {
                        source: Some(source),
                        filename_hint: payload.suggested_filename,
                        handoff_auth: payload.handoff_auth,
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(result) => {
                    let host_snapshot = state.register_host_contact().await;
                    emit_snapshot(shell, &result.snapshot).await;
                    emit_snapshot(shell, &host_snapshot).await;
                    if result.status == EnqueueStatus::Queued {
                        let _ = shell.schedule_downloads(state).await;
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

pub async fn refresh_host_connection_diagnostics<S>(
    state: &SharedState,
    shell: &S,
) -> Result<(), String>
where
    S: ShellServices + ?Sized,
{
    let desired_state = if state.has_recent_host_contact(HOST_CONTACT_TTL).await {
        ConnectionState::Connected
    } else {
        match shell.gather_host_registration_diagnostics().await {
            Ok(diagnostics) => match diagnostics.status {
                HostRegistrationStatus::Configured => ConnectionState::Checking,
                HostRegistrationStatus::Missing => ConnectionState::HostMissing,
                HostRegistrationStatus::Broken => ConnectionState::Error,
            },
            Err(_) => ConnectionState::Error,
        }
    };

    if state.connection_state().await != desired_state {
        let snapshot = state.set_connection_state(desired_state).await?;
        emit_snapshot(shell, &snapshot).await;
    }

    Ok(())
}

pub fn validate_host_request(request: &HostRequest) -> ValidationResult {
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
    )?;
    validate_handoff_auth(
        &request.request_id,
        payload.handoff_auth.as_ref(),
        &payload.source,
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
    )?;
    validate_handoff_auth(
        &request.request_id,
        payload.handoff_auth.as_ref(),
        &payload.source,
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

fn validate_handoff_auth(
    request_id: &str,
    auth: Option<&HandoffAuth>,
    source: &EnqueueSource,
) -> ValidationResult {
    let Some(auth) = auth else {
        return Ok(());
    };

    if source.entry_point != "browser_download" {
        return Err(validation_error(
            request_id,
            "INVALID_PAYLOAD",
            "Authenticated handoff is only supported for browser downloads.",
        ));
    }

    if auth.headers.is_empty() || auth.headers.len() > MAX_HANDOFF_AUTH_HEADERS {
        return Err(validation_error(
            request_id,
            "INVALID_PAYLOAD",
            "Authenticated handoff header count is not supported.",
        ));
    }

    for header in &auth.headers {
        validate_handoff_auth_header(request_id, header)?;
    }

    Ok(())
}

fn validate_handoff_auth_header(request_id: &str, header: &HandoffAuthHeader) -> ValidationResult {
    let name = header.name.trim();
    if name.is_empty()
        || name.len() > MAX_HANDOFF_AUTH_HEADER_NAME_LENGTH
        || header.value.len() > MAX_HANDOFF_AUTH_HEADER_VALUE_LENGTH
        || name.contains(':')
        || name.contains('\r')
        || name.contains('\n')
        || header.value.contains('\r')
        || header.value.contains('\n')
        || !is_allowed_handoff_auth_header(name)
    {
        return Err(validation_error(
            request_id,
            "INVALID_PAYLOAD",
            "Authenticated handoff header is not allowed.",
        ));
    }

    Ok(())
}

fn is_allowed_handoff_auth_header(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    matches!(
        name.as_str(),
        "cookie"
            | "authorization"
            | "referer"
            | "origin"
            | "user-agent"
            | "accept"
            | "accept-language"
    ) || name.starts_with("sec-fetch-")
        || name.starts_with("sec-ch-ua")
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
    )
}

fn side_effect_request_times() -> &'static Mutex<VecDeque<Instant>> {
    SIDE_EFFECT_REQUEST_TIMES.get_or_init(|| Mutex::new(VecDeque::new()))
}

async fn ready_response(
    request_id: String,
    app_state: &str,
    state: &SharedState,
    connection_state: ConnectionState,
) -> HostResponse {
    let queue_summary = state.queue_summary().await;
    let extension_settings = state.extension_integration_settings().await;
    HostResponse::ready(
        request_id,
        app_state,
        connection_state,
        queue_summary,
        extension_settings,
    )
}

async fn register_host_contact<S>(shell: &S, state: &SharedState) -> ConnectionState
where
    S: ShellServices + ?Sized,
{
    let snapshot = state.register_host_contact().await;
    let connection_state = snapshot.connection_state;
    emit_snapshot(shell, &snapshot).await;
    connection_state
}

async fn emit_snapshot<S>(shell: &S, snapshot: &crate::storage::DesktopSnapshot)
where
    S: ShellServices + ?Sized,
{
    let _ = shell
        .emit_event(DesktopEvent::StateChanged(Box::new(snapshot.clone())))
        .await;
}

async fn probe_browser_download_access<S>(
    shell: &S,
    state: &SharedState,
    source: &DownloadSource,
    url: &str,
    handoff_auth: Option<HandoffAuth>,
) -> Result<(), BackendError>
where
    S: ShellServices + ?Sized,
{
    if source.entry_point != "browser_download" {
        return Ok(());
    }

    let (header_count, header_names) = handoff_auth_header_summary(handoff_auth.as_ref());
    let protected_auth_attached = handoff_auth.is_some();
    let _ = state
        .record_diagnostic_event(
            DiagnosticLevel::Info,
            "extension",
            format!(
                "Protected download access probe started: protectedAuthAttached={protected_auth_attached} headerCount={header_count} headerNames={header_names}"
            ),
            None,
        )
        .await;

    match shell
        .probe_browser_download_access(
            state.clone(),
            source.clone(),
            url.to_string(),
            handoff_auth.clone(),
        )
        .await
    {
        Ok(BrowserDownloadAccessProbe { status }) => {
            let _ = state
                .record_diagnostic_event(
                    DiagnosticLevel::Info,
                    "extension",
                    format!(
                        "Protected download access probe succeeded: accessProbeStatus={status} protectedAuthAttached={protected_auth_attached} headerCount={header_count} headerNames={header_names}"
                    ),
                    None,
                )
                .await;
            Ok(())
        }
        Err(error) => {
            record_browser_probe_error(
                state,
                &error,
                protected_auth_attached,
                header_count,
                header_names,
            )
            .await;
            Err(BackendError {
                code: error.code,
                message: error.message,
            })
        }
    }
}

async fn record_browser_probe_error(
    state: &SharedState,
    error: &BrowserDownloadAccessError,
    protected_auth_attached: bool,
    header_count: usize,
    header_names: String,
) {
    let access_probe_status = error
        .status
        .map(|status| status.to_string())
        .unwrap_or_else(|| "none".into());
    let level = if error.code == PROTECTED_DOWNLOAD_AUTH_REQUIRED_CODE {
        DiagnosticLevel::Warning
    } else {
        DiagnosticLevel::Error
    };
    let _ = state
        .record_diagnostic_event(
            level,
            "extension",
            format!(
                "Protected download access probe failed: accessProbeStatus={access_probe_status} protectedAuthAttached={protected_auth_attached} headerCount={header_count} headerNames={header_names}"
            ),
            None,
        )
        .await;
}

fn handoff_auth_header_summary(handoff_auth: Option<&HandoffAuth>) -> (usize, String) {
    let Some(auth) = handoff_auth else {
        return (0, "none".into());
    };
    let mut names: Vec<String> = auth
        .headers
        .iter()
        .map(|header| header.name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    names.sort();
    names.dedup();

    if names.is_empty() {
        (auth.headers.len(), "none".into())
    } else {
        (auth.headers.len(), names.join(","))
    }
}

pub fn prompt_enqueue_details(
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

pub fn map_backend_error(request_id: String, error: BackendError) -> HostResponse {
    let message_type = match error.code {
        "DUPLICATE_JOB" => "duplicate_existing_job",
        "INVALID_URL" | "UNSUPPORTED_SCHEME" => "invalid_url",
        _ => "blocked_by_policy",
    };

    HostResponse::error(request_id, message_type, error.code, error.message)
}

fn should_register_host_contact_before_response(message_type: &str) -> bool {
    matches!(message_type, "prompt_download")
}

async fn run_prompt_download<S>(
    shell: &S,
    state: SharedState,
    prompts: PromptRegistry,
    request_id: String,
    prompt: DownloadPrompt,
    source: DownloadSource,
    handoff_auth: Option<HandoffAuth>,
) -> HostResponse
where
    S: ShellServices + ?Sized,
{
    let receiver = prompts.enqueue(prompt.clone()).await;
    if let Err(error) = shell.show_download_prompt_window().await {
        let _ = prompts.resolve(&prompt.id, PromptDecision::Cancel).await;
        return HostResponse::error(
            request_id,
            "blocked_by_policy",
            "INTERNAL_ERROR",
            format!("Could not open download prompt: {error}"),
        );
    }
    if let Some(active_prompt) = prompts.active_prompt().await {
        let _ = shell
            .emit_event(DesktopEvent::DownloadPromptChanged(Some(Box::new(
                active_prompt,
            ))))
            .await;
    }

    match receiver.await.unwrap_or(PromptDecision::SwapToBrowser) {
        PromptDecision::Cancel => HostResponse::prompt_dismissed(request_id),
        PromptDecision::SwapToBrowser => HostResponse::prompt_canceled(request_id),
        PromptDecision::ShowExisting => {
            if let Some(job) = prompt.duplicate_job {
                let _ = shell.focus_job_in_main_window(job.id.clone()).await;
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
            if let Err(error) = probe_browser_download_access(
                shell,
                &state,
                &source,
                &prompt.url,
                handoff_auth.clone(),
            )
            .await
            {
                return map_backend_error(request_id, error);
            }
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
                    emit_snapshot(shell, &result.snapshot).await;
                    if result.status == EnqueueStatus::Queued {
                        if show_progress {
                            let transfer_kind = result
                                .snapshot
                                .jobs
                                .iter()
                                .find(|job| job.id == result.job_id)
                                .map(|job| job.transfer_kind)
                                .unwrap_or_default();
                            let _ = shell
                                .show_progress_window(result.job_id.clone(), transfer_kind)
                                .await;
                        }
                        let _ = shell.schedule_downloads(state).await;
                    }
                    HostResponse::enqueue_result(request_id, result)
                }
                Err(error) => map_backend_error(request_id, error),
            }
        }
    }
}

fn prompt_has_duplicate(prompt: &DownloadPrompt) -> bool {
    prompt.duplicate_job.is_some()
        || prompt.duplicate_path.is_some()
        || prompt.duplicate_reason.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            "extensionVersion": "0.3.52-alpha"
        })
    }

    fn valid_enqueue_payload() -> Value {
        json!({
            "url": "https://example.com/file.zip",
            "source": valid_source()
        })
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
        side_effect_request_times()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
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
}
