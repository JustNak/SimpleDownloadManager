use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
pub struct NativeRequestEnvelope {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: u32,
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub payload: Value,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RequestSource {
    #[serde(rename = "entryPoint")]
    pub entry_point: String,
    pub browser: String,
    #[serde(rename = "extensionVersion")]
    pub extension_version: String,
    #[serde(rename = "pageUrl")]
    pub page_url: Option<String>,
    #[serde(rename = "pageTitle")]
    pub page_title: Option<String>,
    pub referrer: Option<String>,
    pub incognito: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EnqueueDownloadPayload {
    pub url: String,
    pub source: RequestSource,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PromptDownloadPayload {
    pub url: String,
    pub source: RequestSource,
    #[serde(rename = "suggestedFilename")]
    pub suggested_filename: Option<String>,
    #[serde(rename = "totalBytes")]
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpenAppPayload {
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct AppRequestEnvelope<T>
where
    T: Serialize,
{
    #[serde(rename = "protocolVersion")]
    pub protocol_version: u32,
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub payload: T,
}

#[derive(Debug, Deserialize)]
pub struct AppResponseEnvelope {
    pub ok: bool,
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub payload: Option<Value>,
    pub code: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HostResponseEnvelope {
    pub ok: bool,
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl HostResponseEnvelope {
    pub fn pong(request_id: String, payload: Value) -> Self {
        Self {
            ok: true,
            request_id,
            message_type: "pong".into(),
            payload: Some(payload),
            code: None,
            message: None,
        }
    }

    pub fn accepted_with_status(
        request_id: String,
        job_id: Option<&str>,
        filename: Option<&str>,
        status: &str,
        app_state: &str,
    ) -> Self {
        let mut payload = serde_json::Map::new();
        payload.insert("status".into(), json!(status));
        payload.insert("appState".into(), json!(app_state));
        if let Some(job_id) = job_id {
            payload.insert("jobId".into(), json!(job_id));
        }
        if let Some(filename) = filename {
            payload.insert("filename".into(), json!(filename));
        }

        Self {
            ok: true,
            request_id,
            message_type: "accepted".into(),
            payload: Some(Value::Object(payload)),
            code: None,
            message: None,
        }
    }

    pub fn rejected(
        request_id: String,
        response_type: &str,
        code: &str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            ok: false,
            request_id,
            message_type: response_type.into(),
            payload: None,
            code: Some(code.into()),
            message: Some(message.into()),
        }
    }
}

pub fn validate_protocol(request: &NativeRequestEnvelope) -> Result<(), HostResponseEnvelope> {
    if request.protocol_version != PROTOCOL_VERSION {
        return Err(HostResponseEnvelope::rejected(
            request.request_id.clone(),
            "rejected",
            "HOST_PROTOCOL_MISMATCH",
            format!(
                "Expected protocol version {}, got {}.",
                PROTOCOL_VERSION, request.protocol_version
            ),
        ));
    }

    Ok(())
}

pub fn parse_enqueue_payload(
    request: &NativeRequestEnvelope,
) -> Result<EnqueueDownloadPayload, HostResponseEnvelope> {
    let payload = serde_json::from_value::<EnqueueDownloadPayload>(request.payload.clone())
        .map_err(|error| {
            HostResponseEnvelope::rejected(
                request.request_id.clone(),
                "invalid_payload",
                "INVALID_PAYLOAD",
                format!("Payload could not be parsed: {error}"),
            )
        })?;

    validate_http_url(&request.request_id, &payload.url)?;
    Ok(payload)
}

pub fn parse_prompt_download_payload(
    request: &NativeRequestEnvelope,
) -> Result<PromptDownloadPayload, HostResponseEnvelope> {
    let payload = serde_json::from_value::<PromptDownloadPayload>(request.payload.clone())
        .map_err(|error| {
            HostResponseEnvelope::rejected(
                request.request_id.clone(),
                "invalid_payload",
                "INVALID_PAYLOAD",
                format!("Payload could not be parsed: {error}"),
            )
        })?;

    validate_http_url(&request.request_id, &payload.url)?;
    Ok(payload)
}

pub fn parse_open_app_payload(
    request: &NativeRequestEnvelope,
) -> Result<OpenAppPayload, HostResponseEnvelope> {
    serde_json::from_value::<OpenAppPayload>(request.payload.clone()).map_err(|error| {
        HostResponseEnvelope::rejected(
            request.request_id.clone(),
            "invalid_payload",
            "INVALID_PAYLOAD",
            format!("Payload could not be parsed: {error}"),
        )
    })
}

pub fn validate_http_url(request_id: &str, raw_url: &str) -> Result<(), HostResponseEnvelope> {
    let parsed = Url::parse(raw_url).map_err(|_| {
        HostResponseEnvelope::rejected(
            request_id.to_string(),
            "invalid_payload",
            "INVALID_URL",
            "URL is not valid.",
        )
    })?;

    match parsed.scheme() {
        "http" | "https" => Ok(()),
        _ => Err(HostResponseEnvelope::rejected(
            request_id.to_string(),
            "invalid_payload",
            "UNSUPPORTED_SCHEME",
            "Only http and https URLs are supported.",
        )),
    }
}

pub fn app_status_request(request_id: String) -> AppRequestEnvelope<Value> {
    AppRequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        message_type: "get_status".into(),
        payload: json!({}),
    }
}

pub fn app_show_window_request(
    request_id: String,
    payload: OpenAppPayload,
) -> AppRequestEnvelope<OpenAppPayload> {
    AppRequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        message_type: "show_window".into(),
        payload,
    }
}

pub fn app_enqueue_request(
    request_id: String,
    payload: EnqueueDownloadPayload,
) -> AppRequestEnvelope<EnqueueDownloadPayload> {
    AppRequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        message_type: "enqueue_download".into(),
        payload,
    }
}

pub fn app_prompt_download_request(
    request_id: String,
    payload: PromptDownloadPayload,
) -> AppRequestEnvelope<PromptDownloadPayload> {
    AppRequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        message_type: "prompt_download".into(),
        payload,
    }
}

pub fn app_save_extension_settings_request(
    request_id: String,
    payload: Value,
) -> AppRequestEnvelope<Value> {
    AppRequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        message_type: "save_extension_settings".into(),
        payload,
    }
}
