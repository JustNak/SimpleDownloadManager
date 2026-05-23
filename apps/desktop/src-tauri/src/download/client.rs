use super::*;

pub(super) fn download_client() -> Result<Client, DownloadError> {
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }

    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .pool_idle_timeout(Some(Duration::from_secs(120)))
        .pool_max_idle_per_host(64)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .http2_adaptive_window(true)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .redirect(Policy::none())
        .user_agent("SimpleDownloadManager/0.2")
        .build()
        .map_err(|error| format!("Could not create download client: {error}"))?;

    let _ = CLIENT.set(client);
    CLIENT.get().cloned().ok_or_else(|| {
        "Could not initialize shared download client."
            .to_string()
            .into()
    })
}

pub(super) fn segmented_download_client() -> Result<Client, DownloadError> {
    if let Some(client) = SEGMENTED_CLIENT.get() {
        return Ok(client.clone());
    }

    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .pool_idle_timeout(Some(Duration::from_secs(120)))
        .pool_max_idle_per_host(64)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .http1_only()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .redirect(Policy::none())
        .user_agent("SimpleDownloadManager/0.2")
        .build()
        .map_err(|error| format!("Could not create segmented download client: {error}"))?;

    let _ = SEGMENTED_CLIENT.set(client);
    SEGMENTED_CLIENT.get().cloned().ok_or_else(|| {
        "Could not initialize shared segmented download client."
            .to_string()
            .into()
    })
}

#[cfg(windows)]
pub(super) fn segmented_native_tls_download_client() -> Result<Client, DownloadError> {
    if let Some(client) = SEGMENTED_NATIVE_TLS_CLIENT.get() {
        return Ok(client.clone());
    }

    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .pool_idle_timeout(Some(Duration::from_secs(120)))
        .pool_max_idle_per_host(64)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .http1_only()
        .use_native_tls()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .redirect(Policy::none())
        .user_agent("SimpleDownloadManager/0.2")
        .build()
        .map_err(|error| format!("Could not create native TLS segmented client: {error}"))?;

    let _ = SEGMENTED_NATIVE_TLS_CLIENT.set(client);
    SEGMENTED_NATIVE_TLS_CLIENT.get().cloned().ok_or_else(|| {
        "Could not initialize shared native TLS segmented client."
            .to_string()
            .into()
    })
}

pub(super) fn access_probe_download_error(error: DownloadError) -> BrowserHandoffAccessError {
    BrowserHandoffAccessError {
        code: "DOWNLOAD_FAILED",
        message: error.message,
        status: None,
    }
}

#[derive(Default)]
pub(super) struct RangeBackoffRegistry {
    rejected_hosts: StdMutex<HashMap<String, Instant>>,
}

impl RangeBackoffRegistry {
    pub(super) fn record_rejection(&self, url: &str, now: Instant) {
        let Some(key) = range_backoff_key(url) else {
            return;
        };

        self.record_key_rejection(&key, now);
    }

    pub(super) fn record_key_rejection(&self, key: &str, now: Instant) {
        if key.trim().is_empty() {
            return;
        }

        if let Ok(mut rejected_hosts) = self.rejected_hosts.lock() {
            rejected_hosts.insert(key.to_string(), now);
        }
    }

    pub(super) fn is_backed_off(&self, url: &str, now: Instant) -> bool {
        let Some(key) = range_backoff_key(url) else {
            return false;
        };

        self.is_key_backed_off(&key, now)
    }

    pub(super) fn is_key_backed_off(&self, key: &str, now: Instant) -> bool {
        if key.trim().is_empty() {
            return false;
        }

        let Ok(mut rejected_hosts) = self.rejected_hosts.lock() else {
            return false;
        };

        let Some(rejected_at) = rejected_hosts.get(key).copied() else {
            return false;
        };

        if now.duration_since(rejected_at) < RANGE_BACKOFF_DURATION {
            return true;
        }

        rejected_hosts.remove(key);
        false
    }
}

pub(super) fn range_backoffs() -> &'static RangeBackoffRegistry {
    RANGE_BACKOFFS.get_or_init(RangeBackoffRegistry::default)
}

