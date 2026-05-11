use crate::storage::{HandoffAuth, HandoffAuthHeader};
use futures_util::{stream, StreamExt};
use percent_encoding::percent_decode_str;
use reqwest::header::{COOKIE, REFERER};
use reqwest::redirect::Policy;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;
use url::Url;

const FUCKINGFAST_HOST: &str = "fuckingfast.co";
const FUCKINGFAST_DIRECT_HOST: &str = "dl.fuckingfast.co";
const DATANODES_HOST: &str = "datanodes.to";
const DATANODES_DOWNLOAD_URL: &str = "https://datanodes.to/download";
const DATANODES_DIRECT_SUFFIX: &str = ".datanodes.to";
const DATANODES_PROXY_SUFFIX: &str = ".dlproxy.uk";
const DATANODES_FREE_METHOD: &str = "Free Download >>";
const HOSTER_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HOSTER_READ_TIMEOUT: Duration = Duration::from_secs(30);
const DATANODES_MAX_WAIT_SECONDS: u64 = 60;
const DATANODES_MAX_PRELIMINARY_STEPS: usize = 2;
const HOSTER_RESOLUTION_CONCURRENCY: usize = 6;
const HOSTER_RESOLUTION_RETRY_ATTEMPTS: usize = 3;

#[cfg(test)]
const HOSTER_RESOLUTION_RETRY_DELAYS: [Duration; 2] =
    [Duration::from_millis(0), Duration::from_millis(0)];
