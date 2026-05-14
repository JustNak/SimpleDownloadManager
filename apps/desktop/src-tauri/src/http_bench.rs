use futures_util::StreamExt;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use std::env;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::io::AsyncWriteExt;

const REPORT_SCHEMA_VERSION: u32 = 2;
const DEFAULT_DURATION: Duration = Duration::from_secs(30);
const SEGMENT_VARIANTS: [usize; 7] = [8, 12, 16, 24, 32, 48, 64];
const BENCHMARK_MODES: [HttpBenchmarkMode; 2] =
    [HttpBenchmarkMode::NetworkOnly, HttpBenchmarkMode::DiskWrite];
const BENCHMARK_ADMISSIONS: [HttpBenchmarkAdmission; 3] = [
    HttpBenchmarkAdmission::Normal,
    HttpBenchmarkAdmission::DirectBulk,
    HttpBenchmarkAdmission::ProtectedHosterBulk,
];
const BENCHMARK_NORMAL_FAST_ORIGIN_SEGMENT_CAP: usize = 64;
const BENCHMARK_PROTECTED_HOSTER_FAST_ADAPTIVE_SEGMENT_CAP: usize = 10;

#[derive(Debug, Serialize)]
pub struct HttpBenchmarkReport {
    pub schema_version: u32,
    pub run_started_unix_ms: u64,
    pub duration_ms: u64,
    pub source: String,
    pub variants: Vec<HttpBenchmarkVariant>,
}

#[derive(Debug, Serialize)]
pub struct HttpBenchmarkVariant {
    pub label: String,
    pub admission_class: String,
    pub requested_segments: usize,
    pub admitted_segments: usize,
    pub segments: usize,
    pub mode: String,
    pub transport: String,
    pub first_byte_time_ms: Option<u64>,
    pub average_bps: u64,
    pub bytes_read: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum HttpBenchmarkMode {
    NetworkOnly,
    DiskWrite,
}

#[derive(Debug, Clone, Copy)]
enum HttpBenchmarkAdmission {
    Normal,
    DirectBulk,
    ProtectedHosterBulk,
}

impl HttpBenchmarkAdmission {
    fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::DirectBulk => "direct_bulk",
            Self::ProtectedHosterBulk => "protected_hoster_bulk",
        }
    }

    fn admit_segments(self, requested_segments: usize) -> usize {
        match self {
            Self::Normal | Self::DirectBulk => {
                requested_segments.min(BENCHMARK_NORMAL_FAST_ORIGIN_SEGMENT_CAP)
            }
            Self::ProtectedHosterBulk => {
                requested_segments.min(BENCHMARK_PROTECTED_HOSTER_FAST_ADAPTIVE_SEGMENT_CAP)
            }
        }
        .max(1)
    }
}

impl HttpBenchmarkMode {
    fn label(self) -> &'static str {
        match self {
            Self::NetworkOnly => "network-only",
            Self::DiskWrite => "disk-write",
        }
    }
}

pub async fn run_benchmark_from_env() -> Result<HttpBenchmarkReport, String> {
    let source = env::var("SDM_HTTP_BENCH_URL")
        .map_err(|_| "Set SDM_HTTP_BENCH_URL to a legal/user-authorized HTTP URL.".to_string())?;
    let duration = env::var("SDM_HTTP_BENCH_DURATION_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_DURATION);
    let output_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("http-bench");

    run_benchmark(&source, duration, output_root).await
}

pub async fn run_benchmark(
    source: &str,
    duration: Duration,
    output_root: PathBuf,
) -> Result<HttpBenchmarkReport, String> {
    let run_started_unix_ms = unix_millis();
    let run_dir = output_root.join(format!("run-{run_started_unix_ms}-{}", std::process::id()));
    fs::create_dir_all(&run_dir)
        .await
        .map_err(|error| format!("Could not create HTTP benchmark directory: {error}"))?;

    let mut variants = Vec::with_capacity(
        SEGMENT_VARIANTS.len() * BENCHMARK_MODES.len() * BENCHMARK_ADMISSIONS.len(),
    );
    for mode in BENCHMARK_MODES {
        for admission in BENCHMARK_ADMISSIONS {
            for requested_segments in SEGMENT_VARIANTS {
                let admitted_segments = admission.admit_segments(requested_segments);
                variants.push(
                    run_variant(
                        source,
                        admission,
                        requested_segments,
                        admitted_segments,
                        mode,
                        duration,
                        &run_dir,
                    )
                    .await,
                );
            }
        }
    }

    let report = HttpBenchmarkReport {
        schema_version: REPORT_SCHEMA_VERSION,
        run_started_unix_ms,
        duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
        source: redact_url(source),
        variants,
    };
    let report_json = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("Could not serialize HTTP benchmark report: {error}"))?;
    fs::write(run_dir.join("report.json"), report_json)
        .await
        .map_err(|error| format!("Could not write HTTP benchmark report: {error}"))?;
    Ok(report)
}

