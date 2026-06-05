use futures_util::StreamExt;
use reqwest::header::ACCEPT_ENCODING;
use reqwest::Client;
use std::env;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};

const DEFAULT_DURATION: Duration = Duration::from_secs(20);

#[derive(Clone, Copy)]
struct Variant {
    label: &'static str,
    http1_only: bool,
    buffer_size: Option<usize>,
}

#[tokio::main]
async fn main() -> Result<(), String> {
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
        .join("http-stream-bench");
    fs::create_dir_all(&output_root)
        .await
        .map_err(|error| format!("Could not create stream benchmark directory: {error}"))?;

    let variants = [
        Variant {
            label: "default-network",
            http1_only: false,
            buffer_size: None,
        },
        Variant {
            label: "http1-network",
            http1_only: true,
            buffer_size: None,
        },
        Variant {
            label: "default-disk-512k",
            http1_only: false,
            buffer_size: Some(512 * 1024),
        },
        Variant {
            label: "default-disk-1m",
            http1_only: false,
            buffer_size: Some(1024 * 1024),
        },
        Variant {
            label: "default-disk-2m",
            http1_only: false,
            buffer_size: Some(2 * 1024 * 1024),
        },
        Variant {
            label: "default-disk-4m",
            http1_only: false,
            buffer_size: Some(4 * 1024 * 1024),
        },
        Variant {
            label: "http1-disk-512k",
            http1_only: true,
            buffer_size: Some(512 * 1024),
        },
        Variant {
            label: "http1-disk-4m",
            http1_only: true,
            buffer_size: Some(4 * 1024 * 1024),
        },
    ];

    for variant in variants {
        run_variant(&source, duration, &output_root, variant).await;
    }

    Ok(())
}

async fn run_variant(source: &str, duration: Duration, output_root: &PathBuf, variant: Variant) {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(120))
        .pool_idle_timeout(Some(Duration::from_secs(120)))
        .pool_max_idle_per_host(64)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .http2_adaptive_window(true)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .user_agent("SimpleDownloadManager/0.2");
    if variant.http1_only {
        builder = builder.http1_only();
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(error) => {
            println!("{} error=client: {}", variant.label, error);
            return;
        }
    };

    let started = Instant::now();
    let response = match client
        .get(source)
        .header(ACCEPT_ENCODING, "identity")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            println!("{} error=request: {}", variant.label, error);
            return;
        }
    };
    let version = format!("{:?}", response.version());
    let status = response.status();
    let mut stream = response.bytes_stream();
    let mut writer = match variant.buffer_size {
        Some(capacity) => {
            let path = output_root.join(format!("{}.part", variant.label));
            match fs::File::create(path).await {
                Ok(file) => Some(BufWriter::with_capacity(capacity, file)),
                Err(error) => {
                    println!("{} error=file: {}", variant.label, error);
                    return;
                }
            }
        }
        None => None,
    };

    let deadline = Instant::now() + duration;
    let mut total_bytes = 0_u64;
    let mut interval_bytes = 0_u64;
    let mut interval_started = Instant::now();
    let mut first_byte_ms = None;
    println!("{} status={} version={}", variant.label, status, version);

    while Instant::now() < deadline {
        match tokio::time::timeout_at(deadline.into(), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                if first_byte_ms.is_none() {
                    first_byte_ms = Some(started.elapsed().as_millis());
                }
                if let Some(writer) = writer.as_mut() {
                    if let Err(error) = writer.write_all(&chunk).await {
                        println!("{} error=write: {}", variant.label, error);
                        return;
                    }
                }
                total_bytes = total_bytes.saturating_add(chunk.len() as u64);
                interval_bytes = interval_bytes.saturating_add(chunk.len() as u64);
                let elapsed = interval_started.elapsed();
                if elapsed >= Duration::from_secs(1) {
                    println!(
                        "{} sample_mbps={:.1}",
                        variant.label,
                        interval_bytes as f64 * 8.0 / elapsed.as_secs_f64() / 1_000_000.0
                    );
                    interval_bytes = 0;
                    interval_started = Instant::now();
                }
            }
            Ok(Some(Err(error))) => {
                println!("{} error=stream: {}", variant.label, error);
                return;
            }
            Ok(None) | Err(_) => break,
        }
    }

    if let Some(writer) = writer.as_mut() {
        let _ = writer.flush().await;
    }
    println!(
        "{} average_mbps={:.1} first_byte_ms={}",
        variant.label,
        total_bytes as f64 * 8.0 / started.elapsed().as_secs_f64() / 1_000_000.0,
        first_byte_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".into())
    );
}