#[cfg(not(test))]
const HOSTER_RESOLUTION_RETRY_DELAYS: [Duration; 2] =
    [Duration::from_millis(500), Duration::from_secs(2)];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHosterLink {
    pub url: String,
    pub filename_hint: Option<String>,
    pub resolved_from_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HosterResolutionError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedHosterLink {
    pub url: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HosterResolutionBatch {
    pub links: Vec<ResolvedHosterLink>,
    pub failed_links: Vec<FailedHosterLink>,
}

pub type HosterDownloadContext = HandoffAuth;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HosterLinkRefreshOutcome {
    pub url: String,
    pub filename_hint: Option<String>,
    pub resolved_from_url: Option<String>,
    pub download_context: Option<HosterDownloadContext>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HosterSourcePreflight {
    pub filename_hint: Option<String>,
    pub resolved_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HosterAccelerationPolicy {
    pub backoff_key: String,
    pub max_balanced_segments: usize,
    pub max_fast_segments: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HosterKind {
    FuckingFast,
    Datanodes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DatanodesDirectUrlKind {
    Native,
    Proxy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DatanodesDownloadPage {
    code: String,
    referer: String,
    rand: String,
    free_method: String,
    premium_method: String,
    countdown_secs: u64,
    filename_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DatanodesPreliminaryDownloadPage {
    action_url: String,
    op: String,
    usr_login: String,
    id: String,
    fname: String,
    referer: String,
    method_free: String,
}

#[derive(Debug, Deserialize)]
struct DatanodesDirectLinkResponse {
    url: Option<String>,
    error: Option<String>,
}

type IndexedResolvedHosterLink = Result<(usize, ResolvedHosterLink), (usize, FailedHosterLink)>;

pub async fn resolve_hoster_links(
    urls: Vec<String>,
) -> Result<Vec<ResolvedHosterLink>, HosterResolutionError> {
    let batch = resolve_hoster_links_partial(urls).await?;
    if let Some(failed) = batch.failed_links.into_iter().next() {
        return Err(resolution_error(failed.message));
    }
    Ok(batch.links)
}

pub async fn refresh_resolved_hoster_link(
    source_url: &str,
) -> Result<HosterLinkRefreshOutcome, HosterResolutionError> {
    let mut links = resolve_hoster_links(vec![source_url.to_string()]).await?;
    let link = links.pop().ok_or_else(|| {
        resolution_error("Hoster resolver did not return a refreshed link.".into())
    })?;
    let download_context =
        hoster_download_context_for_resolved_url(&link.url, link.resolved_from_url.as_deref());

    Ok(HosterLinkRefreshOutcome {
        url: link.url,
        filename_hint: link.filename_hint,
        resolved_from_url: link.resolved_from_url,
        download_context,
    })
}

pub fn is_supported_hoster_url(raw_url: &str) -> bool {
    hoster_kind_for_url(raw_url).is_some()
}

pub fn source_filename_hint_for_url(raw_url: &str) -> Option<String> {
    filename_hint_from_fragment(raw_url).or_else(|| datanodes_filename_hint_from_url(raw_url))
}

pub async fn preflight_hoster_source(
    source_url: &str,
) -> Result<Option<HosterSourcePreflight>, HosterResolutionError> {
    let trimmed_url = source_url.trim().to_string();
    let Some(kind) = hoster_kind_for_url(&trimmed_url) else {
        return Ok(None);
    };
    let client = hoster_client()?;
    let preflight = retry_hoster_resolution("hoster source preflight", || {
        let client = client.clone();
        let source_url = trimmed_url.clone();
        async move {
            match kind {
                HosterKind::FuckingFast => preflight_fuckingfast_source(&client, &source_url).await,
                HosterKind::Datanodes => preflight_datanodes_source(&client, &source_url).await,
            }
        }
    })
    .await?;
    Ok(Some(preflight))
}

pub async fn resolve_hoster_links_partial(
    urls: Vec<String>,
) -> Result<HosterResolutionBatch, HosterResolutionError> {
    let client = hoster_client()?;

    let expected_len = urls.len();
    let indexed = stream::iter(urls.into_iter().enumerate().map(|(index, url)| {
        let client = client.clone();
        async move {
            let failed_url = url.trim().to_string();
            resolve_hoster_link_with_retry(&client, url)
                .await
                .map(|link| (index, link))
                .map_err(|error| {
                    (
                        index,
                        FailedHosterLink {
                            url: failed_url,
                            message: error.message,
                        },
                    )
                })
        }
    }))
    .buffer_unordered(HOSTER_RESOLUTION_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;

    ordered_resolved_hoster_batch(indexed, expected_len)
}

fn hoster_client() -> Result<Client, HosterResolutionError> {
    Client::builder()
        .connect_timeout(HOSTER_CONNECT_TIMEOUT)
        .read_timeout(HOSTER_READ_TIMEOUT)
        .redirect(Policy::limited(5))
        .user_agent("SimpleDownloadManager/0.5")
        .build()
        .map_err(|error| resolution_error(format!("Could not create hoster resolver: {error}")))
}

async fn resolve_hoster_link_with_retry(
    client: &Client,
    url: String,
) -> Result<ResolvedHosterLink, HosterResolutionError> {
    retry_hoster_resolution("hoster resolver", || {
        let client = client.clone();
        let url = url.clone();
        async move { resolve_hoster_link(&client, url).await }
    })
    .await
}

async fn retry_hoster_resolution<T, F, Fut>(
    _operation_name: &str,
    mut operation: F,
) -> Result<T, HosterResolutionError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, HosterResolutionError>>,
{
    let mut attempt = 0;
    loop {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) if error.retryable && attempt + 1 < HOSTER_RESOLUTION_RETRY_ATTEMPTS => {
                let delay = HOSTER_RESOLUTION_RETRY_DELAYS
                    .get(attempt)
                    .copied()
                    .unwrap_or_else(|| *HOSTER_RESOLUTION_RETRY_DELAYS.last().unwrap());
                attempt += 1;
                tokio::time::sleep(delay).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn resolve_hoster_link(
    client: &Client,
    url: String,
) -> Result<ResolvedHosterLink, HosterResolutionError> {
    let trimmed_url = url.trim().to_string();
    match hoster_kind_for_url(&trimmed_url) {
        Some(HosterKind::FuckingFast) => resolve_fuckingfast_link(client, &trimmed_url).await,
        Some(HosterKind::Datanodes) => resolve_datanodes_link(client, &trimmed_url).await,
        None => Ok(ResolvedHosterLink {
            url: trimmed_url,
            filename_hint: None,
            resolved_from_url: None,
        }),
    }
}

async fn resolve_fuckingfast_link(
    client: &Client,
    url: &str,
) -> Result<ResolvedHosterLink, HosterResolutionError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| reqwest_resolution_error("Could not load FuckingFast page", error))?;

    if !response.status().is_success() {
        return Err(http_status_resolution_error(
            "Could not load FuckingFast page",
            response.status(),
        ));
    }

    let html = response
        .text()
        .await
        .map_err(|error| reqwest_resolution_error("Could not read FuckingFast page", error))?;
    resolve_hoster_link_from_html(url, &html)
}

async fn preflight_fuckingfast_source(
    client: &Client,
    url: &str,
) -> Result<HosterSourcePreflight, HosterResolutionError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| reqwest_resolution_error("Could not load FuckingFast page", error))?;

    if !response.status().is_success() {
        return Err(http_status_resolution_error(
            "Could not load FuckingFast page",
            response.status(),
        ));
    }

    let html = response
        .text()
        .await
        .map_err(|error| reqwest_resolution_error("Could not read FuckingFast page", error))?;
    preflight_fuckingfast_source_from_html(url, &html)
}

fn preflight_fuckingfast_source_from_html(
    original_url: &str,
    html: &str,
) -> Result<HosterSourcePreflight, HosterResolutionError> {
    let resolved = resolve_hoster_link_from_html(original_url, html)?;
    Ok(HosterSourcePreflight {
        filename_hint: resolved.filename_hint,
        resolved_url: None,
    })
}

async fn resolve_datanodes_link(
    client: &Client,
    original_url: &str,
) -> Result<ResolvedHosterLink, HosterResolutionError> {
    let file_code = datanodes_file_code_from_url(original_url).ok_or_else(|| {
        resolution_error("DataNodes URL does not contain a supported file code.".into())
    })?;
    let cookie = format!("file_code={file_code}");
    let response = client
        .get(DATANODES_DOWNLOAD_URL)
        .header(COOKIE, cookie.clone())
        .send()
        .await
        .map_err(|error| reqwest_resolution_error("Could not load DataNodes page", error))?;

    if !response.status().is_success() {
        return Err(http_status_resolution_error(
            "Could not load DataNodes page",
            response.status(),
        ));
    }

    let html = response
        .text()
        .await
        .map_err(|error| reqwest_resolution_error("Could not read DataNodes page", error))?;
    let page =
        resolve_datanodes_download_page_from_html(html, &file_code, {
            let client = client.clone();
            let cookie = cookie.clone();
            move |preliminary| {
                let client = client.clone();
                let cookie = cookie.clone();
                async move {
                    request_datanodes_preliminary_download(&client, &cookie, preliminary).await
                }
            }
        })
        .await?;

    let wait_seconds = page.countdown_secs.min(DATANODES_MAX_WAIT_SECONDS);
    if wait_seconds > 0 {
        tokio::time::sleep(Duration::from_secs(wait_seconds)).await;
    }

    let form = [
        ("op", "download2".to_string()),
        ("id", page.code.clone()),
        ("rand", page.rand.clone()),
        ("referer", page.referer.clone()),
        ("method_free", page.free_method.clone()),
        ("method_premium", page.premium_method.clone()),
        ("g_captch__a", "1".to_string()),
    ];
    let response = client
        .post(DATANODES_DOWNLOAD_URL)
        .header(COOKIE, cookie)
        .header(REFERER, page.referer.as_str())
        .form(&form)
        .send()
        .await
        .map_err(|error| {
            reqwest_resolution_error("Could not request DataNodes standard download", error)
        })?;

    if !response.status().is_success() {
        return Err(http_status_resolution_error(
            "Could not request DataNodes standard download",
            response.status(),
        ));
    }

    let json = response.text().await.map_err(|error| {
        reqwest_resolution_error("Could not read DataNodes standard download response", error)
    })?;
    let direct_url = extract_datanodes_direct_url_from_json(&json)?;

    Ok(ResolvedHosterLink {
        resolved_from_url: original_url_for_resolved_link(original_url, &direct_url),
        url: direct_url,
        filename_hint: datanodes_filename_hint_from_url(original_url).or(page.filename_hint),
    })
}

async fn preflight_datanodes_source(
    client: &Client,
    original_url: &str,
) -> Result<HosterSourcePreflight, HosterResolutionError> {
    let file_code = datanodes_file_code_from_url(original_url).ok_or_else(|| {
        resolution_error("DataNodes URL does not contain a supported file code.".into())
    })?;
    let cookie = format!("file_code={file_code}");
    let response = client
        .get(DATANODES_DOWNLOAD_URL)
        .header(COOKIE, cookie.clone())
        .send()
        .await
        .map_err(|error| reqwest_resolution_error("Could not load DataNodes page", error))?;

    if !response.status().is_success() {
        return Err(http_status_resolution_error(
            "Could not load DataNodes page",
            response.status(),
        ));
    }

    let html = response
        .text()
        .await
        .map_err(|error| reqwest_resolution_error("Could not read DataNodes page", error))?;
    let mut preflight =
        preflight_datanodes_source_from_html(html, &file_code, {
            let client = client.clone();
            let cookie = cookie.clone();
            move |preliminary| {
                let client = client.clone();
                let cookie = cookie.clone();
                async move {
                    request_datanodes_preliminary_download(&client, &cookie, preliminary).await
                }
            }
        })
        .await?;
    preflight.filename_hint =
        datanodes_filename_hint_from_url(original_url).or(preflight.filename_hint);
    Ok(preflight)
}

async fn preflight_datanodes_source_from_html<F, Fut>(
    html: String,
    file_code: &str,
    request_preliminary: F,
) -> Result<HosterSourcePreflight, HosterResolutionError>
where
    F: FnMut(DatanodesPreliminaryDownloadPage) -> Fut,
    Fut: Future<Output = Result<String, HosterResolutionError>>,
{
    let page =
        resolve_datanodes_download_page_from_html(html, file_code, request_preliminary).await?;
    Ok(HosterSourcePreflight {
        filename_hint: page.filename_hint,
        resolved_url: None,
    })
}

async fn resolve_datanodes_download_page_from_html<F, Fut>(
    mut html: String,
    file_code: &str,
    mut request_preliminary: F,
) -> Result<DatanodesDownloadPage, HosterResolutionError>
where
    F: FnMut(DatanodesPreliminaryDownloadPage) -> Fut,
    Fut: Future<Output = Result<String, HosterResolutionError>>,
{
    let mut preliminary_steps = 0;
    let mut seen_preliminary_pages = HashSet::new();
    loop {
        if extract_html_tag(&html, "download-countdown").is_some() {
            let page = parse_datanodes_standard_download_page(&html)?;
            validate_datanodes_standard_page_code(&page, file_code)?;
            return Ok(page);
        }

        if preliminary_steps >= DATANODES_MAX_PRELIMINARY_STEPS {
            return Err(resolution_error(
                "DataNodes preliminary download flow did not reach the standard download form."
                    .into(),
            ));
        }

        let preliminary = parse_datanodes_preliminary_download_page(&html, file_code)?;
        let signature = datanodes_preliminary_signature(&preliminary);
        if !seen_preliminary_pages.insert(signature) {
            return Err(resolution_error(
                "DataNodes returned a repeated preliminary download page before exposing the standard download form.".into(),
            ));
        }
        html = request_preliminary(preliminary).await?;
        preliminary_steps += 1;
    }
}

fn datanodes_preliminary_signature(page: &DatanodesPreliminaryDownloadPage) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        page.action_url, page.op, page.id, page.fname, page.method_free
    )
}

async fn request_datanodes_preliminary_download(
    client: &Client,
    cookie: &str,
    preliminary: DatanodesPreliminaryDownloadPage,
) -> Result<String, HosterResolutionError> {
    let form = [
        ("op", preliminary.op),
        ("usr_login", preliminary.usr_login),
        ("id", preliminary.id),
        ("fname", preliminary.fname),
        ("referer", preliminary.referer),
        ("method_free", preliminary.method_free),
    ];
    let response = client
        .post(preliminary.action_url)
        .header(COOKIE, cookie)
        .header(REFERER, DATANODES_DOWNLOAD_URL)
        .form(&form)
        .send()
        .await
        .map_err(|error| {
            reqwest_resolution_error("Could not request DataNodes preliminary download", error)
        })?;

    if !response.status().is_success() {
        return Err(http_status_resolution_error(
            "Could not request DataNodes preliminary download",
            response.status(),
        ));
    }

    response.text().await.map_err(|error| {
        reqwest_resolution_error(
            "Could not read DataNodes preliminary download response",
            error,
        )
    })
}

#[cfg(test)]
fn ordered_resolved_hoster_links(
    indexed: Vec<IndexedResolvedHosterLink>,
    expected_len: usize,
) -> Result<Vec<ResolvedHosterLink>, HosterResolutionError> {
    let batch = ordered_resolved_hoster_batch(indexed, expected_len)?;
    if let Some(failed) = batch.failed_links.into_iter().next() {
        return Err(resolution_error(failed.message));
    }
    Ok(batch.links)
}

fn ordered_resolved_hoster_batch(
    mut indexed: Vec<IndexedResolvedHosterLink>,
    expected_len: usize,
) -> Result<HosterResolutionBatch, HosterResolutionError> {
    indexed.sort_by_key(|result| match result {
        Ok((index, _)) | Err((index, _)) => *index,
    });

    if indexed.len() != expected_len {
        return Err(resolution_error(
            "Hoster resolver returned incomplete results.".into(),
        ));
    }

    let mut links = Vec::with_capacity(expected_len);
    let mut failed_links = Vec::new();
    for (expected_index, result) in indexed.into_iter().enumerate() {
        match result {
            Ok((index, link)) if index == expected_index => links.push(link),
            Err((index, failed)) if index == expected_index => failed_links.push(failed),
            Ok(_) | Err(_) => {
                return Err(resolution_error(
                    "Hoster resolver returned results with inconsistent indexes.".into(),
                ));
            }
        }
    }

    Ok(HosterResolutionBatch {
        links,
        failed_links,
    })
}

pub fn resolve_hoster_link_from_html(
    original_url: &str,
    html: &str,
) -> Result<ResolvedHosterLink, HosterResolutionError> {
    if !is_fuckingfast_page_url(original_url) {
        return Ok(ResolvedHosterLink {
            url: original_url.trim().to_string(),
            filename_hint: None,
            resolved_from_url: None,
        });
    }

    let direct_url = extract_fuckingfast_direct_url(html)?;
    Ok(ResolvedHosterLink {
        resolved_from_url: original_url_for_resolved_link(original_url, &direct_url),
        url: direct_url,
        filename_hint: filename_hint_from_fragment(original_url)
            .or_else(|| filename_hint_from_meta_title(html))
            .or_else(|| filename_hint_from_title(html)),
    })
}

fn original_url_for_resolved_link(original_url: &str, resolved_url: &str) -> Option<String> {
    let trimmed = original_url.trim();
    (!trimmed.is_empty() && trimmed != resolved_url).then(|| trimmed.to_string())
}

pub fn is_fuckingfast_page_url(raw_url: &str) -> bool {
    let Ok(parsed) = Url::parse(raw_url.trim()) else {
        return false;
    };

    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }

    matches!(
        parsed.host_str().map(str::to_ascii_lowercase).as_deref(),
        Some(FUCKINGFAST_HOST) | Some("www.fuckingfast.co")
    )
}

pub fn is_datanodes_page_url(raw_url: &str) -> bool {
    datanodes_file_code_from_url(raw_url).is_some()
}

pub fn hoster_download_context_for_resolved_url(
    resolved_url: &str,
    source_url: Option<&str>,
) -> Option<HosterDownloadContext> {
    if matches!(
        datanodes_direct_url_kind(resolved_url),
        Ok(DatanodesDirectUrlKind::Native)
    ) {
        return datanodes_download_context_for_source_url(source_url?);
    }

    None
}

pub fn hoster_acceleration_policy(
    source_url: &str,
    resolved_url: &str,
) -> Option<HosterAccelerationPolicy> {
    match hoster_kind_for_url(source_url)? {
        HosterKind::Datanodes => datanodes_hoster_acceleration_policy(source_url, resolved_url),
        HosterKind::FuckingFast => fuckingfast_hoster_acceleration_policy(source_url, resolved_url),
    }
}

fn datanodes_hoster_acceleration_policy(
    source_url: &str,
    resolved_url: &str,
) -> Option<HosterAccelerationPolicy> {
    datanodes_direct_url_kind(resolved_url).ok()?;
    let file_code = datanodes_file_code_from_url(source_url)?;
    Some(HosterAccelerationPolicy {
        backoff_key: format!("hoster:datanodes:{file_code}"),
        max_balanced_segments: 4,
        max_fast_segments: 6,
    })
}

fn fuckingfast_hoster_acceleration_policy(
    source_url: &str,
    resolved_url: &str,
) -> Option<HosterAccelerationPolicy> {
    if !validate_fuckingfast_direct_url(resolved_url) {
        return None;
    }
    let source_id = fuckingfast_source_id(source_url)?;
    Some(HosterAccelerationPolicy {
        backoff_key: format!("hoster:fuckingfast:{source_id}"),
        max_balanced_segments: 4,
        max_fast_segments: 6,
    })
}

fn fuckingfast_source_id(raw_url: &str) -> Option<String> {
    let parsed = Url::parse(raw_url.trim()).ok()?;
    parsed
        .path_segments()?
        .find(|segment| !segment.trim().is_empty())
        .map(str::to_string)
}

fn datanodes_download_context_for_source_url(source_url: &str) -> Option<HosterDownloadContext> {
    let file_code = datanodes_file_code_from_url(source_url)?;
    Some(HandoffAuth {
        headers: vec![
            HandoffAuthHeader {
                name: "Cookie".into(),
                value: format!("file_code={file_code}"),
            },
            HandoffAuthHeader {
                name: "Referer".into(),
                value: DATANODES_DOWNLOAD_URL.into(),
            },
            HandoffAuthHeader {
                name: "User-Agent".into(),
                value: "SimpleDownloadManager/0.5".into(),
            },
        ],
    })
}

fn hoster_kind_for_url(raw_url: &str) -> Option<HosterKind> {
    if is_fuckingfast_page_url(raw_url) {
        return Some(HosterKind::FuckingFast);
    }

    if is_datanodes_page_url(raw_url) {
        return Some(HosterKind::Datanodes);
    }

    None
}

fn datanodes_file_code_from_url(raw_url: &str) -> Option<String> {
    let parsed = Url::parse(raw_url.trim()).ok()?;

    if !matches!(parsed.scheme(), "http" | "https") || !is_datanodes_host(parsed.host_str()) {
        return None;
    }

    let segments = parsed
        .path_segments()?
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>();
    let code = segments.first()?.trim();
    is_datanodes_file_code(code).then(|| code.to_string())
}

fn datanodes_filename_hint_from_url(raw_url: &str) -> Option<String> {
    let parsed = Url::parse(raw_url.trim()).ok()?;
    let segments = parsed
        .path_segments()?
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>();
    if segments.len() < 2 || !is_datanodes_file_code(segments[0]) {
        return None;
    }

    non_empty_filename_hint(&decode_html_entities(
        &percent_decode_str(segments.last()?).decode_utf8_lossy(),
    ))
}

fn is_datanodes_host(host: Option<&str>) -> bool {
    matches!(
        host.map(str::to_ascii_lowercase).as_deref(),
        Some(DATANODES_HOST) | Some("www.datanodes.to")
    )
}

fn is_datanodes_file_code(value: &str) -> bool {
    if !(6..=32).contains(&value.len()) {
        return false;
    }

    if matches!(
        value.to_ascii_lowercase().as_str(),
        "download"
            | "pages"
            | "api"
            | "check_files"
            | "premium"
            | "login"
            | "register"
            | "contact"
            | "links"
            | "account"
            | "images"
            | "theme_2023"
            | "cdn-cgi"
    ) {
        return false;
    }

    value
        .chars()
        .all(|character| character.is_ascii_alphanumeric())
}

fn extract_fuckingfast_direct_url(html: &str) -> Result<String, HosterResolutionError> {
    let candidates = extract_window_open_literal_urls(html);
    let mut saw_invalid_window_open_candidate = false;
    for candidate in candidates {
        if validate_fuckingfast_direct_url(&candidate) {
            return Ok(candidate);
        }

        saw_invalid_window_open_candidate = true;
    }

    if let Some(candidate) = extract_any_fuckingfast_direct_url(html) {
        if validate_fuckingfast_direct_url(&candidate) {
            return Ok(candidate);
        }
    }

    if saw_invalid_window_open_candidate {
        return Err(resolution_error(
            "FuckingFast page pointed at an unexpected download host.".into(),
        ));
    }

    Err(resolution_error(
        "Could not find a direct FuckingFast download link on the page.".into(),
    ))
}

fn extract_window_open_literal_urls(html: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut search_from = 0;
    while let Some(relative_index) = html[search_from..].find("window.open(") {
        let open_start = search_from + relative_index + "window.open(".len();
        let rest = html[open_start..].trim_start();
        let Some(quote) = rest
            .chars()
            .next()
            .filter(|quote| *quote == '"' || *quote == '\'')
        else {
            search_from = open_start;
            continue;
        };
        let value_start =
            open_start + html[open_start..].find(quote).unwrap_or_default() + quote.len_utf8();
        if let Some(value_end_relative) = html[value_start..].find(quote) {
            let value = decode_javascript_string_literal(
                &html[value_start..value_start + value_end_relative],
            );
            if value.starts_with("http://") || value.starts_with("https://") {
                urls.push(value);
            }
            search_from = value_start + value_end_relative + quote.len_utf8();
        } else {
            break;
        }
    }
    urls
}

fn extract_any_fuckingfast_direct_url(html: &str) -> Option<String> {
    for prefix in [
        "https://dl.fuckingfast.co/dl/",
        "http://dl.fuckingfast.co/dl/",
    ] {
        let Some(start) = html.find(prefix) else {
            continue;
        };
        let candidate = html[start..]
            .chars()
            .take_while(|character| {
                !matches!(
                    character,
                    '"' | '\'' | '`' | '<' | '>' | ')' | '(' | ' ' | '\r' | '\n' | '\t'
                )
            })
            .collect::<String>();
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    None
}

fn parse_datanodes_standard_download_page(
    html: &str,
) -> Result<DatanodesDownloadPage, HosterResolutionError> {
    let tag = extract_html_tag(html, "download-countdown").ok_or_else(|| {
        resolution_error("Could not find the DataNodes standard download form.".into())
    })?;
    let code = datanodes_attribute_value(tag, &["code"]).ok_or_else(|| {
        resolution_error("DataNodes standard download form is missing the file code.".into())
    })?;
    let free_method = datanodes_attribute_value(tag, &["free-method"]).ok_or_else(|| {
        resolution_error(
            "DataNodes standard download form is missing the free download method.".into(),
        )
    })?;
    if free_method.trim().is_empty() {
        return Err(resolution_error(
            "DataNodes page did not expose a standard free download method.".into(),
        ));
    }

    if datanodes_bool_attribute(tag, &[":has-password", "has-password"]) {
        return Err(resolution_error(
            "DataNodes password-protected downloads are not supported.".into(),
        ));
    }

    if datanodes_bool_attribute(tag, &[":has-captcha", "has-captcha"]) {
        return Err(resolution_error(
            "DataNodes captcha-protected downloads are not supported.".into(),
        ));
    }

    let countdown_secs = datanodes_attribute_value(tag, &[":countdown", "countdown"])
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let referer = datanodes_attribute_value(tag, &["referer"])
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DATANODES_DOWNLOAD_URL.to_string());
    let rand = datanodes_attribute_value(tag, &["rand"]).unwrap_or_default();
    let premium_method = datanodes_attribute_value(tag, &["premium-method"]).unwrap_or_default();
    let filename_hint = datanodes_attribute_value(tag, &["name"])
        .and_then(|value| non_empty_filename_hint(&decode_html_entities(&value)))
        .or_else(|| datanodes_filename_hint_from_scan_card(html));

    Ok(DatanodesDownloadPage {
        code,
        referer,
        rand,
        free_method,
        premium_method,
        countdown_secs,
        filename_hint,
    })
}

fn parse_datanodes_preliminary_download_page(
    html: &str,
    expected_file_code: &str,
) -> Result<DatanodesPreliminaryDownloadPage, HosterResolutionError> {
    let form = extract_datanodes_download_form(html).ok_or_else(|| {
        resolution_error("Could not find the DataNodes preliminary download form.".into())
    })?;
    let opening_tag = extract_opening_tag(form, "form").ok_or_else(|| {
        resolution_error("DataNodes preliminary download form is malformed.".into())
    })?;
    let action_url = resolve_datanodes_form_action(
        &extract_attribute_value(opening_tag, "action").unwrap_or_default(),
    )?;
    let op = extract_named_form_value(form, "op").ok_or_else(|| {
        resolution_error("DataNodes preliminary download form is missing the operation.".into())
    })?;
    if op.trim() != "download1" {
        return Err(resolution_error(
            "DataNodes preliminary download form has an unexpected operation.".into(),
        ));
    }

    let id = extract_named_form_value(form, "id").ok_or_else(|| {
        resolution_error("DataNodes preliminary download form is missing the file code.".into())
    })?;
    if id != expected_file_code {
        return Err(resolution_error(
            "DataNodes preliminary page returned a different file code than the requested link."
                .into(),
        ));
    }

    let fname = extract_named_form_value(form, "fname").ok_or_else(|| {
        resolution_error("DataNodes preliminary download form is missing the filename.".into())
    })?;
    let method_free = extract_datanodes_preliminary_free_method(form).ok_or_else(|| {
        resolution_error(
            "DataNodes preliminary download form is missing the free download method.".into(),
        )
    })?;
    if method_free.trim().is_empty() {
        return Err(resolution_error(
            "DataNodes preliminary page did not expose a standard free download method.".into(),
        ));
    }

    Ok(DatanodesPreliminaryDownloadPage {
        action_url,
        op,
        usr_login: extract_named_form_value(form, "usr_login").unwrap_or_default(),
        id,
        fname,
        referer: extract_named_form_value(form, "referer").unwrap_or_default(),
        method_free,
    })
}

fn validate_datanodes_standard_page_code(
    page: &DatanodesDownloadPage,
    expected_file_code: &str,
) -> Result<(), HosterResolutionError> {
    if page.code == expected_file_code {
        return Ok(());
    }

    Err(resolution_error(
        "DataNodes page returned a different file code than the requested link.".into(),
    ))
}

fn extract_datanodes_download_form(html: &str) -> Option<&str> {
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(relative_index) = lower[search_from..].find("<form") {
        let start = search_from + relative_index;
        let end = lower[start..].find("</form>")? + start + "</form>".len();
        let form = &html[start..end];
        let opening_tag = extract_opening_tag(form, "form")?;
        if extract_attribute_value(opening_tag, "id")
            .as_deref()
            .is_some_and(|id| id == "downloadForm")
        {
            return Some(form);
        }
        search_from = end;
    }

    None
}

fn extract_opening_tag<'a>(html: &'a str, tag_name: &str) -> Option<&'a str> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{}", tag_name.to_ascii_lowercase());
    let start = lower.find(&open)?;
    let end = lower[start..].find('>')? + start + 1;
    Some(&html[start..end])
}

fn extract_named_form_value(form: &str, name: &str) -> Option<String> {
    extract_named_tag_attribute(form, "input", name, "value")
        .or_else(|| extract_named_tag_attribute(form, "button", name, "value"))
        .map(|value| decode_html_entities(&value))
}

fn extract_datanodes_preliminary_free_method(form: &str) -> Option<String> {
    extract_named_form_value(form, "method_free")
        .and_then(|value| non_empty_filename_hint(&value))
        .or_else(|| extract_datanodes_actionable_free_method(form))
}

fn extract_datanodes_actionable_free_method(form: &str) -> Option<String> {
    ["input", "button", "a", "label", "div"]
        .iter()
        .find_map(|tag_name| extract_datanodes_free_method_from_elements(form, tag_name))
}

fn extract_datanodes_free_method_from_elements(html: &str, tag_name: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{}", tag_name.to_ascii_lowercase());
    let close = format!("</{}>", tag_name.to_ascii_lowercase());
    let mut search_from = 0;
    while let Some(relative_index) = lower[search_from..].find(&open) {
        let start = search_from + relative_index;
        let tag_end = lower[start..].find('>')? + start + 1;
        let opening_tag = &html[start..tag_end];
        let inner_html = if tag_name.eq_ignore_ascii_case("input") {
            ""
        } else {
            lower[tag_end..]
                .find(&close)
                .map(|end| &html[tag_end..tag_end + end])
                .unwrap_or_default()
        };

        if datanodes_element_is_actionable_free_method(tag_name, opening_tag, inner_html) {
            return extract_attribute_value(opening_tag, "value")
                .map(|value| decode_html_entities(&value))
                .and_then(|value| datanodes_signal_mentions_free_download(&value).then_some(value))
                .or_else(|| Some(DATANODES_FREE_METHOD.to_string()));
        }

        search_from = tag_end;
    }

    None
}

fn datanodes_element_is_actionable_free_method(
    tag_name: &str,
    opening_tag: &str,
    inner_html: &str,
) -> bool {
    let tag_signal = decode_html_entities(opening_tag).to_ascii_lowercase();
    let text_signal = html_fragment_text(inner_html).to_ascii_lowercase();
    let combined_signal = format!("{tag_signal} {text_signal}");
    if !datanodes_signal_mentions_free_download(&combined_signal) {
        return false;
    }

    match tag_name.to_ascii_lowercase().as_str() {
        "button" => true,
        "input" => {
            !datanodes_attribute_equals(&tag_signal, "type", "hidden")
                && datanodes_signal_mentions_free_download(&tag_signal)
        }
        "a" => {
            tag_signal.contains("href=")
                || tag_signal.contains("onclick")
                || datanodes_attribute_equals(&tag_signal, "role", "button")
                || datanodes_opening_tag_has_free_marker(&tag_signal)
        }
        "label" | "div" => {
            datanodes_opening_tag_has_free_marker(&tag_signal)
                || datanodes_attribute_equals(&tag_signal, "role", "button")
                || tag_signal.contains("onclick")
        }
        _ => false,
    }
}

fn datanodes_signal_mentions_free_download(signal: &str) -> bool {
    let normalized = signal.replace(['_', '-'], " ");
    signal.contains("method_free")
        || signal.contains("method-free")
        || signal.contains("free_download")
        || signal.contains("free-download")
        || normalized.contains("free download")
}

fn datanodes_opening_tag_has_free_marker(tag_signal: &str) -> bool {
    datanodes_signal_mentions_free_download(tag_signal)
        || tag_signal
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .any(|part| part == "free" || part == "method_free")
}

fn datanodes_attribute_equals(tag_signal: &str, attribute: &str, expected_value: &str) -> bool {
    extract_attribute_value(tag_signal, attribute)
        .map(|value| value.eq_ignore_ascii_case(expected_value))
        .unwrap_or(false)
}

fn extract_named_tag_attribute(
    html: &str,
    tag_name: &str,
    expected_name: &str,
    attribute: &str,
) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{}", tag_name.to_ascii_lowercase());
    let mut search_from = 0;
    while let Some(relative_index) = lower[search_from..].find(&open) {
        let start = search_from + relative_index;
        let end = lower[start..].find('>')? + start + 1;
        let tag = &html[start..end];
        if extract_attribute_value(tag, "name")
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(expected_name))
        {
            return extract_attribute_value(tag, attribute);
        }
        search_from = end;
    }

    None
}