async fn run_variant(
    source: &str,
    admission: HttpBenchmarkAdmission,
    requested_segments: usize,
    admitted_segments: usize,
    mode: HttpBenchmarkMode,
    duration: Duration,
    run_dir: &std::path::Path,
) -> HttpBenchmarkVariant {
    let started = Instant::now();
    let client = match Client::builder()
        .pool_idle_timeout(Some(Duration::from_secs(120)))
        .pool_max_idle_per_host(64)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .http1_only()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return benchmark_error(
                admission,
                requested_segments,
                admitted_segments,
                mode,
                format!("Could not create benchmark client: {error}"),
            );
        }
    };

    let total_bytes = match probe_total_bytes(&client, source).await {
        Ok(total_bytes) => total_bytes,
        Err(error) => {
            return benchmark_error(
                admission,
                requested_segments,
                admitted_segments,
                mode,
                error,
            )
        }
    };

    let ranges = partition_ranges(total_bytes, admitted_segments);
    let deadline = Instant::now() + duration;
    let mut handles = tokio::task::JoinSet::new();
    for (index, range) in ranges.into_iter().enumerate() {
        let client = client.clone();
        let source = source.to_string();
        let output_path = match mode {
            HttpBenchmarkMode::NetworkOnly => None,
            HttpBenchmarkMode::DiskWrite => Some(run_dir.join(format!(
                "http1-{}-{requested_segments}-{admitted_segments}-{}-{index}.part",
                admission.label(),
                mode.label()
            ))),
        };
        handles.spawn(async move {
            read_range_until(&client, &source, range, deadline, output_path).await
        });
    }

    let mut bytes_read = 0_u64;
    let mut first_byte_time_ms = None;
    let mut error = None;
    while let Some(result) = handles.join_next().await {
        match result {
            Ok(Ok(sample)) => {
                bytes_read = bytes_read.saturating_add(sample.bytes_read);
                first_byte_time_ms = min_optional(first_byte_time_ms, sample.first_byte_time_ms);
            }
            Ok(Err(message)) if error.is_none() => error = Some(message),
            Err(join_error) if error.is_none() => {
                error = Some(format!("Worker failed: {join_error}"))
            }
            _ => {}
        }
    }

    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    HttpBenchmarkVariant {
        label: benchmark_variant_label(admission, requested_segments, admitted_segments, mode),
        admission_class: admission.label().into(),
        requested_segments,
        admitted_segments,
        segments: admitted_segments,
        mode: mode.label().into(),
        transport: "http/1.1".into(),
        first_byte_time_ms,
        average_bps: (bytes_read as f64 / elapsed) as u64,
        bytes_read,
        error,
    }
}

async fn probe_total_bytes(client: &Client, source: &str) -> Result<u64, String> {
    let response = client
        .get(source)
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(|error| format!("Range probe failed: {error}"))?;
    if response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(format!("Range probe returned {}", response.status()));
    }
    let content_range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "Range probe did not include Content-Range.".to_string())?;
    content_range
        .split('/')
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "Range probe did not include a valid total length.".to_string())
}

fn partition_ranges(total_bytes: u64, segments: usize) -> Vec<(u64, u64)> {
    let segments = segments.max(1).min(total_bytes.max(1) as usize);
    let segment_size = total_bytes / segments as u64;
    (0..segments)
        .map(|index| {
            let start = index as u64 * segment_size;
            let end = if index == segments - 1 {
                total_bytes - 1
            } else {
                ((index as u64 + 1) * segment_size).saturating_sub(1)
            };
            (start, end)
        })
        .collect()
}

#[derive(Debug)]
struct RangeReadSample {
    bytes_read: u64,
    first_byte_time_ms: Option<u64>,
}

