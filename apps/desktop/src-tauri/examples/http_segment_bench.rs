use futures_util::StreamExt;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE};
use reqwest::{Client, StatusCode};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::Mutex;

const DEFAULT_DURATION: Duration = Duration::from_secs(8);
const DEFAULT_SEGMENTS: &[usize] = &[4, 6, 8, 10, 12, 16];
const WRITE_BUFFER_SIZE: usize = 1024 * 1024;

#[tokio::main]
async fn main() -> Result<(), String> {
    let urls = env::var("SDM_HTTP_SEGMENT_BENCH_URLS")
        .or_else(|_| env::var("SDM_HTTP_BENCH_URL"))
        .map_err(|_| {
            "Set SDM_HTTP_SEGMENT_BENCH_URLS or SDM_HTTP_BENCH_URL to authorized URLs.".to_string()
        })?;
    let duration = env::var("SDM_HTTP_BENCH_DURATION_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_DURATION);
    let segment_counts = env::var("SDM_HTTP_SEGMENT_BENCH_SEGMENTS")
        .ok()
        .map(|value| parse_segments(&value))
        .filter(|segments| !segments.is_empty())
        .unwrap_or_else(|| DEFAULT_SEGMENTS.to_vec());
    let output_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("http-segment-bench");
    fs::create_dir_all(&output_root)
        .await
        .map_err(|error| format!("Could not create segment benchmark directory: {error}"))?;

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(120))
        .pool_idle_timeout(Some(Duration::from_secs(120)))
        .pool_max_idle_per_host(64)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .http1_only()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
        .map_err(|error| format!("Could not create benchmark client: {error}"))?;

    for (url_index, url) in urls
        .split(['\n', ';'])
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .enumerate()
    {
        let total_bytes = match probe_total_bytes(&client, url).await {
            Ok(total_bytes) => total_bytes,
            Err(error) => {
                println!("url{url_index} probe_error={error}");
                continue;
            }
        };
        println!("url{url_index} total_mib={}", total_bytes / (1024 * 1024));

        for segments in &segment_counts {
            run_segment_variant(
                &client,
                url,
                url_index,
                *segments,
                total_bytes,
                duration,
                &output_root,
            )
            .await;
        }
    }

    Ok(())
}

fn parse_segments(raw: &str) -> Vec<usize> {
    raw.split(',')
        .filter_map(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .collect()
}

async fn probe_total_bytes(client: &Client, source: &str) -> Result<u64, String> {
    let response = client
        .get(source)
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(|error| format!("range probe failed: {error}"))?;
    if response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(format!("range probe returned {}", response.status()));
    }
    response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split('/').nth(1))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "range probe did not include a valid total length".to_string())
}

async fn run_segment_variant(
    client: &Client,
    source: &str,
    url_index: usize,
    segments: usize,
    total_bytes: u64,
    duration: Duration,
    output_root: &PathBuf,
) {
    let ranges = partition_ranges(total_bytes, segments);
    let started = Instant::now();
    let deadline = started + duration;
    let sample_bytes = Arc::new(Mutex::new(0_u64));
    let total_read = Arc::new(Mutex::new(0_u64));
    let first_byte_ms = Arc::new(Mutex::new(None::<u128>));
    let mut handles = tokio::task::JoinSet::new();

    for (index, range) in ranges.into_iter().enumerate() {
        let client = client.clone();
        let source = source.to_string();
        let sample_bytes = sample_bytes.clone();
        let total_read = total_read.clone();
        let first_byte_ms = first_byte_ms.clone();
        let path = output_root.join(format!("url{url_index}-{segments}-{index}.part"));
        handles.spawn(async move {
            read_range(
                &client,
                &source,
                range,
                deadline,
                path,
                started,
                sample_bytes,
                total_read,
                first_byte_ms,
            )
            .await
        });
    }

    let sampler_sample_bytes = sample_bytes.clone();
    let sampler = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            if Instant::now() >= deadline {
                break;
            }
            let mut bytes = sampler_sample_bytes.lock().await;
            let interval_bytes = std::mem::take(&mut *bytes);
            println!(
                "url{url_index} segments={segments} sample_mbps={:.1}",
                interval_bytes as f64 * 8.0 / 1_000_000.0
            );
        }
    });

    let mut error = None;
    while let Some(result) = handles.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(message)) if error.is_none() => error = Some(message),
            Err(message) if error.is_none() => error = Some(message.to_string()),
            _ => {}
        }
    }
    let _ = sampler.await;

    let read = *total_read.lock().await;
    let first = *first_byte_ms.lock().await;
    println!(
        "url{url_index} segments={segments} average_mbps={:.1} first_byte_ms={} error={}",
        read as f64 * 8.0 / started.elapsed().as_secs_f64() / 1_000_000.0,
        first
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".into()),
        error.unwrap_or_else(|| "none".into())
    );
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

#[allow(clippy::too_many_arguments)]
async fn read_range(
    client: &Client,
    source: &str,
    range: (u64, u64),
    deadline: Instant,
    path: PathBuf,
    started: Instant,
    sample_bytes: Arc<Mutex<u64>>,
    total_read: Arc<Mutex<u64>>,
    first_byte_ms: Arc<Mutex<Option<u128>>>,
) -> Result<(), String> {
    let response = client
        .get(source)
        .header(ACCEPT_ENCODING, "identity")
        .header(RANGE, format!("bytes={}-{}", range.0, range.1))
        .send()
        .await
        .map_err(|error| format!("range request failed: {error}"))?;
    if response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(format!("range request returned {}", response.status()));
    }

    let file = fs::File::create(path)
        .await
        .map_err(|error| format!("could not create benchmark output: {error}"))?;
    let mut writer = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);
    let mut stream = response.bytes_stream();
    while Instant::now() < deadline {
        match tokio::time::timeout_at(deadline.into(), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                {
                    let mut first = first_byte_ms.lock().await;
                    if first.is_none() {
                        *first = Some(started.elapsed().as_millis());
                    }
                }
                writer
                    .write_all(&chunk)
                    .await
                    .map_err(|error| format!("could not write benchmark output: {error}"))?;
                let len = chunk.len() as u64;
                *sample_bytes.lock().await += len;
                *total_read.lock().await += len;
            }
            Ok(Some(Err(error))) => return Err(format!("range stream failed: {error}")),
            Ok(None) | Err(_) => break,
        }
    }
    writer
        .flush()
        .await
        .map_err(|error| format!("could not flush benchmark output: {error}"))?;
    Ok(())
}