pub(super) fn range_backoff_key(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let path = if parsed.path().is_empty() {
        "/"
    } else {
        parsed.path()
    };
    let query = parsed
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    Some(format!(
        "{}://{}:{}{}{}",
        parsed.scheme(),
        host.to_ascii_lowercase(),
        parsed.port_or_known_default().unwrap_or(0),
        path,
        query
    ))
}

pub(super) async fn send_request(
    client: &Client,
    url: &str,
    existing_bytes: u64,
    handoff_auth: Option<&HandoffAuth>,
    validators: Option<&EntityValidators>,
) -> Result<reqwest::Response, DownloadError> {
    let range_header = if existing_bytes > 0 {
        Some(format!("bytes={existing_bytes}-"))
    } else {
        None
    };
    let if_range = range_header
        .as_ref()
        .and_then(|_| validators.and_then(EntityValidators::if_range_value));

    send_download_request(client, url, range_header, handoff_auth, if_range).await
}

pub(super) async fn send_range_request(
    client: &Client,
    url: &str,
    range: ByteRange,
    handoff_auth: Option<&HandoffAuth>,
    validators: Option<&EntityValidators>,
) -> Result<reqwest::Response, DownloadError> {
    let if_range = validators.and_then(EntityValidators::if_range_value);
    send_download_request(
        client,
        url,
        Some(format!("bytes={}-{}", range.start, range.end)),
        handoff_auth,
        if_range,
    )
    .await
}

pub(super) async fn send_download_request(
    client: &Client,
    url: &str,
    range_header: Option<String>,
    handoff_auth: Option<&HandoffAuth>,
    if_range: Option<&str>,
) -> Result<reqwest::Response, DownloadError> {
    let mut next_retry = 0;
    let mut current_url = url.to_string();
    let mut redirects = 0;

    loop {
        let mut request = client.get(&current_url).header(ACCEPT_ENCODING, "identity");
        if let Some(range_header) = range_header.as_deref() {
            request = request.header(RANGE, range_header);
        }
        if let Some(if_range) = if_range {
            request = request.header(IF_RANGE, if_range);
        }
        let request_auth = handoff_auth_for_request_origin(url, &current_url, handoff_auth);
        request = apply_handoff_auth_headers(request, request_auth)?;

        match request.send().await {
            Ok(response) => {
                if response.status().is_redirection() {
                    let next_url = redirect_location(response.url().as_str(), &response)?;
                    redirects += 1;
                    if redirects > 10 {
                        return Err(download_error(
                            FailureCategory::Http,
                            "Download redirected too many times.".into(),
                            false,
                        ));
                    }

                    current_url = next_url;
                    next_retry = 0;
                    continue;
                }

                if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
                    return Err(download_error(
                        FailureCategory::Resume,
                        "The remote server rejected the resume request.".into(),
                        false,
                    ));
                }

                if response.status().is_success() {
                    return Ok(response);
                }

                let status = response.status();

                if is_gofile_direct_http_url(&current_url)
                    && matches!(
                        status,
                        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND | StatusCode::GONE
                    )
                {
                    return Err(download_error(
                        FailureCategory::Http,
                        format!(
                            "Gofile direct link expired or access was rejected with HTTP {status}. Reopen the source page or capture a fresh download link."
                        ),
                        false,
                    ));
                }

                if should_retry_status(status) && next_retry < REQUEST_RETRY_DELAYS.len() {
                    let delay = retry_delay_for_response(
                        status,
                        response.headers(),
                        next_retry,
                        range_header.as_deref().unwrap_or("request"),
                        &current_url,
                    );
                    tokio::time::sleep(delay).await;
                    next_retry += 1;
                    continue;
                }

                return Err(error_for_http_response(
                    status,
                    response.headers(),
                    request_auth.is_some(),
                ));
            }
            Err(error) => {
                if should_retry_error(&error) && next_retry < REQUEST_RETRY_DELAYS.len() {
                    let delay = retry_delay_for_attempt_with_jitter(
                        next_retry,
                        range_header.as_deref().unwrap_or("request"),
                        &current_url,
                    );
                    tokio::time::sleep(delay).await;
                    next_retry += 1;
                    continue;
                }

                return Err(request_error(error));
            }
        }
    }
}

pub(super) fn handoff_auth_for_request_origin<'a>(
    original_url: &str,
    request_url: &str,
    handoff_auth: Option<&'a HandoffAuth>,
) -> Option<&'a HandoffAuth> {
    let auth = handoff_auth?;

    if request_url == original_url || redirect_keeps_origin(original_url, request_url) {
        Some(auth)
    } else {
        None
    }
}