fn resolve_datanodes_form_action(action: &str) -> Result<String, HosterResolutionError> {
    let base = Url::parse(DATANODES_DOWNLOAD_URL)
        .map_err(|_| resolution_error("DataNodes download URL is invalid.".into()))?;
    let resolved = if action.trim().is_empty() {
        base
    } else {
        base.join(action.trim()).map_err(|_| {
            resolution_error("DataNodes preliminary download form action is invalid.".into())
        })?
    };

    if !matches!(resolved.scheme(), "http" | "https") || !is_datanodes_host(resolved.host_str()) {
        return Err(resolution_error(
            "DataNodes preliminary download form pointed at an unexpected host.".into(),
        ));
    }

    Ok(resolved.to_string())
}

fn extract_html_tag<'a>(html: &'a str, tag_name: &str) -> Option<&'a str> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{}", tag_name.to_ascii_lowercase());
    let start = lower.find(&open)?;
    let end = lower[start..].find('>')? + start + 1;
    Some(&html[start..end])
}

fn datanodes_attribute_value(tag: &str, attributes: &[&str]) -> Option<String> {
    attributes.iter().find_map(|attribute| {
        extract_attribute_value(tag, attribute).map(|value| decode_html_entities(&value))
    })
}

fn datanodes_bool_attribute(tag: &str, attributes: &[&str]) -> bool {
    datanodes_attribute_value(tag, attributes)
        .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "1"))
        .unwrap_or(false)
}

