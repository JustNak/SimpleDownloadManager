use percent_encoding::percent_decode_str;
use reqwest::redirect::Policy;
use reqwest::Client;
use std::time::Duration;
use url::Url;

const FUCKINGFAST_HOST: &str = "fuckingfast.co";
const FUCKINGFAST_DIRECT_HOST: &str = "dl.fuckingfast.co";
const HOSTER_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HOSTER_READ_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHosterLink {
    pub url: String,
    pub filename_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HosterResolutionError {
    pub code: &'static str,
    pub message: String,
}

pub async fn resolve_hoster_links(
    urls: Vec<String>,
) -> Result<Vec<ResolvedHosterLink>, HosterResolutionError> {
    let client = Client::builder()
        .connect_timeout(HOSTER_CONNECT_TIMEOUT)
        .read_timeout(HOSTER_READ_TIMEOUT)
        .redirect(Policy::limited(5))
        .user_agent("SimpleDownloadManager/0.5")
        .build()
        .map_err(|error| resolution_error(format!("Could not create hoster resolver: {error}")))?;

    let mut resolved = Vec::with_capacity(urls.len());
    for url in urls {
        if !is_fuckingfast_page_url(&url) {
            resolved.push(ResolvedHosterLink {
                url: url.trim().to_string(),
                filename_hint: None,
            });
            continue;
        }

        let response = client.get(url.trim()).send().await.map_err(|error| {
            resolution_error(format!("Could not load FuckingFast page: {error}"))
        })?;

        if !response.status().is_success() {
            return Err(resolution_error(format!(
                "Could not load FuckingFast page: HTTP {}.",
                response.status()
            )));
        }

        let html = response.text().await.map_err(|error| {
            resolution_error(format!("Could not read FuckingFast page: {error}"))
        })?;
        resolved.push(resolve_hoster_link_from_html(&url, &html)?);
    }

    Ok(resolved)
}

pub fn resolve_hoster_link_from_html(
    original_url: &str,
    html: &str,
) -> Result<ResolvedHosterLink, HosterResolutionError> {
    if !is_fuckingfast_page_url(original_url) {
        return Ok(ResolvedHosterLink {
            url: original_url.trim().to_string(),
            filename_hint: None,
        });
    }

    let direct_url = extract_fuckingfast_direct_url(html)?;
    Ok(ResolvedHosterLink {
        url: direct_url,
        filename_hint: filename_hint_from_fragment(original_url)
            .or_else(|| filename_hint_from_meta_title(html))
            .or_else(|| filename_hint_from_title(html)),
    })
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

fn extract_fuckingfast_direct_url(html: &str) -> Result<String, HosterResolutionError> {
    let candidates = extract_window_open_literal_urls(html);
    for candidate in candidates {
        if validate_fuckingfast_direct_url(&candidate) {
            return Ok(candidate);
        }

        return Err(resolution_error(
            "FuckingFast page pointed at an unexpected download host.".into(),
        ));
    }

    if let Some(candidate) = extract_any_fuckingfast_direct_url(html) {
        if validate_fuckingfast_direct_url(&candidate) {
            return Ok(candidate);
        }
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
    }
}