pub(super) fn apply_handoff_auth_headers(
    mut request: reqwest::RequestBuilder,
    handoff_auth: Option<&HandoffAuth>,
) -> Result<reqwest::RequestBuilder, DownloadError> {
    let Some(auth) = handoff_auth else {
        return Ok(request);
    };

    for header in &auth.headers {
        let name = HeaderName::from_bytes(header.name.as_bytes()).map_err(|_| {
            download_error(
                FailureCategory::Internal,
                "Authenticated handoff header name is invalid.".into(),
                false,
            )
        })?;
        let value = HeaderValue::from_str(&header.value).map_err(|_| {
            download_error(
                FailureCategory::Internal,
                "Authenticated handoff header value is invalid.".into(),
                false,
            )
        })?;
        request = request.header(name, value);
    }

    Ok(request)
}

pub(super) fn redirect_location(
    current_url: &str,
    response: &reqwest::Response,
) -> Result<String, DownloadError> {
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            download_error(
                FailureCategory::Http,
                "Download redirected without a Location header.".into(),
                false,
            )
        })?;
    let base = reqwest::Url::parse(current_url).map_err(|_| {
        download_error(
            FailureCategory::Http,
            "Download redirected from an invalid URL.".into(),
            false,
        )
    })?;
    let next_url = base.join(location).map_err(|_| {
        download_error(
            FailureCategory::Http,
            "Download redirected to an invalid URL.".into(),
            false,
        )
    })?;

    match next_url.scheme() {
        "http" | "https" => Ok(next_url.to_string()),
        _ => Err(download_error(
            FailureCategory::Http,
            "Download redirected to an unsupported URL scheme.".into(),
            false,
        )),
    }
}

pub(super) fn redirect_keeps_origin(current_url: &str, next_url: &str) -> bool {
    let Ok(current) = reqwest::Url::parse(current_url) else {
        return false;
    };
    let Ok(next) = reqwest::Url::parse(next_url) else {
        return false;
    };

    current.scheme() == next.scheme()
        && current.host_str().map(str::to_ascii_lowercase)
            == next.host_str().map(str::to_ascii_lowercase)
        && current.port_or_known_default() == next.port_or_known_default()
}

pub(super) async fn preflight_download(
    client: &Client,
    url: &str,
    handoff_auth: Option<&HandoffAuth>,
) -> Option<PreflightMetadata> {
    let mut current_url = url.to_string();
    let mut redirects = 0;
    let response = loop {
        let request = client
            .head(&current_url)
            .timeout(PREFLIGHT_TIMEOUT)
            .header(ACCEPT_ENCODING, "identity");
        let request_auth = handoff_auth_for_request_origin(url, &current_url, handoff_auth);
        let request = apply_handoff_auth_headers(request, request_auth).ok()?;
        let response = request.send().await.ok()?;
        if !response.status().is_redirection() {
            break response;
        }

        let next_url = redirect_location(response.url().as_str(), &response).ok()?;
        redirects += 1;
        if redirects > 10 {
            return None;
        }
        current_url = next_url;
    };
    if !response.status().is_success() {
        return None;
    }

    let accept_ranges = response
        .headers()
        .get(ACCEPT_RANGES)
        .and_then(|value| value.to_str().ok());
    let content_disposition = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok());

    Some(derive_preflight_metadata_from_parts(
        response.content_length(),
        accept_ranges,
        content_disposition,
        response.url().as_str(),
        entity_validators_from_headers(response.headers()),
    ))
}

pub(super) fn derive_preflight_metadata_from_parts(
    total_bytes: Option<u64>,
    accept_ranges: Option<&str>,
    content_disposition: Option<&str>,
    final_url: &str,
    validators: EntityValidators,
) -> PreflightMetadata {
    PreflightMetadata {
        total_bytes,
        resume_support: derive_resume_support_from_parts(StatusCode::OK, 0, accept_ranges),
        filename: content_disposition
            .and_then(parse_content_disposition_filename)
            .or_else(|| derive_filename_from_url(final_url)),
        validators,
    }
}