fn datanodes_filename_hint_from_scan_card(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let attribute_index = lower.find("data-scan-file")?;
    let tag_start = lower[..attribute_index].rfind('<')?;
    let tag_end = lower[attribute_index..].find('>')? + attribute_index + 1;
    extract_attribute_value(&html[tag_start..tag_end], "data-scan-file")
        .and_then(|value| non_empty_filename_hint(&decode_html_entities(&value)))
}

fn extract_datanodes_direct_url_from_json(json: &str) -> Result<String, HosterResolutionError> {
    let response = serde_json::from_str::<DatanodesDirectLinkResponse>(json).map_err(|error| {
        resolution_error(format!(
            "Could not parse DataNodes standard download response: {error}"
        ))
    })?;

    if let Some(error) = response
        .error
        .and_then(|value| non_empty_filename_hint(&value))
    {
        return Err(resolution_error(format!(
            "DataNodes rejected the standard download request: {error}"
        )));
    }

    let encoded_url = response.url.ok_or_else(|| {
        resolution_error(
            "DataNodes standard download response did not include a direct URL.".into(),
        )
    })?;
    let direct_url = decode_html_entities(&percent_decode_str(&encoded_url).decode_utf8_lossy());
    if let Some(reason) = datanodes_direct_url_rejection_reason(&direct_url) {
        Err(resolution_error(
            format!(
                "DataNodes standard download response pointed at an unexpected download host: {reason}."
            ),
        ))
    } else {
        Ok(direct_url)
    }
}