async fn read_range_until(
    client: &Client,
    source: &str,
    range: (u64, u64),
    deadline: Instant,
    output_path: Option<PathBuf>,
) -> Result<RangeReadSample, String> {
    let started = Instant::now();
    let response = client
        .get(source)
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, format!("bytes={}-{}", range.0, range.1))
        .send()
        .await
        .map_err(|error| format!("Range request failed: {error}"))?;
    if response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(format!("Range request returned {}", response.status()));
    }

    let mut stream = response.bytes_stream();
    let mut output_file = match output_path {
        Some(path) => Some(
            fs::File::create(path)
                .await
                .map_err(|error| format!("Could not create benchmark output file: {error}"))?,
        ),
        None => None,
    };
    let mut bytes_read = 0_u64;
    let mut first_byte_time_ms = None;
    while Instant::now() < deadline {
        match tokio::time::timeout_at(deadline.into(), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                if first_byte_time_ms.is_none() {
                    first_byte_time_ms =
                        Some(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
                }
                if let Some(file) = output_file.as_mut() {
                    file.write_all(&chunk)
                        .await
                        .map_err(|error| format!("Could not write benchmark output: {error}"))?;
                }
                bytes_read = bytes_read.saturating_add(chunk.len() as u64);
            }
            Ok(Some(Err(error))) => return Err(format!("Range stream failed: {error}")),
            Ok(None) | Err(_) => break,
        }
    }

    if let Some(file) = output_file.as_mut() {
        file.flush()
            .await
            .map_err(|error| format!("Could not flush benchmark output: {error}"))?;
    }

    Ok(RangeReadSample {
        bytes_read,
        first_byte_time_ms,
    })
}

fn benchmark_error(
    admission: HttpBenchmarkAdmission,
    requested_segments: usize,
    admitted_segments: usize,
    mode: HttpBenchmarkMode,
    error: String,
) -> HttpBenchmarkVariant {
    HttpBenchmarkVariant {
        label: benchmark_variant_label(admission, requested_segments, admitted_segments, mode),
        admission_class: admission.label().into(),
        requested_segments,
        admitted_segments,
        segments: admitted_segments,
        mode: mode.label().into(),
        transport: "http/1.1".into(),
        first_byte_time_ms: None,
        average_bps: 0,
        bytes_read: 0,
        error: Some(error),
    }
}

fn benchmark_variant_label(
    admission: HttpBenchmarkAdmission,
    requested_segments: usize,
    admitted_segments: usize,
    mode: HttpBenchmarkMode,
) -> String {
    format!(
        "http1-{}-requested-{requested_segments}-admitted-{admitted_segments}-{}",
        admission.label(),
        mode.label()
    )
}

fn min_optional(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn redact_url(source: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(source) else {
        return "<invalid-url>".into();
    };
    url.set_query(url.query().map(|_| "<redacted>"));
    url.set_fragment(url.fragment().map(|_| "<redacted>"));
    url.to_string()
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_ranges_covers_total_without_overlap() {
        assert_eq!(partition_ranges(10, 3), vec![(0, 2), (3, 5), (6, 9)]);
    }

    #[test]
    fn live_http_benchmark_covers_adaptive_sustain_variants() {
        assert_eq!(SEGMENT_VARIANTS, [8, 12, 16, 24, 32, 48, 64]);
    }

    #[test]
    fn benchmark_schema_records_bulk_admission_details() {
        assert_eq!(REPORT_SCHEMA_VERSION, 2);

        let variant = benchmark_error(
            HttpBenchmarkAdmission::DirectBulk,
            64,
            64,
            HttpBenchmarkMode::NetworkOnly,
            "expected error".into(),
        );

        assert_eq!(variant.admission_class, "direct_bulk");
        assert_eq!(variant.requested_segments, 64);
        assert_eq!(variant.admitted_segments, 64);
        assert_eq!(variant.segments, 64);
    }

    #[tokio::test]
    #[ignore = "requires SDM_HTTP_BENCH_URL"]
    async fn live_http_benchmark_from_env() {
        std::env::var("SDM_HTTP_BENCH_URL")
            .expect("set SDM_HTTP_BENCH_URL to a legal/user-authorized HTTP URL");

        let report = run_benchmark_from_env()
            .await
            .expect("live HTTP benchmark should run");

        assert!(!report.variants.is_empty());
    }
}