pub(super) fn unusable_download_response_error_for_parts(
    raw_url: &str,
    headers: &HeaderMap,
    browser_handoff: bool,
    resolved_hoster: bool,
) -> Option<DownloadError> {
    let content_type = normalized_content_type(headers);
    let is_html = content_type.as_deref().is_some_and(is_html_content_type);
    let is_json = content_type.as_deref().is_some_and(is_json_content_type);
    let attachment_filename = attachment_filename_from_headers(headers);
    let explicit_html_attachment = attachment_filename
        .as_deref()
        .is_some_and(filename_has_html_extension);

    if is_gofile_direct_http_url(raw_url) && (is_html || is_json) && attachment_filename.is_none() {
        return Some(download_error(
            FailureCategory::Http,
            "Gofile direct link returned a hoster response instead of file content. Reopen the source page or capture a fresh download link."
                .into(),
            false,
        ));
    }

    if resolved_hoster && is_html {
        return Some(download_error(
            FailureCategory::Http,
            "Hoster direct link returned HTML instead of file content.".into(),
            true,
        ));
    }

    if browser_handoff && is_html && !explicit_html_attachment {
        return Some(download_error(
            FailureCategory::Http,
            "Browser download handoff returned HTML instead of file content. Let the browser handle this download or enable Protected Downloads for this site."
                .into(),
            false,
        ));
    }

    None
}

pub(super) fn unusable_browser_handoff_access_error(
    response: &reqwest::Response,
) -> Option<BrowserHandoffAccessError> {
    let error = unusable_download_response_error_for_parts(
        response.url().as_str(),
        response.headers(),
        true,
        false,
    )?;

    Some(BrowserHandoffAccessError {
        code: PROTECTED_DOWNLOAD_AUTH_REQUIRED_CODE,
        message: error.message,
        status: Some(response.status().as_u16()),
    })
}

fn normalized_content_type(headers: &HeaderMap) -> Option<String> {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

fn is_html_content_type(content_type: &str) -> bool {
    matches!(content_type, "text/html" | "application/xhtml+xml")
}

fn is_json_content_type(content_type: &str) -> bool {
    content_type == "application/json" || content_type.ends_with("+json")
}

fn attachment_filename_from_headers(headers: &HeaderMap) -> Option<String> {
    let content_disposition = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())?;
    let has_attachment_disposition = content_disposition
        .split(';')
        .any(|segment| segment.trim().eq_ignore_ascii_case("attachment"));
    if !has_attachment_disposition {
        return None;
    }

    parse_content_disposition_filename(content_disposition)
}

fn filename_has_html_extension(filename: &str) -> bool {
    let filename = filename.trim().to_ascii_lowercase();
    filename.ends_with(".html") || filename.ends_with(".htm")
}

pub(super) fn entity_validators_from_headers(
    headers: &reqwest::header::HeaderMap,
) -> EntityValidators {
    EntityValidators {
        etag: headers
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
        last_modified: headers
            .get(LAST_MODIFIED)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    }
}

pub(super) fn derive_total_bytes(response: &reqwest::Response, existing_bytes: u64) -> Option<u64> {
    if response.status() == StatusCode::PARTIAL_CONTENT {
        if let Some(total_bytes) = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_range_total)
        {
            return Some(total_bytes);
        }

        return response
            .content_length()
            .map(|length| existing_bytes.saturating_add(length));
    }

    response.content_length()
}

pub(super) fn derive_resume_support(
    response: &reqwest::Response,
    existing_bytes: u64,
) -> ResumeSupport {
    let accept_ranges = response
        .headers()
        .get(ACCEPT_RANGES)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    derive_resume_support_from_parts(response.status(), existing_bytes, accept_ranges.as_deref())
}

pub(super) fn derive_resume_support_from_parts(
    status: StatusCode,
    existing_bytes: u64,
    accept_ranges: Option<&str>,
) -> ResumeSupport {
    if status == StatusCode::PARTIAL_CONTENT {
        return ResumeSupport::Supported;
    }

    if existing_bytes > 0 {
        return ResumeSupport::Unsupported;
    }

    accept_ranges
        .map(|value| {
            if value.to_ascii_lowercase().contains("bytes") {
                ResumeSupport::Supported
            } else {
                ResumeSupport::Unsupported
            }
        })
        .unwrap_or(ResumeSupport::Unknown)
}