#[cfg(test)]
fn validate_datanodes_direct_url(raw_url: &str) -> bool {
    datanodes_direct_url_rejection_reason(raw_url).is_none()
}

fn datanodes_direct_url_rejection_reason(raw_url: &str) -> Option<String> {
    datanodes_direct_url_kind(raw_url).err()
}

fn datanodes_direct_url_kind(raw_url: &str) -> Result<DatanodesDirectUrlKind, String> {
    let Ok(parsed) = Url::parse(raw_url) else {
        return Err("URL could not be parsed".into());
    };
    if parsed.scheme() != "https" {
        return Err(format!("scheme `{}` is not https", parsed.scheme()));
    }

    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return Err("URL did not include a host".into());
    };

    let kind = if is_datanodes_direct_file_host(&host) {
        DatanodesDirectUrlKind::Native
    } else if is_datanodes_proxy_file_host(&host) {
        DatanodesDirectUrlKind::Proxy
    } else {
        return Err(format!(
            "host `{host}` is not a supported DataNodes file server"
        ));
    };

    let path = parsed.path();
    let required_path_prefix = match kind {
        DatanodesDirectUrlKind::Native => "/d/",
        DatanodesDirectUrlKind::Proxy => "/download/",
    };
    if !path.starts_with(required_path_prefix) || path.len() <= required_path_prefix.len() {
        return Err(format!(
            "path `{path}` is not a DataNodes file download path"
        ));
    }

    Ok(kind)
}

fn is_datanodes_direct_file_host(host: &str) -> bool {
    is_valid_datanodes_file_subdomain(host, DATANODES_DIRECT_SUFFIX)
}

fn is_datanodes_proxy_file_host(host: &str) -> bool {
    is_valid_datanodes_file_subdomain(host, DATANODES_PROXY_SUFFIX)
}

fn is_valid_datanodes_file_subdomain(host: &str, suffix: &str) -> bool {
    let Some(subdomain) = host.strip_suffix(suffix) else {
        return false;
    };
    if subdomain.is_empty() || subdomain == "www" {
        return false;
    }

    subdomain.split('.').all(is_valid_datanodes_subdomain_label)
}

fn is_valid_datanodes_subdomain_label(label: &str) -> bool {
    if label.is_empty() || label.len() > 63 {
        return false;
    }

    let first = label.as_bytes()[0];
    let last = label.as_bytes()[label.len() - 1];
    first.is_ascii_alphanumeric()
        && last.is_ascii_alphanumeric()
        && label
            .bytes()
            .all(|character| character.is_ascii_alphanumeric() || character == b'-')
}

fn validate_fuckingfast_direct_url(raw_url: &str) -> bool {
    let Ok(parsed) = Url::parse(raw_url) else {
        return false;
    };

    matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().map(str::to_ascii_lowercase).as_deref()
            == Some(FUCKINGFAST_DIRECT_HOST)
        && parsed.path().starts_with("/dl/")
        && parsed.path().len() > "/dl/".len()
}

fn filename_hint_from_fragment(raw_url: &str) -> Option<String> {
    let parsed = Url::parse(raw_url.trim()).ok()?;
    let fragment = parsed.fragment()?.trim();
    non_empty_filename_hint(&decode_html_entities(
        &percent_decode_str(fragment).decode_utf8_lossy(),
    ))
}

fn filename_hint_from_meta_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(relative_index) = lower[search_from..].find("<meta") {
        let start = search_from + relative_index;
        let Some(end_relative) = lower[start..].find('>') else {
            break;
        };
        let end = start + end_relative + 1;
        let tag = &html[start..end];
        let tag_lower = &lower[start..end];
        if has_title_name_attribute(tag_lower) {
            if let Some(content) = extract_attribute_value(tag, "content") {
                return non_empty_filename_hint(&decode_html_entities(&content));
            }
        }
        search_from = end;
    }

    None
}

fn filename_hint_from_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = lower[start..].find("</title>")? + start;
    non_empty_filename_hint(&decode_html_entities(&html[start..end]))
}

fn html_fragment_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }

    decode_html_entities(&text)
}

fn has_title_name_attribute(tag_lower: &str) -> bool {
    tag_lower.contains("name=\"title\"")
        || tag_lower.contains("name='title'")
        || tag_lower.contains("name=title")
}

fn extract_attribute_value(tag: &str, attribute: &str) -> Option<String> {
    let tag_lower = tag.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(relative_index) = tag_lower[search_from..].find(attribute) {
        let attr_start = search_from + relative_index;
        let after_name = attr_start + attribute.len();
        let rest = tag[after_name..].trim_start();
        if !rest.starts_with('=') {
            search_from = after_name;
            continue;
        }

        let rest = rest[1..].trim_start();
        let quote = rest.chars().next()?;
        if quote == '"' || quote == '\'' {
            let value_start = quote.len_utf8();
            let value_end = rest[value_start..].find(quote)? + value_start;
            return Some(rest[value_start..value_end].to_string());
        }

        let value = rest
            .chars()
            .take_while(|character| {
                !character.is_whitespace() && *character != '>' && *character != '/'
            })
            .collect::<String>();
        return Some(value);
    }

    None
}

