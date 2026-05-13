use crate::storage::TorrentSettings;
use crate::torrent::{prepare_torrent_source, TorrentEngine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, timeout};

const BENCHMARK_SCHEMA_VERSION: u32 = 1;
const DEFAULT_BENCHMARK_DURATION: Duration = Duration::from_secs(180);
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const ARIA2_RPC_START_TIMEOUT: Duration = Duration::from_secs(10);
const LIBRQBIT_METADATA_TIMEOUT: Duration = Duration::from_secs(180);
const GATE_METADATA_MS: u64 = 30_000;
const GATE_FIRST_BYTE_MS: u64 = 60_000;
const GATE_ZERO_STALL_MS: u64 = 30_000;
const GATE_QBIT_SPEED_RATIO_NUMERATOR: u64 = 70;
const GATE_QBIT_SPEED_RATIO_DENOMINATOR: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkEngine {
    Librqbit,
    Aria2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkSpeedSample {
    pub elapsed_ms: u64,
    pub downloaded_bytes: u64,
    pub fetched_bytes: u64,
    pub download_bps: u64,
    pub connected_peers: Option<u32>,
    pub seeds: Option<u32>,
    pub dht_nodes: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BenchmarkSpeedAggregate {
    pub first_byte_time_ms: Option<u64>,
    pub average_download_bps_3m: u64,
    pub longest_zero_bps_stall_ms: u64,
    pub final_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkSourceSummary {
    pub source_redacted: String,
    pub source_kind: String,
    pub info_hash_prefix: Option<String>,
    pub original_tracker_count: usize,
    pub effective_tracker_count: usize,
    pub fallback_tracker_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkEngineReport {
    pub engine: BenchmarkEngine,
    pub metadata_time_ms: Option<u64>,
    pub first_byte_time_ms: Option<u64>,
    pub original_tracker_count: Option<usize>,
    pub effective_tracker_count: Option<usize>,
    pub fallback_tracker_count: Option<usize>,
    pub connected_peers: Option<u32>,
    pub seeds: Option<u32>,
    pub dht_nodes: Option<u32>,
    pub samples: Vec<BenchmarkSpeedSample>,
    pub average_download_bps_3m: u64,
    pub longest_zero_bps_stall_ms: u64,
    pub final_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QbittorrentReference {
    pub metadata_time_ms: Option<u64>,
    pub first_byte_time_ms: Option<u64>,
    pub average_download_bps_3m: Option<u64>,
    pub longest_zero_bps_stall_ms: Option<u64>,
    pub connected_peers: Option<u32>,
    pub seeds: Option<u32>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkDecisionAction {
    PlanAria2SidecarMigration,
    PlanLibtorrentHelperProcessSpike,
    KeepLibrqbitAndFixDiagnostics,
    InvestigateNetworkConfiguration,
    CollectQbittorrentReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkDecision {
    pub action: BenchmarkDecisionAction,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkReport {
    pub schema_version: u32,
    pub run_started_unix_ms: u64,
    pub duration_ms: u64,
    pub source: BenchmarkSourceSummary,
    pub qbittorrent_reference: Option<QbittorrentReference>,
    pub engines: Vec<BenchmarkEngineReport>,
    pub decision: BenchmarkDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aria2StatusSnapshot {
    pub status: String,
    pub total_length: Option<u64>,
    pub completed_length: u64,
    pub download_speed_bps: u64,
    pub connections: Option<u32>,
    pub seeders: Option<u32>,
    pub has_metadata: bool,
}

pub fn benchmark_source_summary(source: &str) -> BenchmarkSourceSummary {
    let prepared = prepare_torrent_source(source);
    let original_tracker_count = magnet_tracker_values(source).len();
    let effective_tracker_count = match prepared.source_kind {
        crate::torrent::TorrentSourceKind::Magnet => magnet_tracker_values(&prepared.source).len(),
        crate::torrent::TorrentSourceKind::TorrentFile => {
            prepared.fallback_trackers_for_options.len()
        }
    };
    let info_hash = magnet_info_hash(source);
    let info_hash_prefix = info_hash.as_ref().map(|hash| redacted_hash_prefix(hash));

    BenchmarkSourceSummary {
        source_redacted: redact_source(
            source,
            original_tracker_count,
            prepared.fallback_trackers_added,
        ),
        source_kind: prepared.source_kind.label().to_string(),
        info_hash_prefix,
        original_tracker_count,
        effective_tracker_count,
        fallback_tracker_count: prepared.fallback_trackers_added,
    }
}

pub fn aggregate_speed_samples(samples: &[BenchmarkSpeedSample]) -> BenchmarkSpeedAggregate {
    if samples.is_empty() {
        return BenchmarkSpeedAggregate::default();
    }

    let mut ordered = samples.to_vec();
    ordered.sort_by_key(|sample| sample.elapsed_ms);

    let final_bytes = ordered
        .iter()
        .map(sample_progress)
        .max()
        .unwrap_or_default();
    let first_byte_time_ms = ordered
        .iter()
        .find(|sample| sample_progress(sample) > 0)
        .map(|sample| sample.elapsed_ms);
    let longest_zero_bps_stall_ms = longest_no_progress_stall_ms(&ordered);
    let average_download_bps_3m = average_download_bps(&ordered, Duration::from_secs(180));

    BenchmarkSpeedAggregate {
        first_byte_time_ms,
        average_download_bps_3m,
        longest_zero_bps_stall_ms,
        final_bytes,
    }
}

pub fn parse_aria2_status_response(raw_json: &str) -> Result<Aria2StatusSnapshot, String> {
    let value: Value = serde_json::from_str(raw_json)
        .map_err(|error| format!("Could not parse aria2 JSON-RPC response: {error}"))?;
    parse_aria2_status_value(&value)
}

pub fn benchmark_decision(
    engines: &[BenchmarkEngineReport],
    qbittorrent_reference: Option<&QbittorrentReference>,
) -> BenchmarkDecision {
    let Some(qbit) = qbittorrent_reference else {
        return BenchmarkDecision {
            action: BenchmarkDecisionAction::CollectQbittorrentReference,
            reason: "qBittorrent reference was not provided for the same magnet.".into(),
        };
    };

    let aria2 = engines
        .iter()
        .find(|report| report.engine == BenchmarkEngine::Aria2);
    let librqbit = engines
        .iter()
        .find(|report| report.engine == BenchmarkEngine::Librqbit);

    if aria2.is_some_and(|report| engine_passes_decision_gate(report, qbit)) {
        return BenchmarkDecision {
            action: BenchmarkDecisionAction::PlanAria2SidecarMigration,
            reason:
                "aria2 met the metadata, first-byte, stall, and qBittorrent-relative speed gates."
                    .into(),
        };
    }

    if librqbit.is_some_and(|report| engine_passes_decision_gate(report, qbit)) {
        return BenchmarkDecision {
            action: BenchmarkDecisionAction::KeepLibrqbitAndFixDiagnostics,
            reason:
                "librqbit met the qBittorrent-relative benchmark gate; migration is not justified by this run."
                    .into(),
        };
    }

    if qbittorrent_succeeded(qbit) {
        return BenchmarkDecision {
            action: BenchmarkDecisionAction::PlanLibtorrentHelperProcessSpike,
            reason: "qBittorrent succeeded while the benchmarked non-qBittorrent engines did not meet the reliability gate."
                .into(),
        };
    }

    BenchmarkDecision {
        action: BenchmarkDecisionAction::InvestigateNetworkConfiguration,
        reason:
            "No engine met the gate; inspect port, DHT, tracker, firewall, and network configuration before migrating."
                .into(),
    }
}

pub fn redacted_report_json(report: &BenchmarkReport) -> Result<String, String> {
    serde_json::to_string_pretty(report)
        .map_err(|error| format!("Could not serialize torrent benchmark report: {error}"))
}

pub async fn run_benchmark_from_env() -> Result<BenchmarkReport, String> {
    let source = env::var("SDM_TORRENT_BENCH_MAGNET").map_err(|_| {
        "Set SDM_TORRENT_BENCH_MAGNET to a legal/user-authorized magnet.".to_string()
    })?;
    let duration = env::var("SDM_TORRENT_BENCH_DURATION_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_BENCHMARK_DURATION);
    let aria2c_path = env::var("ARIA2C_PATH").unwrap_or_else(|_| "aria2c".into());
    let qbittorrent_reference = load_qbittorrent_reference_from_env()?;
    let output_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("torrent-bench");

    run_benchmark(
        &source,
        qbittorrent_reference,
        &aria2c_path,
        duration,
        &output_root,
    )
    .await
}

pub async fn run_benchmark(
    source: &str,
    qbittorrent_reference: Option<QbittorrentReference>,
    aria2c_path: &str,
    duration: Duration,
    output_root: &Path,
) -> Result<BenchmarkReport, String> {
    let run_started_unix_ms = unix_millis();
    let run_dir = output_root.join(format!("run-{run_started_unix_ms}-{}", std::process::id()));
    fs::create_dir_all(&run_dir)
        .map_err(|error| format!("Could not create torrent benchmark directory: {error}"))?;

    let source_summary = benchmark_source_summary(source);
    let mut engines = Vec::with_capacity(2);
    engines.push(run_librqbit_benchmark(source, &source_summary, &run_dir, duration).await);
    engines
        .push(run_aria2_benchmark(source, &source_summary, aria2c_path, &run_dir, duration).await);
    let decision = benchmark_decision(&engines, qbittorrent_reference.as_ref());

    let report = BenchmarkReport {
        schema_version: BENCHMARK_SCHEMA_VERSION,
        run_started_unix_ms,
        duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
        source: source_summary,
        qbittorrent_reference,
        engines,
        decision,
    };
    let report_json = redacted_report_json(&report)?;
    fs::write(run_dir.join("report.json"), report_json)
        .map_err(|error| format!("Could not write torrent benchmark report: {error}"))?;

    Ok(report)
}

async fn run_librqbit_benchmark(
    source: &str,
    source_summary: &BenchmarkSourceSummary,
    run_dir: &Path,
    duration: Duration,
) -> BenchmarkEngineReport {
    let mut report = blank_engine_report(BenchmarkEngine::Librqbit, source_summary);
    let engine_root = run_dir.join("librqbit");
    let output_dir = engine_root.join("files");
    let data_dir = engine_root.join("data");
    let started = Instant::now();

    let engine =
        match TorrentEngine::new(output_dir.clone(), data_dir, TorrentSettings::default()).await {
            Ok(engine) => engine,
            Err(error) => {
                report.error = Some(error);
                return report;
            }
        };

    let prepared = prepare_torrent_source(source);
    let add_timeout = duration.max(LIBRQBIT_METADATA_TIMEOUT);
    let add_result = timeout(
        add_timeout,
        engine.add_source(&prepared, &output_dir, 0, false, None),
    )
    .await;
    let add_outcome = match add_result {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => {
            report.metadata_time_ms = Some(elapsed_ms(started));
            report.error = Some(error);
            return report;
        }
        Err(_) => {
            report.metadata_time_ms = Some(elapsed_ms(started));
            report.error = Some(format!(
                "librqbit metadata lookup did not complete within {} seconds",
                add_timeout.as_secs()
            ));
            return report;
        }
    };

    report.metadata_time_ms = Some(elapsed_ms(started));
    let samples = collect_librqbit_samples(&engine, add_outcome.engine_id, started, duration).await;
    let forget_result = engine.forget(add_outcome.engine_id).await;

    apply_sample_aggregate(&mut report, samples);
    if let Err(error) = forget_result {
        report.error = Some(match report.error.take() {
            Some(existing) => format!("{existing}; cleanup failed: {error}"),
            None => format!("cleanup failed: {error}"),
        });
    }

    report
}

async fn collect_librqbit_samples(
    engine: &TorrentEngine,
    engine_id: usize,
    started: Instant,
    duration: Duration,
) -> Vec<BenchmarkSpeedSample> {
    let sample_started = Instant::now();
    let mut samples = Vec::new();

    while sample_started.elapsed() <= duration {
        if let Ok(snapshot) = engine.snapshot(engine_id).await {
            let diagnostics = snapshot.diagnostics.as_ref();
            samples.push(BenchmarkSpeedSample {
                elapsed_ms: elapsed_ms(started),
                downloaded_bytes: snapshot.downloaded_bytes,
                fetched_bytes: snapshot.fetched_bytes.max(snapshot.downloaded_bytes),
                download_bps: snapshot.download_speed,
                connected_peers: snapshot
                    .peers
                    .or_else(|| diagnostics.map(|diagnostics| diagnostics.live_peers)),
                seeds: snapshot.seeds,
                dht_nodes: None,
            });
        }
        sleep(SAMPLE_INTERVAL).await;
    }

    samples
}

async fn run_aria2_benchmark(
    source: &str,
    source_summary: &BenchmarkSourceSummary,
    aria2c_path: &str,
    run_dir: &Path,
    duration: Duration,
) -> BenchmarkEngineReport {
    let mut report = blank_engine_report(BenchmarkEngine::Aria2, source_summary);
    let engine_root = run_dir.join("aria2");
    let output_dir = engine_root.join("files");
    let session_dir = engine_root.join("session");
    if let Err(error) =
        fs::create_dir_all(&output_dir).and_then(|_| fs::create_dir_all(&session_dir))
    {
        report.error = Some(format!(
            "Could not create aria2 benchmark directory: {error}"
        ));
        return report;
    }

    let started = Instant::now();
    let mut daemon = match Aria2Daemon::start(aria2c_path, &output_dir, &session_dir) {
        Ok(daemon) => daemon,
        Err(error) => {
            report.error = Some(error);
            return report;
        }
    };
    let client = reqwest::Client::new();
    if let Err(error) = daemon.wait_until_ready(&client).await {
        report.error = Some(error);
        return report;
    }

    let gid = match aria2_rpc(
        &client,
        daemon.rpc_url(),
        "addUri",
        json!([
            [source],
            {
                "dir": output_dir.to_string_lossy(),
                "seed-time": "0"
            }
        ]),
    )
    .await
    .and_then(|response| {
        response
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "aria2 addUri response did not include a gid".to_string())
    }) {
        Ok(gid) => gid,
        Err(error) => {
            report.error = Some(error);
            return report;
        }
    };

    let (samples, metadata_time_ms) =
        collect_aria2_samples(&client, daemon.rpc_url(), &gid, started, duration).await;
    let _ = aria2_rpc(&client, daemon.rpc_url(), "forceRemove", json!([gid])).await;
    let _ = aria2_rpc(&client, daemon.rpc_url(), "shutdown", json!([])).await;
    daemon.mark_shutdown_requested();

    apply_sample_aggregate(&mut report, samples);
    report.metadata_time_ms = metadata_time_ms;
    report
}

async fn collect_aria2_samples(
    client: &reqwest::Client,
    rpc_url: &str,
    gid: &str,
    started: Instant,
    duration: Duration,
) -> (Vec<BenchmarkSpeedSample>, Option<u64>) {
    let sample_started = Instant::now();
    let mut samples = Vec::new();
    let mut metadata_time_seen = None;

    while sample_started.elapsed() <= duration {
        let response = aria2_rpc(
            client,
            rpc_url,
            "tellStatus",
            json!([
                gid,
                [
                    "status",
                    "totalLength",
                    "completedLength",
                    "downloadSpeed",
                    "connections",
                    "numSeeders",
                    "bittorrent"
                ]
            ]),
        )
        .await;
        if let Ok(value) = response {
            if let Ok(status) = parse_aria2_status_value(&value) {
                if status.has_metadata && metadata_time_seen.is_none() {
                    metadata_time_seen = Some(elapsed_ms(started));
                }
                samples.push(BenchmarkSpeedSample {
                    elapsed_ms: elapsed_ms(started),
                    downloaded_bytes: status.completed_length,
                    fetched_bytes: status.completed_length,
                    download_bps: status.download_speed_bps,
                    connected_peers: status.connections,
                    seeds: status.seeders,
                    dht_nodes: None,
                });
                if matches!(status.status.as_str(), "complete" | "removed" | "error") {
                    break;
                }
            }
        }
        sleep(SAMPLE_INTERVAL).await;
    }

    (samples, metadata_time_seen)
}

fn blank_engine_report(
    engine: BenchmarkEngine,
    source_summary: &BenchmarkSourceSummary,
) -> BenchmarkEngineReport {
    BenchmarkEngineReport {
        engine,
        metadata_time_ms: None,
        first_byte_time_ms: None,
        original_tracker_count: Some(source_summary.original_tracker_count),
        effective_tracker_count: Some(source_summary.effective_tracker_count),
        fallback_tracker_count: Some(source_summary.fallback_tracker_count),
        connected_peers: None,
        seeds: None,
        dht_nodes: None,
        samples: Vec::new(),
        average_download_bps_3m: 0,
        longest_zero_bps_stall_ms: 0,
        final_bytes: 0,
        error: None,
    }
}

fn apply_sample_aggregate(report: &mut BenchmarkEngineReport, samples: Vec<BenchmarkSpeedSample>) {
    let aggregate = aggregate_speed_samples(&samples);
    report.first_byte_time_ms = aggregate.first_byte_time_ms;
    report.average_download_bps_3m = aggregate.average_download_bps_3m;
    report.longest_zero_bps_stall_ms = aggregate.longest_zero_bps_stall_ms;
    report.final_bytes = aggregate.final_bytes;
    report.connected_peers = samples
        .iter()
        .rev()
        .find_map(|sample| sample.connected_peers);
    report.seeds = samples.iter().rev().find_map(|sample| sample.seeds);
    report.dht_nodes = samples.iter().rev().find_map(|sample| sample.dht_nodes);
    report.samples = samples;
}

fn engine_passes_decision_gate(
    report: &BenchmarkEngineReport,
    qbit: &QbittorrentReference,
) -> bool {
    if report.error.is_some() {
        return false;
    }
    if report
        .metadata_time_ms
        .is_none_or(|metadata_ms| metadata_ms > GATE_METADATA_MS)
    {
        return false;
    }
    if report
        .first_byte_time_ms
        .is_none_or(|first_byte_ms| first_byte_ms > GATE_FIRST_BYTE_MS)
    {
        return false;
    }
    if report.longest_zero_bps_stall_ms > GATE_ZERO_STALL_MS {
        return false;
    }
    let Some(qbit_average_bps) = qbit.average_download_bps_3m.filter(|value| *value > 0) else {
        return false;
    };
    let required_bps = qbit_average_bps.saturating_mul(GATE_QBIT_SPEED_RATIO_NUMERATOR)
        / GATE_QBIT_SPEED_RATIO_DENOMINATOR;

    report.average_download_bps_3m >= required_bps
}

fn qbittorrent_succeeded(qbit: &QbittorrentReference) -> bool {
    qbit.average_download_bps_3m.is_some_and(|speed| speed > 0) || qbit.first_byte_time_ms.is_some()
}

fn parse_aria2_status_value(value: &Value) -> Result<Aria2StatusSnapshot, String> {
    if let Some(error) = value.get("error") {
        return Err(format!("aria2 JSON-RPC error: {error}"));
    }
    let result = value
        .get("result")
        .and_then(Value::as_object)
        .ok_or_else(|| "aria2 status response did not include a result object".to_string())?;

    let status = result
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let total_length = parse_u64_field(result.get("totalLength"));
    let completed_length = parse_u64_field(result.get("completedLength")).unwrap_or(0);
    let download_speed_bps = parse_u64_field(result.get("downloadSpeed")).unwrap_or(0);
    let connections = parse_u64_field(result.get("connections")).map(saturating_u32);
    let seeders = parse_u64_field(result.get("numSeeders")).map(saturating_u32);
    let has_metadata = result
        .get("bittorrent")
        .and_then(|bittorrent| bittorrent.get("info"))
        .and_then(|info| info.get("name"))
        .and_then(Value::as_str)
        .is_some_and(|name| !name.is_empty())
        || total_length.is_some_and(|length| length > 0);

    Ok(Aria2StatusSnapshot {
        status,
        total_length,
        completed_length,
        download_speed_bps,
        connections,
        seeders,
        has_metadata,
    })
}

async fn aria2_rpc(
    client: &reqwest::Client,
    rpc_url: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": "sdm-bench",
        "method": format!("aria2.{method}"),
        "params": params,
    });
    let response = client
        .post(rpc_url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|error| format!("aria2 JSON-RPC request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("aria2 JSON-RPC returned an HTTP error: {error}"))?
        .text()
        .await
        .map_err(|error| format!("Could not read aria2 JSON-RPC response: {error}"))?;

    serde_json::from_str(&response)
        .map_err(|error| format!("Could not parse aria2 JSON-RPC response: {error}"))
}

struct Aria2Daemon {
    child: Child,
    rpc_url: String,
    shutdown_requested: bool,
}

impl Aria2Daemon {
    fn start(aria2c_path: &str, output_dir: &Path, session_dir: &Path) -> Result<Self, String> {
        let port = allocate_loopback_port()?;
        let rpc_url = format!("http://127.0.0.1:{port}/jsonrpc");
        let child = Command::new(aria2c_path)
            .arg("--enable-rpc=true")
            .arg("--rpc-listen-all=false")
            .arg(format!("--rpc-listen-port={port}"))
            .arg("--summary-interval=0")
            .arg("--console-log-level=warn")
            .arg("--enable-dht=true")
            .arg("--enable-peer-exchange=true")
            .arg("--bt-enable-lpd=true")
            .arg("--seed-time=0")
            .arg(format!("--dir={}", output_dir.display()))
            .arg(format!(
                "--save-session={}",
                session_dir.join("aria2.session").display()
            ))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                format!(
                    "Could not start aria2c at '{}'. Set ARIA2C_PATH if it is not on PATH: {error}",
                    aria2c_path
                )
            })?;

        Ok(Self {
            child,
            rpc_url,
            shutdown_requested: false,
        })
    }

    fn rpc_url(&self) -> &str {
        &self.rpc_url
    }

    fn mark_shutdown_requested(&mut self) {
        self.shutdown_requested = true;
    }

    async fn wait_until_ready(&mut self, client: &reqwest::Client) -> Result<(), String> {
        let started = Instant::now();
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(format!("aria2c exited before JSON-RPC was ready: {status}"));
            }
            if aria2_rpc(client, self.rpc_url(), "getVersion", json!([]))
                .await
                .is_ok()
            {
                return Ok(());
            }
            if started.elapsed() >= ARIA2_RPC_START_TIMEOUT {
                return Err("aria2c JSON-RPC did not become ready within 10 seconds".into());
            }
            sleep(Duration::from_millis(200)).await;
        }
    }
}

impl Drop for Aria2Daemon {
    fn drop(&mut self) {
        if !self.shutdown_requested {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

fn load_qbittorrent_reference_from_env() -> Result<Option<QbittorrentReference>, String> {
    let Some(path) = env::var("SDM_TORRENT_BENCH_QBITTORRENT_JSON")
        .ok()
        .filter(|path| !path.trim().is_empty())
    else {
        return Ok(None);
    };
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read qBittorrent reference JSON '{path}': {error}"))?;
    serde_json::from_str(&contents)
        .map(Some)
        .map_err(|error| format!("Could not parse qBittorrent reference JSON '{path}': {error}"))
}

fn sample_progress(sample: &BenchmarkSpeedSample) -> u64 {
    sample.fetched_bytes.max(sample.downloaded_bytes)
}

fn longest_no_progress_stall_ms(samples: &[BenchmarkSpeedSample]) -> u64 {
    let Some(first) = samples.first() else {
        return 0;
    };
    let Some(last) = samples.last() else {
        return 0;
    };

    let mut longest = 0_u64;
    let mut stall_start = Some(first.elapsed_ms);
    let mut last_elapsed = first.elapsed_ms;
    let mut last_progress = sample_progress(first);

    for sample in samples.iter().skip(1) {
        let progress = sample_progress(sample);
        if progress > last_progress {
            if let Some(start) = stall_start.take() {
                longest = longest.max(sample.elapsed_ms.saturating_sub(start));
            }
            last_progress = progress;
        } else if stall_start.is_none() {
            stall_start = Some(last_elapsed);
        }
        last_elapsed = sample.elapsed_ms;
    }

    if let Some(start) = stall_start {
        longest = longest.max(last.elapsed_ms.saturating_sub(start));
    }

    longest
}

fn average_download_bps(samples: &[BenchmarkSpeedSample], window: Duration) -> u64 {
    let Some(last) = samples.last() else {
        return 0;
    };
    let window_start = last
        .elapsed_ms
        .saturating_sub(window.as_millis().min(u128::from(u64::MAX)) as u64);
    let first_in_window = samples
        .iter()
        .find(|sample| sample.elapsed_ms >= window_start)
        .unwrap_or(&samples[0]);
    let elapsed_ms = last.elapsed_ms.saturating_sub(first_in_window.elapsed_ms);
    if elapsed_ms == 0 {
        return 0;
    }
    let byte_delta = sample_progress(last).saturating_sub(sample_progress(first_in_window));
    byte_delta.saturating_mul(1000) / elapsed_ms
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn allocate_loopback_port() -> Result<u16, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("Could not allocate local aria2 JSON-RPC port: {error}"))?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| format!("Could not inspect local aria2 JSON-RPC port: {error}"))
}

fn parse_u64_field(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse::<u64>().ok(),
        _ => None,
    }
}

fn saturating_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn magnet_tracker_values(source: &str) -> Vec<String> {
    url::Url::parse(source)
        .ok()
        .map(|url| {
            url.query_pairs()
                .filter_map(|(key, value)| (key == "tr").then(|| value.into_owned()))
                .collect()
        })
        .unwrap_or_default()
}

fn magnet_info_hash(source: &str) -> Option<String> {
    let parsed = url::Url::parse(source).ok()?;
    parsed.query_pairs().find_map(|(key, value)| {
        if !key.eq_ignore_ascii_case("xt") {
            return None;
        }
        let value = value.into_owned();
        let (prefix, hash) = value.split_at(value.len().min("urn:btih:".len()));
        if !prefix.eq_ignore_ascii_case("urn:btih:") || hash.is_empty() {
            return None;
        }
        Some(hash.to_string())
    })
}

fn redact_source(source: &str, original_trackers: usize, fallback_trackers: usize) -> String {
    if source
        .get(..source.len().min("magnet:".len()))
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("magnet:"))
    {
        let hash = magnet_info_hash(source)
            .map(|hash| redacted_hash_prefix(&hash))
            .unwrap_or_else(|| "unknown".into());
        return format!("magnet:?xt=urn:btih:{hash}&tr={original_trackers}+{fallback_trackers}");
    }

    let filename = Path::new(source)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("torrent-file");
    format!("torrent-file:{filename}")
}

fn redacted_hash_prefix(hash: &str) -> String {
    let prefix_len = hash.len().min(12);
    format!("{}...", &hash[..prefix_len])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::torrent::FALLBACK_TORRENT_TRACKERS;

    const TEST_MAGNET: &str = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Private%20Name&tr=udp%3A%2F%2Ftracker.example%3A1337%2Fannounce";

    fn sample(elapsed_ms: u64, fetched_bytes: u64, download_bps: u64) -> BenchmarkSpeedSample {
        BenchmarkSpeedSample {
            elapsed_ms,
            downloaded_bytes: fetched_bytes,
            fetched_bytes,
            download_bps,
            connected_peers: Some(6),
            seeds: Some(2),
            dht_nodes: None,
        }
    }

    fn engine_report(
        engine: BenchmarkEngine,
        metadata_time_ms: Option<u64>,
        first_byte_time_ms: Option<u64>,
        average_download_bps_3m: u64,
        longest_zero_bps_stall_ms: u64,
        error: Option<&str>,
    ) -> BenchmarkEngineReport {
        BenchmarkEngineReport {
            engine,
            metadata_time_ms,
            first_byte_time_ms,
            original_tracker_count: Some(1),
            effective_tracker_count: Some(1 + FALLBACK_TORRENT_TRACKERS.len()),
            fallback_tracker_count: Some(FALLBACK_TORRENT_TRACKERS.len()),
            connected_peers: Some(8),
            seeds: Some(4),
            dht_nodes: Some(120),
            samples: Vec::new(),
            average_download_bps_3m,
            longest_zero_bps_stall_ms,
            final_bytes: 512 * 1024,
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn source_summary_distinguishes_original_and_appended_trackers() {
        let summary = benchmark_source_summary(TEST_MAGNET);

        assert_eq!(summary.original_tracker_count, 1);
        assert_eq!(
            summary.effective_tracker_count,
            1 + FALLBACK_TORRENT_TRACKERS.len()
        );
        assert_eq!(
            summary.fallback_tracker_count,
            FALLBACK_TORRENT_TRACKERS.len()
        );
        assert!(summary.source_redacted.contains("0123456789ab..."));
        assert!(summary.source_redacted.contains("tr=1+8"));
        assert!(!summary.source_redacted.contains("Private"));
        assert!(!summary.source_redacted.contains("tracker.example"));
    }

    #[test]
    fn sample_aggregation_uses_fetched_byte_progress_for_stall_windows() {
        let samples = vec![
            sample(0, 0, 0),
            sample(20_000, 0, 1024),
            sample(60_000, 0, 0),
            sample(90_000, 90_000, 0),
            sample(120_000, 120_000, 2048),
        ];

        let aggregate = aggregate_speed_samples(&samples);

        assert_eq!(aggregate.first_byte_time_ms, Some(90_000));
        assert_eq!(aggregate.longest_zero_bps_stall_ms, 90_000);
        assert_eq!(aggregate.average_download_bps_3m, 1_000);
        assert_eq!(aggregate.final_bytes, 120_000);
    }

    #[test]
    fn aria2_status_parser_accepts_json_rpc_string_numbers() {
        let status = parse_aria2_status_response(
            r#"{
                "jsonrpc": "2.0",
                "id": "sdm-bench",
                "result": {
                    "gid": "2089b05ecca3d829",
                    "status": "active",
                    "totalLength": "1536000",
                    "completedLength": "245760",
                    "downloadSpeed": "81920",
                    "connections": "7",
                    "numSeeders": "13",
                    "bittorrent": {
                        "info": {
                            "name": "Example"
                        }
                    }
                }
            }"#,
        )
        .expect("aria2 status should parse");

        assert_eq!(status.status, "active");
        assert_eq!(status.total_length, Some(1_536_000));
        assert_eq!(status.completed_length, 245_760);
        assert_eq!(status.download_speed_bps, 81_920);
        assert_eq!(status.connections, Some(7));
        assert_eq!(status.seeders, Some(13));
        assert!(status.has_metadata);
    }

    #[test]
    fn decision_gate_prefers_aria2_when_it_matches_qbittorrent_reference() {
        let qbit = QbittorrentReference {
            metadata_time_ms: Some(12_000),
            first_byte_time_ms: Some(24_000),
            average_download_bps_3m: Some(3_000_000),
            longest_zero_bps_stall_ms: Some(8_000),
            connected_peers: Some(20),
            seeds: Some(20),
            notes: None,
        };
        let librqbit = engine_report(
            BenchmarkEngine::Librqbit,
            Some(55_000),
            Some(95_000),
            600_000,
            70_000,
            None,
        );
        let aria2 = engine_report(
            BenchmarkEngine::Aria2,
            Some(10_000),
            Some(22_000),
            2_200_000,
            20_000,
            None,
        );

        let decision = benchmark_decision(&[librqbit, aria2], Some(&qbit));

        assert_eq!(
            decision.action,
            BenchmarkDecisionAction::PlanAria2SidecarMigration
        );
        assert!(decision.reason.contains("aria2"));
    }

    #[test]
    fn decision_gate_keeps_librqbit_when_it_passes_and_aria2_does_not() {
        let qbit = QbittorrentReference {
            metadata_time_ms: Some(8_000),
            first_byte_time_ms: Some(18_000),
            average_download_bps_3m: Some(2_000_000),
            longest_zero_bps_stall_ms: Some(6_000),
            connected_peers: Some(10),
            seeds: Some(8),
            notes: None,
        };
        let librqbit = engine_report(
            BenchmarkEngine::Librqbit,
            Some(10_000),
            Some(28_000),
            1_600_000,
            15_000,
            None,
        );
        let aria2 = engine_report(
            BenchmarkEngine::Aria2,
            None,
            None,
            0,
            180_000,
            Some("aria2c failed"),
        );

        let decision = benchmark_decision(&[librqbit, aria2], Some(&qbit));

        assert_eq!(
            decision.action,
            BenchmarkDecisionAction::KeepLibrqbitAndFixDiagnostics
        );
    }

    #[test]
    fn redacted_artifact_output_does_not_include_raw_magnet_or_tracker_urls() {
        let summary = benchmark_source_summary(TEST_MAGNET);
        let report = BenchmarkReport {
            schema_version: 1,
            run_started_unix_ms: 1_700_000_000_000,
            duration_ms: 180_000,
            source: summary,
            qbittorrent_reference: None,
            engines: vec![engine_report(
                BenchmarkEngine::Aria2,
                Some(10_000),
                Some(20_000),
                1_000_000,
                5_000,
                None,
            )],
            decision: BenchmarkDecision {
                action: BenchmarkDecisionAction::CollectQbittorrentReference,
                reason: "qBittorrent reference was not provided".into(),
            },
        };

        let json = redacted_report_json(&report).expect("report should serialize");

        assert!(!json.contains(TEST_MAGNET));
        assert!(!json.contains("tracker.example"));
        assert!(!json.contains("Private%20Name"));
        assert!(json.contains("sourceRedacted"));
        assert!(json.contains("0123456789ab..."));
    }

    #[test]
    fn qbittorrent_reference_example_uses_supported_json_shape() {
        let reference: QbittorrentReference =
            serde_json::from_str(include_str!("../torrent-bench.qbittorrent.example.json"))
                .expect("example qBittorrent reference JSON should parse");

        assert_eq!(reference.metadata_time_ms, Some(12_000));
        assert_eq!(reference.first_byte_time_ms, Some(24_000));
        assert_eq!(reference.average_download_bps_3m, Some(3_000_000));
        assert_eq!(reference.longest_zero_bps_stall_ms, Some(8_000));
    }

    #[tokio::test]
    #[ignore = "requires SDM_TORRENT_BENCH_MAGNET and optional ARIA2C_PATH"]
    async fn live_torrent_benchmark_from_env() {
        std::env::var("SDM_TORRENT_BENCH_MAGNET")
            .expect("set SDM_TORRENT_BENCH_MAGNET to a legal/user-authorized magnet");

        let report = run_benchmark_from_env()
            .await
            .expect("live torrent benchmark should run");

        assert!(!report.engines.is_empty());
    }
}