fn non_empty_filename_hint(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn decode_javascript_string_literal(value: &str) -> String {
    value
        .replace("\\/", "/")
        .replace("\\\"", "\"")
        .replace("\\'", "'")
        .replace("\\u002f", "/")
        .replace("\\u002F", "/")
}

fn decode_html_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn resolution_error(message: String) -> HosterResolutionError {
    HosterResolutionError {
        code: "HOSTER_RESOLUTION_FAILED",
        message,
        retryable: false,
    }
}

fn transient_resolution_error(message: String) -> HosterResolutionError {
    HosterResolutionError {
        code: "HOSTER_RESOLUTION_FAILED",
        message,
        retryable: true,
    }
}

fn reqwest_resolution_error(context: &str, error: reqwest::Error) -> HosterResolutionError {
    if is_retryable_reqwest_error(&error) {
        transient_resolution_error(format!("{context}: {error}"))
    } else {
        resolution_error(format!("{context}: {error}"))
    }
}

fn is_retryable_reqwest_error(error: &reqwest::Error) -> bool {
    error.is_timeout()
        || error.is_connect()
        || error.is_request()
        || error.is_body()
        || error.is_decode()
}

fn http_status_resolution_error(context: &str, status: StatusCode) -> HosterResolutionError {
    let message = format!("{context}: HTTP {status}.");
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        transient_resolution_error(message)
    } else {
        resolution_error(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuckingfast_resolver_extracts_direct_link_and_fragment_filename() {
        let original_url = "https://fuckingfast.co/ecw0lw398okf#archive.part01.rar";
        let html = r#"
            <html>
              <head><title>Ignored title</title></head>
              <body>
                <script>
                  function download() {
                    window.open("https://dl.fuckingfast.co/dl/direct-token_123")
                  }
                </script>
              </body>
            </html>
        "#;

        let resolved = resolve_hoster_link_from_html(original_url, html)
            .expect("fuckingfast page should resolve");

        assert_eq!(
            resolved.url,
            "https://dl.fuckingfast.co/dl/direct-token_123"
        );
        assert_eq!(
            resolved.filename_hint.as_deref(),
            Some("archive.part01.rar")
        );
        assert_eq!(resolved.resolved_from_url.as_deref(), Some(original_url));
    }

    #[test]
    fn fuckingfast_resolver_uses_page_title_when_fragment_is_missing() {
        let html = r#"
            <html>
              <head>
                <meta name="title" content="I_Am_Jesus_Christ.part02.rar">
              </head>
              <body>
                <script>window.open("https://dl.fuckingfast.co/dl/direct-token_456")</script>
              </body>
            </html>
        "#;

        let resolved = resolve_hoster_link_from_html("https://fuckingfast.co/ecw0lw398okf", html)
            .expect("fuckingfast page should resolve");

        assert_eq!(
            resolved.url,
            "https://dl.fuckingfast.co/dl/direct-token_456"
        );
        assert_eq!(
            resolved.filename_hint.as_deref(),
            Some("I_Am_Jesus_Christ.part02.rar")
        );
    }

    #[test]
    fn fuckingfast_source_preflight_validates_without_returning_direct_url() {
        let original_url = "https://fuckingfast.co/ecw0lw398okf#archive.part01.rar";
        let html = r#"
            <html>
              <head><title>Ignored title</title></head>
              <body>
                <script>window.open("https://dl.fuckingfast.co/dl/direct-token_123")</script>
              </body>
            </html>
        "#;

        let preflight = preflight_fuckingfast_source_from_html(original_url, html)
            .expect("source-only FuckingFast preflight should validate supported pages");

        assert_eq!(
            preflight.filename_hint.as_deref(),
            Some("archive.part01.rar")
        );
        assert_eq!(preflight.resolved_url, None);
    }

    #[test]
    fn hoster_resolver_leaves_unsupported_hosts_unchanged() {
        let resolved =
            resolve_hoster_link_from_html("https://example.com/file.zip", "<html></html>")
                .expect("unsupported hosts should pass through");

        assert_eq!(resolved.url, "https://example.com/file.zip");
        assert_eq!(resolved.filename_hint, None);
        assert_eq!(resolved.resolved_from_url, None);
    }

    #[test]
    fn datanodes_resolver_identifies_public_file_urls_only() {
        assert!(is_datanodes_page_url(
            "https://datanodes.to/61nni6me5p0n/Neon-White.rar"
        ));
        assert!(is_datanodes_page_url(
            "https://www.datanodes.to/61nni6me5p0n"
        ));

        for unsupported in [
            "https://datanodes.to/download",
            "https://datanodes.to/pages/api",
            "https://datanodes.to/api/file/info",
            "https://datanodes.to/check_files",
            "https://example.com/61nni6me5p0n/Neon-White.rar",
        ] {
            assert!(
                !is_datanodes_page_url(unsupported),
                "{unsupported} should not be treated as a DataNodes file URL"
            );
        }
    }

    #[test]
    fn datanodes_resolver_extracts_file_code_and_filename_hint_from_url() {
        assert_eq!(
            datanodes_file_code_from_url("https://datanodes.to/61nni6me5p0n/Neon-White.rar")
                .as_deref(),
            Some("61nni6me5p0n")
        );
        assert_eq!(
            datanodes_filename_hint_from_url("https://datanodes.to/61nni6me5p0n/Neon%20White.rar")
                .as_deref(),
            Some("Neon White.rar")
        );
        assert_eq!(
            datanodes_filename_hint_from_url("https://datanodes.to/61nni6me5p0n"),
            None
        );
    }

    #[test]
    fn datanodes_resolver_parses_standard_download_page_form() {
        let html = r#"
            <download-countdown :countdown="5"
                code="61nni6me5p0n" referer="https://datanodes.to/download" rand="rand-token"
                free-method="Free Download &gt;&gt;" premium-method=""
                :has-password="false" :has-captcha="false"
                :has-countdown="true"
                name="Neon-White.rar"></download-countdown>
        "#;

        let page = parse_datanodes_standard_download_page(html)
            .expect("standard DataNodes page should parse");

        assert_eq!(page.code, "61nni6me5p0n");
        assert_eq!(page.countdown_secs, 5);
        assert_eq!(page.referer, "https://datanodes.to/download");
        assert_eq!(page.rand, "rand-token");
        assert_eq!(page.free_method, "Free Download >>");
        assert_eq!(page.premium_method, "");
        assert_eq!(page.filename_hint.as_deref(), Some("Neon-White.rar"));
    }

    #[tokio::test]
    async fn datanodes_source_preflight_stops_before_direct_link_request() {
        let preliminary_html = r#"
            <form method="POST" action="/download" id="downloadForm">
                <input type="hidden" name="op" value="download1">
                <input type="hidden" name="id" value="61nni6me5p0n">
                <input type="hidden" name="fname" value="Neon White.rar">
                <input type="hidden" name="referer" value="">
                <button type="submit" name="method_free" value="Free Download &gt;&gt;">Continue</button>
            </form>
        "#;
        let standard_html = r#"
            <download-countdown :countdown="45"
                code="61nni6me5p0n" referer="https://datanodes.to/download" rand="rand-token"
                free-method="Free Download &gt;&gt;" premium-method=""
                :has-password="false" :has-captcha="false"
                :has-countdown="true"
                name="Neon White.rar"></download-countdown>
        "#;
        let submitted_methods = std::cell::RefCell::new(Vec::new());

        let preflight = preflight_datanodes_source_from_html(
            preliminary_html.to_string(),
            "61nni6me5p0n",
            |preliminary| {
                submitted_methods
                    .borrow_mut()
                    .push(preliminary.method_free.clone());
                futures_util::future::ready(Ok(standard_html.to_string()))
            },
        )
        .await
        .expect("source-only DataNodes preflight should reach the standard form");

        assert_eq!(submitted_methods.into_inner(), vec!["Free Download >>"]);
        assert_eq!(preflight.filename_hint.as_deref(), Some("Neon White.rar"));
        assert_eq!(preflight.resolved_url, None);
    }

    #[test]
    fn datanodes_resolver_parses_preliminary_download_form() {
        let html = r#"
            <form method="POST" action='' id="downloadForm" class="m-0 w-full">
                <input type="hidden" name="op" value="download1">
                <input type="hidden" name="usr_login" value="">
                <input type="hidden" name="id" value="wrpbp7ne3rby">
                <input type="hidden" name="fname" value="REVEIL_--_fitgirl-repacks.site_--_.part09.rar">
                <input type="hidden" name="referer" value="">
                <button type="submit" id="method_free" name="method_free" value="Free Download &gt;&gt;">
                    Continue to Download
                </button>
            </form>
        "#;

        let page = parse_datanodes_preliminary_download_page(html, "wrpbp7ne3rby")
            .expect("preliminary DataNodes page should parse");

        assert_eq!(page.action_url, DATANODES_DOWNLOAD_URL);
        assert_eq!(page.op, "download1");
        assert_eq!(page.id, "wrpbp7ne3rby");
        assert_eq!(page.fname, "REVEIL_--_fitgirl-repacks.site_--_.part09.rar");
        assert_eq!(page.referer, "");
        assert_eq!(page.method_free, "Free Download >>");
    }

    #[test]
    fn datanodes_resolver_parses_preliminary_free_download_card_without_method_free() {
        let html = r#"
            <form method="POST" action="/download" id="downloadForm" class="download-options">
                <input type="hidden" name="op" value="download1">
                <input type="hidden" name="usr_login" value="">
                <input type="hidden" name="id" value="wrpbp7ne3rby">
                <input type="hidden" name="fname" value="REVEIL_--_fitgirl-repacks.site_--_.part09.rar">
                <input type="hidden" name="referer" value="">
                <button type="submit" id="free-download-option" class="download-option free">
                    <span>Free Download</span>
                    <span>Standard speed</span>
                </button>
            </form>
        "#;

        let page = parse_datanodes_preliminary_download_page(html, "wrpbp7ne3rby")
            .expect("preliminary DataNodes free-download card should parse");

        assert_eq!(page.method_free, "Free Download >>");
    }

    #[test]
    fn datanodes_resolver_does_not_infer_free_method_from_comparison_table() {
        let html = r#"
            <form method="POST" action="/download" id="downloadForm">
                <input type="hidden" name="op" value="download1">
                <input type="hidden" name="id" value="wrpbp7ne3rby">
                <input type="hidden" name="fname" value="file.rar">
                <input type="hidden" name="referer" value="">
            </form>
            <table>
                <tr><th>Download type</th><td>Free</td><td>Premium</td></tr>
                <tr><th>Download speed</th><td>Standard</td><td>Maximum</td></tr>
            </table>
        "#;

        let error = parse_datanodes_preliminary_download_page(html, "wrpbp7ne3rby")
            .expect_err("comparison table alone should not count as a free method");

        assert_eq!(
            error.message,
            "DataNodes preliminary download form is missing the free download method."
        );
    }

    #[tokio::test]
    async fn datanodes_resolver_advances_two_preliminary_pages_before_standard_page() {
        let first_html = r#"
            <form method="POST" action="/download" id="downloadForm">
                <input type="hidden" name="op" value="download1">
                <input type="hidden" name="id" value="wrpbp7ne3rby">
                <input type="hidden" name="fname" value="file.rar">
                <input type="hidden" name="referer" value="">
                <button type="submit" name="method_free" value="Continue to Download">
                    Continue to Download
                </button>
            </form>
        "#;
        let second_html = r#"
            <form method="POST" action="/download" id="downloadForm">
                <input type="hidden" name="op" value="download1">
                <input type="hidden" name="id" value="wrpbp7ne3rby">
                <input type="hidden" name="fname" value="file.rar">
                <input type="hidden" name="referer" value="">
                <button type="submit" id="free-download-option" class="download-option free">
                    <span>Free Download</span>
                    <span>Standard speed</span>
                </button>
            </form>
        "#;
        let standard_html = r#"
            <download-countdown :countdown="5"
                code="wrpbp7ne3rby" referer="https://datanodes.to/download" rand="rand-token"
                free-method="Free Download &gt;&gt;" premium-method=""
                :has-password="false" :has-captcha="false"
                :has-countdown="true"
                name="file.rar"></download-countdown>
        "#;
        let responses = std::cell::RefCell::new(
            vec![second_html.to_string(), standard_html.to_string()].into_iter(),
        );
        let submitted_methods = std::cell::RefCell::new(Vec::new());

        let page = resolve_datanodes_download_page_from_html(
            first_html.to_string(),
            "wrpbp7ne3rby",
            |preliminary| {
                submitted_methods
                    .borrow_mut()
                    .push(preliminary.method_free.clone());
                futures_util::future::ready(Ok(responses
                    .borrow_mut()
                    .next()
                    .expect("mock response")))
            },
        )
        .await
        .expect("two preliminary DataNodes pages should reach the standard page");

        assert_eq!(page.code, "wrpbp7ne3rby");
        assert_eq!(
            submitted_methods.into_inner(),
            vec![
                "Continue to Download".to_string(),
                "Free Download >>".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn datanodes_resolver_stops_repeated_preliminary_pages() {
        let html = r#"
            <form method="POST" action="/download" id="downloadForm">
                <input type="hidden" name="op" value="download1">
                <input type="hidden" name="id" value="wrpbp7ne3rby">
                <input type="hidden" name="fname" value="file.rar">
                <input type="hidden" name="referer" value="">
                <button type="submit" name="method_free" value="Continue to Download">
                    Continue to Download
                </button>
            </form>
        "#;
        let requests = std::cell::Cell::new(0);

        let error =
            resolve_datanodes_download_page_from_html(html.to_string(), "wrpbp7ne3rby", |_| {
                requests.set(requests.get() + 1);
                futures_util::future::ready(Ok(html.to_string()))
            })
            .await
            .expect_err("repeated preliminary pages should stop without looping");

        assert_eq!(requests.get(), 1);
        assert!(
            error.message.contains("repeated preliminary"),
            "unexpected error: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn hoster_resolution_retries_transient_errors_before_success() {
        let attempts = std::cell::Cell::new(0);

        let resolved = retry_hoster_resolution("test resolver", || {
            attempts.set(attempts.get() + 1);
            futures_util::future::ready(if attempts.get() < 3 {
                Err(transient_resolution_error(
                    "temporary resolver outage".into(),
                ))
            } else {
                Ok("resolved")
            })
        })
        .await
        .expect("transient resolver failures should retry");

        assert_eq!(resolved, "resolved");
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn hoster_resolution_does_not_retry_terminal_errors() {
        let attempts = std::cell::Cell::new(0);

        let error = retry_hoster_resolution("test resolver", || {
            attempts.set(attempts.get() + 1);
            futures_util::future::ready(Err::<(), _>(resolution_error(
                "DataNodes captcha-protected downloads are not supported.".into(),
            )))
        })
        .await
        .expect_err("terminal resolver failures should not retry");

        assert_eq!(attempts.get(), 1);
        assert!(!error.retryable);
    }

    #[test]
    fn datanodes_resolver_rejects_preliminary_form_for_different_file_code() {
        let html = r#"
            <form method="POST" action="/download" id="downloadForm">
                <input type="hidden" name="op" value="download1">
                <input type="hidden" name="id" value="othercode">
                <input type="hidden" name="fname" value="file.rar">
                <input type="hidden" name="referer" value="">
                <button type="submit" name="method_free" value="Free Download &gt;&gt;">Continue to Download</button>
            </form>
        "#;

        assert!(parse_datanodes_preliminary_download_page(html, "wrpbp7ne3rby").is_err());
    }

    #[test]
    fn datanodes_resolver_rejects_preliminary_form_on_unexpected_action_host() {
        let html = r#"
            <form method="POST" action="https://evil.example/download" id="downloadForm">
                <input type="hidden" name="op" value="download1">
                <input type="hidden" name="id" value="wrpbp7ne3rby">
                <input type="hidden" name="fname" value="file.rar">
                <input type="hidden" name="referer" value="">
                <button type="submit" name="method_free" value="Free Download &gt;&gt;">Continue to Download</button>
            </form>
        "#;

        assert!(parse_datanodes_preliminary_download_page(html, "wrpbp7ne3rby").is_err());
    }

    #[test]
    fn datanodes_resolver_rejects_captcha_and_password_pages() {
        let captcha = r#"
            <download-countdown :countdown="5" code="61nni6me5p0n"
                referer="https://datanodes.to/download" rand=""
                free-method="Free Download &gt;&gt;" premium-method=""
                :has-password="false" :has-captcha="true"></download-countdown>
        "#;
        let password = r#"
            <download-countdown :countdown="5" code="61nni6me5p0n"
                referer="https://datanodes.to/download" rand=""
                free-method="Free Download &gt;&gt;" premium-method=""
                :has-password="true" :has-captcha="false"></download-countdown>
        "#;

        assert!(parse_datanodes_standard_download_page(captcha).is_err());
        assert!(parse_datanodes_standard_download_page(password).is_err());
    }

    #[test]
    fn datanodes_resolver_decodes_and_validates_direct_link_json() {
        let json = r#"{"url":"https%3A%2F%2Fnode41.datanodes.to%3A8443%2Fd%2Ftoken_123%2FNeon-White.rar"}"#;

        let direct = extract_datanodes_direct_url_from_json(json)
            .expect("DataNodes direct URL JSON should parse");

        assert_eq!(
            direct,
            "https://node41.datanodes.to:8443/d/token_123/Neon-White.rar"
        );
        assert!(validate_datanodes_direct_url(&direct));
    }

    #[test]
    fn datanodes_resolver_accepts_proxy_direct_link_json() {
        let json = r#"{"url":"https%3A%2F%2Ftunnel5.dlproxy.uk%2Fdownload%2Fproxy-token_123"}"#;

        let direct = extract_datanodes_direct_url_from_json(json)
            .expect("DataNodes proxy direct URL JSON should parse");

        assert_eq!(
            direct,
            "https://tunnel5.dlproxy.uk/download/proxy-token_123"
        );
        assert!(validate_datanodes_direct_url(&direct));
    }

    #[test]
    fn datanodes_direct_download_context_includes_cookie_referer_and_user_agent() {
        let context = hoster_download_context_for_resolved_url(
            "https://node41.datanodes.to:8443/d/token_123/Neon-White.rar",
            Some("https://datanodes.to/61nni6me5p0n/Neon-White.rar"),
        )
        .expect("DataNodes direct URLs resolved from a public page should get request context");
        let headers = context
            .headers
            .iter()
            .map(|header| (header.name.as_str(), header.value.as_str()))
            .collect::<Vec<_>>();

        assert!(headers.contains(&("Cookie", "file_code=61nni6me5p0n")));
        assert!(headers.contains(&("Referer", DATANODES_DOWNLOAD_URL)));
        assert!(
            headers
                .iter()
                .any(|(name, value)| *name == "User-Agent"
                    && value.contains("SimpleDownloadManager")),
            "DataNodes context should carry an explicit downloader User-Agent"
        );
    }

    #[test]
    fn datanodes_direct_urls_have_safe_hoster_acceleration_policy() {
        let policy = hoster_acceleration_policy(
            "https://datanodes.to/abc123456789/fg-optional-bonus-content.bin",
            "https://s42.datanodes.to/d/abc123456789/fg-optional-bonus-content.bin",
        )
        .expect("validated DataNodes direct URLs should opt into safe acceleration");

        assert_eq!(policy.backoff_key, "hoster:datanodes:abc123456789");
        assert_eq!(policy.max_balanced_segments, 4);
        assert_eq!(policy.max_fast_segments, 6);
    }

    #[test]
    fn fuckingfast_direct_urls_have_safe_hoster_acceleration_policy() {
        let policy = hoster_acceleration_policy(
            "https://fuckingfast.co/ecw0lw398okf#Game.part01.rar",
            "https://dl.fuckingfast.co/dl/token/Game.part01.rar",
        )
        .expect("validated FuckingFast direct URLs should opt into safe acceleration");

        assert_eq!(policy.backoff_key, "hoster:fuckingfast:ecw0lw398okf");
        assert_eq!(policy.max_balanced_segments, 4);
        assert_eq!(policy.max_fast_segments, 6);

        let same_source_with_other_fragment = hoster_acceleration_policy(
            "https://www.fuckingfast.co/ecw0lw398okf#Other.part01.rar",
            "https://dl.fuckingfast.co/dl/other-token/Other.part01.rar",
        )
        .expect("filename fragments should not affect the FuckingFast backoff key");
        assert_eq!(
            same_source_with_other_fragment.backoff_key,
            "hoster:fuckingfast:ecw0lw398okf"
        );
    }

    #[test]
    fn fuckingfast_acceleration_policy_rejects_invalid_direct_urls() {
        assert!(hoster_acceleration_policy(
            "https://fuckingfast.co/ecw0lw398okf#Game.part01.rar",
            "https://fuckingfast.co/dl/token/Game.part01.rar",
        )
        .is_none());
        assert!(hoster_acceleration_policy(
            "https://fuckingfast.co/ecw0lw398okf#Game.part01.rar",
            "https://dl.fuckingfast.co/not-dl/token/Game.part01.rar",
        )
        .is_none());
    }

    #[test]
    fn unverified_hosters_remain_single_stream_for_bulk_acceleration() {
        assert!(hoster_acceleration_policy(
            "https://example.com/file.bin",
            "https://cdn.example.com/file.bin",
        )
        .is_none());
    }

    #[test]
    fn datanodes_direct_download_context_rejects_missing_source_page() {
        assert!(hoster_download_context_for_resolved_url(
            "https://node41.datanodes.to:8443/d/token_123/Neon-White.rar",
            None,
        )
        .is_none());
        assert!(hoster_download_context_for_resolved_url(
            "https://node41.datanodes.to:8443/d/token_123/Neon-White.rar",
            Some("https://example.com/61nni6me5p0n/Neon-White.rar"),
        )
        .is_none());
    }

    #[test]
    fn datanodes_proxy_download_context_does_not_forward_source_cookie() {
        assert!(hoster_download_context_for_resolved_url(
            "https://tunnel5.dlproxy.uk/download/proxy-token_123",
            Some("https://datanodes.to/61nni6me5p0n/Neon-White.rar"),
        )
        .is_none());
    }

    #[test]
    fn datanodes_resolver_accepts_datanodes_file_server_subdomains() {
        for host in [
            "node41.datanodes.to:8443",
            "dl.datanodes.to",
            "cdn2.datanodes.to",
            "stor03.datanodes.to",
            "mnode.datanodes.to",
            "rocket.datanodes.to",
        ] {
            let direct_url = format!("https://{host}/d/token_123/file.rar");
            let json = format!(r#"{{"url":"{direct_url}"}}"#);

            let direct = extract_datanodes_direct_url_from_json(&json)
                .expect("DataNodes file-server direct URL should parse");

            assert_eq!(direct, direct_url);
            assert!(
                validate_datanodes_direct_url(&direct),
                "{direct} should be treated as a DataNodes direct file URL"
            );
        }
    }

    #[test]
    fn datanodes_resolver_rejects_non_file_direct_urls_with_reason() {
        for (direct_url, expected_reason) in [
            (
                "https://datanodes.to/d/token/file.rar",
                "host `datanodes.to` is not a supported DataNodes file server",
            ),
            (
                "https://www.datanodes.to/d/token/file.rar",
                "host `www.datanodes.to` is not a supported DataNodes file server",
            ),
            (
                "https://evil.example/d/token/file.rar",
                "host `evil.example` is not a supported DataNodes file server",
            ),
            (
                "http://node41.datanodes.to/d/token/file.rar",
                "scheme `http` is not https",
            ),
            (
                "https://node41.datanodes.to/not-download/file.rar",
                "path `/not-download/file.rar` is not a DataNodes file download path",
            ),
            (
                "http://tunnel5.dlproxy.uk/download/file.rar",
                "scheme `http` is not https",
            ),
            (
                "https://dlproxy.uk/download/file.rar",
                "host `dlproxy.uk` is not a supported DataNodes file server",
            ),
            (
                "https://www.dlproxy.uk/download/file.rar",
                "host `www.dlproxy.uk` is not a supported DataNodes file server",
            ),
            (
                "https://evil-dlproxy.uk/download/file.rar",
                "host `evil-dlproxy.uk` is not a supported DataNodes file server",
            ),
            (
                "https://tunnel5.dlproxy.uk.evil/download/file.rar",
                "host `tunnel5.dlproxy.uk.evil` is not a supported DataNodes file server",
            ),
            (
                "https://tunnel5.dlproxy.uk/not-download/file.rar",
                "path `/not-download/file.rar` is not a DataNodes file download path",
            ),
        ] {
            let json = format!(r#"{{"url":"{direct_url}"}}"#);

            let error = extract_datanodes_direct_url_from_json(&json)
                .expect_err("invalid DataNodes direct URL should be rejected");

            assert!(
                error.message.contains(expected_reason),
                "{direct_url} should explain rejection reason; got: {}",
                error.message
            );
        }
    }

    #[test]
    fn datanodes_resolver_rejects_invalid_direct_link_json() {
        for json in [
            r#"{"url":"https%3A%2F%2Fevil.example%2Fd%2Ftoken%2Ffile.rar"}"#,
            r#"{"url":"https%3A%2F%2Fnode41.datanodes.to%3A8443%2Fnot-download%2Ffile.rar"}"#,
            r#"{"error":"Premium only"}"#,
        ] {
            assert!(
                extract_datanodes_direct_url_from_json(json).is_err(),
                "{json} should not resolve to a direct DataNodes URL"
            );
        }
    }

    #[test]
    fn resolved_hoster_links_are_reordered_after_parallel_resolution() {
        let resolved = ordered_resolved_hoster_links(
            vec![
                Ok((
                    2,
                    ResolvedHosterLink {
                        url: "https://node41.datanodes.to:8443/d/token/file.rar".into(),
                        filename_hint: Some("file.rar".into()),
                        resolved_from_url: Some(
                            "https://datanodes.to/61nni6me5p0n/file.rar".into(),
                        ),
                    },
                )),
                Ok((
                    0,
                    ResolvedHosterLink {
                        url: "https://example.com/file.zip".into(),
                        filename_hint: None,
                        resolved_from_url: None,
                    },
                )),
                Ok((
                    1,
                    ResolvedHosterLink {
                        url: "https://dl.fuckingfast.co/dl/direct-token_123".into(),
                        filename_hint: Some("archive.part01.rar".into()),
                        resolved_from_url: Some(
                            "https://fuckingfast.co/ecw0lw398okf#archive.part01.rar".into(),
                        ),
                    },
                )),
            ],
            3,
        )
        .expect("ordered links should be restored by input index");

        assert_eq!(
            resolved
                .iter()
                .map(|link| link.url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "https://example.com/file.zip",
                "https://dl.fuckingfast.co/dl/direct-token_123",
                "https://node41.datanodes.to:8443/d/token/file.rar",
            ]
        );
    }

    #[test]
    fn partial_hoster_resolution_preserves_success_and_failure_order() {
        let batch = ordered_resolved_hoster_batch(
            vec![
                Ok((
                    2,
                    ResolvedHosterLink {
                        url: "https://node41.datanodes.to:8443/d/token/file.rar".into(),
                        filename_hint: Some("file.rar".into()),
                        resolved_from_url: Some(
                            "https://datanodes.to/61nni6me5p0n/file.rar".into(),
                        ),
                    },
                )),
                Err((
                    1,
                    FailedHosterLink {
                        url: "https://datanodes.to/61nni6me5p0n/protected.rar".into(),
                        message: "DataNodes captcha-protected downloads are not supported.".into(),
                    },
                )),
                Ok((
                    0,
                    ResolvedHosterLink {
                        url: "https://example.com/file.zip".into(),
                        filename_hint: None,
                        resolved_from_url: None,
                    },
                )),
            ],
            3,
        )
        .expect("partial batch should preserve input order within successes and failures");

        assert_eq!(
            batch
                .links
                .iter()
                .map(|link| link.url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "https://example.com/file.zip",
                "https://node41.datanodes.to:8443/d/token/file.rar",
            ]
        );
        assert_eq!(
            batch
                .failed_links
                .iter()
                .map(|item| item.url.as_str())
                .collect::<Vec<_>>(),
            vec!["https://datanodes.to/61nni6me5p0n/protected.rar"]
        );
    }

    #[test]
    fn fuckingfast_resolver_rejects_pages_without_direct_download_link() {
        let error = resolve_hoster_link_from_html(
            "https://fuckingfast.co/ecw0lw398okf",
            "<html><button>DOWNLOAD</button></html>",
        )
        .expect_err("missing direct link should fail");

        assert_eq!(error.code, "HOSTER_RESOLUTION_FAILED");
    }

    #[test]
    fn fuckingfast_resolver_rejects_direct_links_on_unexpected_hosts() {
        let error = resolve_hoster_link_from_html(
            "https://fuckingfast.co/ecw0lw398okf",
            r#"<script>window.open("https://evil.example/dl/direct-token_123")</script>"#,
        )
        .expect_err("unexpected direct host should fail");

        assert_eq!(error.code, "HOSTER_RESOLUTION_FAILED");
    }

    #[test]
    fn fuckingfast_resolver_skips_invalid_window_open_candidates() {
        let html = r#"
            <script>window.open("https://evil.example/dl/direct-token_123")</script>
            <script>window.open("https://dl.fuckingfast.co/dl/direct-token_456")</script>
        "#;

        let resolved = resolve_hoster_link_from_html("https://fuckingfast.co/ecw0lw398okf", html)
            .expect("resolver should continue past invalid candidates");

        assert_eq!(
            resolved.url,
            "https://dl.fuckingfast.co/dl/direct-token_456"
        );
    }
}
