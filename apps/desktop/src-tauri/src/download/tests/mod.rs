use super::*;
use crate::storage::{
    BulkArchiveOutputKind, BulkHosterAccelerationMode, DownloadJob, HandoffAuth, HandoffAuthHeader,
    JobState, TorrentInfo, TorrentPeerConnectionWatchdogMode,
};
use std::future::pending;
use std::sync::{Mutex as TestMutex, MutexGuard as TestMutexGuard, OnceLock as TestOnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[path = "recovery.rs"]
mod recovery;
#[path = "scenarios.rs"]
mod scenarios;

#[path = "bulk_finalize.rs"]
mod bulk_finalize;
#[path = "http.rs"]
mod http;
#[path = "segmented.rs"]
mod segmented;
#[path = "torrent_metadata.rs"]
mod torrent_metadata;

static SEGMENT_HOST_SCORE_TEST_LOCK: TestOnceLock<TestMutex<()>> = TestOnceLock::new();

fn segment_host_score_test_guard() -> TestMutexGuard<'static, ()> {
    SEGMENT_HOST_SCORE_TEST_LOCK
        .get_or_init(|| TestMutex::new(()))
        .lock()
        .unwrap()
}

fn torrent_runtime_update(
    uploaded_bytes: u64,
    downloaded_bytes: u64,
    download_speed: u64,
) -> TorrentRuntimeSnapshot {
    TorrentRuntimeSnapshot {
        engine_id: 42,
        info_hash: "420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        name: Some("Ubuntu Desktop".into()),
        total_files: Some(1),
        peers: Some(TORRENT_LOW_THROUGHPUT_LIVE_PEER_THRESHOLD),
        seeds: None,
        downloaded_bytes,
        total_bytes: downloaded_bytes.saturating_mul(2),
        uploaded_bytes,
        fetched_bytes: downloaded_bytes,
        download_speed,
        upload_speed: 0,
        eta: None,
        phase: TorrentRuntimePhase::Live,
        finished: false,
        error: None,
        diagnostics: None,
    }
}

#[tokio::test]
async fn response_body_decode_errors_are_retryable_network_failures() {
    let response = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nbad\r\n0\r\n\r\n";
    let (url, _request_handle) = spawn_one_response_server(response).await;
    let client = Client::builder().redirect(Policy::none()).build().unwrap();
    let response = client.get(&url).send().await.unwrap();
    let error = response
        .bytes()
        .await
        .expect_err("reqwest should reject malformed transfer-encoding bodies");

    let classified = download_stream_error(error);

    assert_eq!(classified.category, FailureCategory::Network);
    assert!(classified.retryable);
}

#[tokio::test]
async fn async_bulk_finalization_planning_counts_sources_before_prepare() {
    let root = test_download_runtime_dir("bulk-finalization-plan-async");
    let first = archive_test_entry(&root, "first.bin", b"first");
    let second = archive_test_entry(&root, "second.bin", b"second");
    let archive = BulkArchiveReady {
        archive_id: "bulk_async_plan".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        output_path: root.join("Bundle"),
        entries: vec![first, second],
    };

    let plan = plan_bulk_archive_finalization(archive)
        .await
        .expect("async bulk plan should be built before source preparation");

    assert_eq!(plan.total_completed_bytes, 11);
    assert_eq!(plan.output_kind, BulkArchiveOutputKind::Folder);
    assert_eq!(plan.finalize_mode, BulkFinalizeMode::Move);
    assert!(!plan.requires_extraction);

    let _ = std::fs::remove_dir_all(root);
}

fn low_throughput_update() -> crate::state::TorrentRuntimeSnapshot {
    crate::state::TorrentRuntimeSnapshot {
        engine_id: 42,
        info_hash: "420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        name: Some("Ubuntu".into()),
        total_files: Some(1),
        peers: Some(12),
        seeds: None,
        downloaded_bytes: 1024,
        total_bytes: 10 * 1024 * 1024,
        uploaded_bytes: 0,
        fetched_bytes: 1024,
        download_speed: 32 * 1024,
        upload_speed: 0,
        eta: None,
        phase: crate::state::TorrentRuntimePhase::Live,
        finished: false,
        error: None,
        diagnostics: Some(crate::storage::TorrentRuntimeDiagnostics {
            queued_peers: 4,
            connecting_peers: 3,
            live_peers: 12,
            seen_peers: 120,
            dead_peers: 40,
            not_needed_peers: 0,
            contributing_peers: 1,
            peer_errors: 18,
            peers_with_errors: 6,
            peer_connection_attempts: 24,
            session_download_speed: 32 * 1024,
            session_upload_speed: 0,
            dht_nodes: Some(80),
            average_piece_download_millis: None,
            listen_port: Some(42000),
            listener_fallback: false,
            peer_samples: Vec::new(),
            ..Default::default()
        }),
    }
}

fn low_throughput_update_before_first_payload() -> crate::state::TorrentRuntimeSnapshot {
    let mut update = low_throughput_update();
    update.downloaded_bytes = 0;
    update.fetched_bytes = 0;
    update.download_speed = 0;
    if let Some(diagnostics) = update.diagnostics.as_mut() {
        diagnostics.live_peers = 0;
        diagnostics.connecting_peers = 0;
        diagnostics.contributing_peers = 0;
        diagnostics.session_download_speed = 0;
    }
    update
}

fn torrent_job(id: &str, state: JobState) -> DownloadJob {
    DownloadJob {
        id: id.into(),
        url: format!("magnet:?xt=urn:btih:{id}"),
        filename: format!("torrent-{id}"),
        source: None,
        transfer_kind: TransferKind::Torrent,
        integrity_check: None,
        torrent: Some(TorrentInfo::default()),
        state,
        removal_state: None,
        created_at: 1,
        progress: 0.0,
        total_bytes: 0,
        downloaded_bytes: 0,
        speed: 0,
        eta: 0,
        active_segments: None,
        planned_segments: None,
        error: None,
        failure_category: None,
        resume_support: ResumeSupport::Unknown,
        retry_attempts: 0,
        auto_restart_attempts: 0,
        resolved_from_url: None,
        hoster_preflight: None,
        target_path: format!("C:/Downloads/torrent-{id}"),
        temp_path: format!("C:/Downloads/torrent-{id}.part"),
        artifact_exists: None,
        bulk_archive: None,
    }
}

async fn torrent_engine_state_for_test(
    storage_name: &str,
    root: &Path,
    jobs: Vec<DownloadJob>,
    configure: impl FnOnce(&mut crate::storage::Settings),
) -> SharedState {
    let state = SharedState::for_tests(test_storage_path(storage_name), jobs);
    let mut settings = crate::storage::Settings {
        download_directory: root.join("downloads").display().to_string(),
        torrent: crate::storage::TorrentSettings {
            download_directory: root.join("torrents").display().to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    configure(&mut settings);
    state.save_settings(settings).await.unwrap();
    state
}

fn test_storage_path(name: &str) -> PathBuf {
    let dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("state.json")
}

#[tokio::test]
async fn stream_wait_observes_canceled_control_before_next_chunk_arrives() {
    let job = DownloadJob {
        id: "job_cancel_wait".into(),
        url: "https://example.com/file.bin".into(),
        filename: "file.bin".into(),
        source: None,
        transfer_kind: TransferKind::Http,
        integrity_check: None,
        torrent: None,
        state: JobState::Canceled,
        removal_state: None,
        created_at: 1,
        progress: 0.0,
        total_bytes: 0,
        downloaded_bytes: 0,
        speed: 0,
        eta: 0,
        active_segments: None,
        planned_segments: None,
        error: None,
        failure_category: None,
        resume_support: ResumeSupport::Unknown,
        retry_attempts: 0,
        auto_restart_attempts: 0,
        resolved_from_url: None,
        hoster_preflight: None,
        target_path: "C:/Downloads/file.bin".into(),
        temp_path: test_storage_path("stream-wait-cancel-part")
            .display()
            .to_string(),
        artifact_exists: None,
        bulk_archive: None,
    };
    let state = SharedState::for_tests(test_storage_path("stream-wait-cancel-state"), vec![job]);
    let result = next_stream_item_with_control(
        &state,
        "job_cancel_wait",
        None,
        std::future::pending::<Option<Result<(), ()>>>(),
    )
    .await;

    assert!(matches!(
        result,
        StreamItemWait::Interrupted(DownloadOutcome::Canceled)
    ));
}

#[test]
fn stale_inflight_hoster_warmups_can_be_replaced() {
    clear_hoster_warmup_cache_for_tests();
    let key = hoster_warmup_key_for_tests("job_warmup", "https://datanodes.to/abc123/file.bin");
    let now = Instant::now();

    assert!(mark_hoster_warmup_inflight_for_tests(&key, now));
    assert!(!mark_hoster_warmup_inflight_for_tests(
        &key,
        now + HOSTER_WARMUP_INFLIGHT_TTL / 2
    ));
    assert!(mark_hoster_warmup_inflight_for_tests(
        &key,
        now + HOSTER_WARMUP_INFLIGHT_TTL + Duration::from_secs(1)
    ));
}

#[test]
fn scheduler_uses_optional_snapshot_claims() {
    let source = include_str!("../mod.rs");
    let scheduler_function = source
        .split("pub fn schedule_downloads")
        .nth(1)
        .expect("scheduler entrypoint should exist");

    assert!(
        scheduler_function.contains("if !state.request_scheduler_wake()"),
        "production scheduler wakeups should use the per-state wake guard before spawning"
    );
    assert!(
        scheduler_function.contains("run_scheduler_loop(app, state).await"),
        "production scheduler wakeups should delegate repeated passes to the guarded runner"
    );
    assert!(
        scheduler_function.contains("claim_schedulable_jobs_for_scheduler().await"),
        "production scheduler wakeups should use the optional-snapshot claim path"
    );
    assert!(
        !scheduler_function.contains("claim_schedulable_jobs().await"),
        "production scheduler wakeups should not force full snapshots on no-op claims"
    );
}

#[test]
fn scheduler_runner_loops_while_pending_wakes_exist() {
    let source = include_str!("../mod.rs");
    let scheduler_loop = source
        .split("async fn run_scheduler_loop")
        .nth(1)
        .expect("scheduler loop helper should exist");

    assert!(
        scheduler_loop.contains("loop {"),
        "scheduler runner should loop instead of spawning one task per wake"
    );
    assert!(
        scheduler_loop.contains("run_scheduler_once(&app, &state).await"),
        "scheduler runner should preserve the existing single-pass scheduling behavior"
    );
    assert!(
        scheduler_loop.contains("if !state.complete_scheduler_run()"),
        "scheduler runner should release or continue through the per-state wake guard"
    );
}

#[test]
fn torrent_low_throughput_monitor_reports_after_sustained_slow_live_peers() {
    let now = Instant::now();
    let mut monitor = TorrentLowThroughputMonitor::default();
    let mut update = torrent_runtime_update(1024, 4096, 32);
    update.download_speed = TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND - 1;
    update.diagnostics = Some(crate::storage::TorrentRuntimeDiagnostics {
        live_peers: TORRENT_LOW_THROUGHPUT_LIVE_PEER_THRESHOLD,
        seen_peers: 25,
        contributing_peers: 2,
        peer_errors: 1,
        session_download_speed: 64 * 1024,
        listen_port: Some(42000),
        ..Default::default()
    });

    assert!(!monitor.should_report(&update, now));
    assert!(!monitor.should_report(
        &update,
        now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW - Duration::from_millis(1)
    ));
    assert!(monitor.should_report(&update, now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW));
    assert!(!monitor.should_report(
        &update,
        now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW + Duration::from_secs(1)
    ));
    assert!(monitor.should_report(
        &update,
        now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW + TORRENT_LOW_THROUGHPUT_REPORT_INTERVAL
    ));
}

#[test]
fn torrent_low_throughput_monitor_reports_zero_speed_with_few_churning_peers() {
    let now = Instant::now();
    let mut monitor = TorrentLowThroughputMonitor::default();
    let mut update = torrent_runtime_update(1024, 4096, 0);
    update.peers = Some(2);
    update.diagnostics = Some(crate::storage::TorrentRuntimeDiagnostics {
        live_peers: 2,
        seen_peers: 8,
        connecting_peers: 2,
        peer_connection_attempts: 6,
        listen_port: None,
        ..Default::default()
    });

    assert!(!monitor.should_report(&update, now));
    assert!(
        monitor.should_report(&update, now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW),
        "few peer torrents with churn and no progress should still be reported as stalled"
    );
}

#[test]
fn torrent_low_throughput_monitor_uses_fetched_progress_window_over_instant_kbps() {
    let now = Instant::now();
    let mut monitor = TorrentLowThroughputMonitor::default();
    let mut update = torrent_runtime_update(0, 0, 32 * 1024);
    update.diagnostics = Some(crate::storage::TorrentRuntimeDiagnostics {
        live_peers: TORRENT_LOW_THROUGHPUT_LIVE_PEER_THRESHOLD,
        seen_peers: 20,
        contributing_peers: 2,
        listen_port: Some(42000),
        ..Default::default()
    });

    assert!(!monitor.should_report(&update, now));
    update.fetched_bytes = TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND
        * TORRENT_LOW_THROUGHPUT_REPORT_WINDOW.as_secs();
    update.downloaded_bytes = update.fetched_bytes;
    assert!(
        !monitor.should_report(&update, now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW),
        "steady fetched-byte progress should prevent a low instant-speed stall report"
    );
    assert!(!monitor.should_report(
        &update,
        now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW + Duration::from_secs(1)
    ));
    assert!(
        monitor.should_report(
            &update,
            now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW
                + Duration::from_secs(1)
                + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW
        ),
        "the monitor should report if fetched-byte progress stops after the recovery window"
    );
}

#[test]
fn torrent_low_throughput_monitor_resets_when_speed_recovers() {
    let now = Instant::now();
    let mut monitor = TorrentLowThroughputMonitor::default();
    let mut update = torrent_runtime_update(1024, 4096, 32);
    update.download_speed = TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND - 1;
    update.diagnostics = Some(crate::storage::TorrentRuntimeDiagnostics {
        live_peers: TORRENT_LOW_THROUGHPUT_LIVE_PEER_THRESHOLD,
        ..Default::default()
    });

    assert!(!monitor.should_report(&update, now));
    update.download_speed = TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND;
    assert!(!monitor.should_report(&update, now + Duration::from_secs(10)));
    update.download_speed = TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND - 1;
    assert!(!monitor.should_report(&update, now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW));
}

#[test]
fn torrent_low_throughput_message_includes_peer_session_and_listener_context() {
    let mut update = torrent_runtime_update(1024, 4096, 32);
    update.download_speed = 64 * 1024;
    update.diagnostics = Some(crate::storage::TorrentRuntimeDiagnostics {
        live_peers: 12,
        seen_peers: 30,
        dead_peers: 4,
        not_needed_peers: 3,
        contributing_peers: 2,
        peer_errors: 1,
        peers_with_errors: 1,
        peer_connection_attempts: 7,
        session_download_speed: 64 * 1024,
        session_upload_speed: 8 * 1024,
        listen_port: Some(42000),
        listener_fallback: true,
        ..Default::default()
    });

    let message = torrent_low_throughput_message(&update);

    assert!(message.contains("12 live peers"));
    assert!(message.contains("30 seen"));
    assert!(message.contains("2 contributing"));
    assert!(message.contains("1 peer error events across 1 peers"));
    assert!(message.contains("7 connection attempts"));
    assert!(message.contains("session down 65536 B/s"));
    assert!(message.contains("listen port 42000"));
    assert!(message.contains("listener fallback active"));
}

#[test]
fn torrent_progress_persists_first_seed_stop_and_interval_ticks() {
    let now = Instant::now();

    assert!(torrent_progress_should_persist(
        true, false, false, now, now,
    ));
    assert!(torrent_progress_should_persist(
        false,
        true,
        false,
        now,
        now + Duration::from_secs(1),
    ));
    assert!(torrent_progress_should_persist(
        false,
        false,
        true,
        now,
        now + Duration::from_millis(250),
    ));
    assert!(!torrent_progress_should_persist(
        false,
        false,
        false,
        now,
        now + Duration::from_secs(4),
    ));
    assert!(torrent_progress_should_persist(
        false,
        false,
        false,
        now,
        now + PROGRESS_PERSIST_INTERVAL,
    ));
}

#[test]
fn torrent_seed_elapsed_prefers_persisted_start_time() {
    assert_eq!(
        torrent_seed_elapsed_seconds(Some(1_000), 91_000, Duration::from_secs(5)),
        90
    );
    assert_eq!(
        torrent_seed_elapsed_seconds(None, 91_000, Duration::from_secs(5)),
        5
    );
}

#[test]
fn torrent_seed_policy_prefers_cumulative_ratio_from_state() {
    let torrent = TorrentInfo {
        uploaded_bytes: 2048,
        ratio: 2.0,
        ..TorrentInfo::default()
    };

    assert_eq!(
        torrent_seed_ratio_for_policy(Some(&torrent), 1024, 128),
        2.0
    );
}

#[test]
fn transfer_dispatch_accepts_http_jobs() {
    assert_eq!(
        transfer_dispatch_for_kind(TransferKind::Http),
        Some(TransferDispatch::Http)
    );
}

#[test]
fn transfer_dispatch_accepts_torrent_jobs() {
    assert_eq!(
        transfer_dispatch_for_kind(TransferKind::Torrent),
        Some(TransferDispatch::Torrent)
    );
}

#[tokio::test]
async fn range_probe_metadata_uses_partial_content_total_and_identity_header() {
    let response = concat!(
        "HTTP/1.1 206 Partial Content\r\n",
        "Content-Range: bytes 0-0/33554432\r\n",
        "Content-Length: 1\r\n",
        "Content-Disposition: attachment; filename=\"probe.bin\"\r\n",
        "\r\n",
        "x"
    );
    let (url, request_handle) = spawn_one_response_server(response).await;
    let client = download_client().unwrap();

    let RangeProbeOutcome::PartialContent(metadata, transport) =
        probe_range_metadata_response(&client, &url, None).await
    else {
        panic!("range probe should derive metadata from partial content");
    };
    let request = request_handle.await.unwrap();
    let request_lower = request.to_ascii_lowercase();

    assert!(request_lower.contains("range: bytes=0-0"));
    assert!(request_lower.contains("accept-encoding: identity"));
    assert_eq!(metadata.total_bytes, Some(33_554_432));
    assert_eq!(metadata.resume_support, ResumeSupport::Supported);
    assert_eq!(metadata.filename.as_deref(), Some("probe.bin"));
    assert_eq!(transport, "http/1.1");
}

#[tokio::test]
async fn range_probe_reuses_full_response_when_server_ignores_range() {
    let response = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Length: 4\r\n",
        "Accept-Ranges: bytes\r\n",
        "\r\n",
        "rust"
    );
    let (url, request_handle) = spawn_one_response_server(response).await;
    let client = download_client().unwrap();

    let RangeProbeOutcome::FullResponse(response) =
        probe_range_metadata_response(&client, &url, None).await
    else {
        panic!("ignored range response should be reusable as the single stream");
    };
    let request = request_handle.await.unwrap();
    let request_lower = request.to_ascii_lowercase();
    let body = response.bytes().await.unwrap();

    assert!(request_lower.contains("range: bytes=0-0"));
    assert_eq!(body.as_ref(), b"rust");
}

#[tokio::test]
async fn direct_segment_writer_writes_into_partial_file_without_segment_artifacts() {
    let root = test_download_runtime_dir("direct-segment-writer");
    let temp_path = root.join("download.bin.part");

    prepare_direct_segment_file(&temp_path, 12).await.unwrap();
    let mut file = open_direct_segment_file(&temp_path).await.unwrap();
    write_segment_chunk_to(&mut file, 4, b"rust").await.unwrap();

    let bytes = tokio::fs::read(&temp_path).await.unwrap();
    assert_eq!(&bytes[4..8], b"rust");
    assert!(!segment_path(&temp_path, 0).exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn direct_segment_file_preparation_preserves_existing_resume_bytes() {
    let root = test_download_runtime_dir("direct-segment-preserve");
    let temp_path = root.join("download.bin.part");
    tokio::fs::write(&temp_path, b"abcdefghijkl").await.unwrap();

    prepare_direct_segment_file(&temp_path, 12).await.unwrap();

    let bytes = tokio::fs::read(&temp_path).await.unwrap();
    assert_eq!(bytes, b"abcdefghijkl");

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn direct_segment_sidecar_tracks_progress_and_cleans_legacy_segments() {
    let root = test_download_runtime_dir("direct-segment-sidecar");
    let temp_path = root.join("download.bin.part");
    let plan = RangePlan {
        total_bytes: 12,
        segments: vec![
            ByteRange { start: 0, end: 3 },
            ByteRange { start: 4, end: 7 },
            ByteRange { start: 8, end: 11 },
        ],
    };

    let validators = EntityValidators::default();
    let mut state = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .unwrap();
    prepare_direct_segment_file(&temp_path, plan.total_bytes)
        .await
        .unwrap();
    state.segments[0].downloaded_bytes = 4;
    state.segments[0].completed = true;
    state.segments[1].downloaded_bytes = 2;
    state.segments[2].downloaded_bytes = 5;
    persist_segment_state(&temp_path, &state).await.unwrap();
    assert!(
        !segment_meta_temp_path(&temp_path).exists(),
        "segment metadata should be finalized with a rename and no stale temp sidecar"
    );
    tokio::fs::write(segment_path(&temp_path, 0), vec![1_u8; 4])
        .await
        .unwrap();

    let mut reloaded = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .unwrap();
    refresh_segment_completion_from_disk(&temp_path, &mut reloaded).await;

    assert_eq!(reloaded.segments[0].downloaded_bytes, 4);
    assert!(reloaded.segments[0].completed);
    assert_eq!(segment_existing_len(&temp_path, &reloaded.segments[1]), 2);
    assert!(!reloaded.segments[1].completed);
    assert_eq!(reloaded.segments[2].downloaded_bytes, 0);
    assert!(!reloaded.segments[2].completed);
    assert!(!segment_path(&temp_path, 0).exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn completed_segmented_download_records_repeated_decode_body_reconnect_summary() {
    let state = SharedState::for_tests(
        test_storage_path("segment-decode-reconnect-diagnostic"),
        vec![torrent_job("job_decode_reconnect", JobState::Downloading)],
    );
    let reconnects = SegmentReconnectTracker::default();

    reconnects.record_decode_body_reconnect(1);
    reconnects.record_decode_body_reconnect(2);
    record_decode_body_reconnect_completion_diagnostic(&state, "job_decode_reconnect", &reconnects)
        .await;

    let snapshot = state
        .diagnostics_snapshot(crate::storage::HostRegistrationDiagnostics {
            status: crate::storage::HostRegistrationStatus::Missing,
            entries: Vec::new(),
        })
        .await;
    let event = snapshot
        .recent_events
        .last()
        .expect("decode-body reconnect completion diagnostic");

    assert_eq!(event.level, DiagnosticLevel::Info);
    assert_eq!(event.category, "download");
    assert_eq!(event.job_id.as_deref(), Some("job_decode_reconnect"));
    assert_eq!(
        event.message,
        "Segmented download completed after 2 retryable decode-body reconnects (max segment attempt 2)."
    );
}

#[tokio::test]
async fn cleanup_segment_artifacts_removes_scanned_legacy_segment_files_only() {
    let root = test_download_runtime_dir("segment-cleanup-scan");
    let temp_path = root.join("download.bin.part");
    let stale_unplanned_segment = segment_path(&temp_path, 5);
    let decoy_segment_prefix = root.join("download.bin.part.segment-note");
    let decoy_non_numeric = root.join("download.bin.part.seg.tmp");

    tokio::fs::write(segment_path(&temp_path, 0), b"old")
        .await
        .unwrap();
    tokio::fs::write(&stale_unplanned_segment, b"stale")
        .await
        .unwrap();
    tokio::fs::write(&decoy_segment_prefix, b"keep")
        .await
        .unwrap();
    tokio::fs::write(&decoy_non_numeric, b"keep").await.unwrap();

    cleanup_segment_artifacts(&temp_path, 1).await;

    assert!(!segment_path(&temp_path, 0).exists());
    assert!(!stale_unplanned_segment.exists());
    assert!(decoy_segment_prefix.exists());
    assert!(decoy_non_numeric.exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn record_segment_progress_releases_metadata_lock_before_persisting() {
    let root = test_download_runtime_dir("segment-progress-lock-release");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let metadata = Arc::new(Mutex::new(new_segment_state_for_test(
        &plan,
        EntityValidators::default(),
    )));
    let metadata_lock = segment_metadata_lock(&temp_path);
    let metadata_guard = metadata_lock.lock().await;

    let record_task = {
        let metadata = Arc::clone(&metadata);
        let temp_path = temp_path.clone();
        tokio::spawn(async move {
            record_segment_progress(&temp_path, &metadata, 1, 2, false, true).await
        })
    };

    let progress_observed = tokio::time::timeout(Duration::from_millis(300), async {
        loop {
            if let Ok(state) = metadata.try_lock() {
                let segment = state
                    .segments
                    .iter()
                    .find(|segment| segment.index == 1)
                    .expect("segment should exist");
                if segment.downloaded_bytes == 2 && !segment.completed {
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .is_ok();

    assert!(
        progress_observed,
        "segment progress should release the metadata mutex before waiting on sidecar persistence"
    );

    drop(metadata_guard);
    record_task
        .await
        .expect("progress task should not panic")
        .expect("progress task should finish after sidecar lock is released");

    let reloaded = load_or_create_segment_state(&temp_path, &plan, &EntityValidators::default())
        .await
        .expect("forced progress persist should write readable metadata");
    assert_eq!(reloaded.segments[1].downloaded_bytes, 2);
    assert!(!reloaded.segments[1].completed);

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn blocked_segment_progress_persist_flushes_latest_metadata_snapshot() {
    let root = test_download_runtime_dir("segment-progress-latest-snapshot");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let metadata = Arc::new(Mutex::new(new_segment_state_for_test(
        &plan,
        EntityValidators::default(),
    )));
    let metadata_lock = segment_metadata_lock(&temp_path);
    let metadata_guard = metadata_lock.lock().await;

    let persist_task = {
        let metadata = Arc::clone(&metadata);
        let temp_path = temp_path.clone();
        tokio::spawn(async move {
            record_segment_progress(&temp_path, &metadata, 0, 4, true, true).await
        })
    };

    tokio::time::timeout(Duration::from_millis(300), async {
        loop {
            if let Ok(state) = metadata.try_lock() {
                if state.segments[0].completed {
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first progress update should not keep the metadata mutex while persistence waits");

    record_segment_progress(&temp_path, &metadata, 1, 3, false, false)
        .await
        .expect("coalesced progress update should succeed while persistence waits");

    drop(metadata_guard);
    persist_task
        .await
        .expect("persisting progress task should not panic")
        .expect("persisting progress task should finish after sidecar lock is released");

    let reloaded = load_or_create_segment_state(&temp_path, &plan, &EntityValidators::default())
        .await
        .expect("forced progress persist should write readable metadata");
    assert_eq!(reloaded.segments[0].downloaded_bytes, 4);
    assert!(reloaded.segments[0].completed);
    assert_eq!(reloaded.segments[1].downloaded_bytes, 3);
    assert!(!reloaded.segments[1].completed);

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn missing_segment_metadata_preserves_preallocated_partial_and_requires_restart() {
    let root = test_download_runtime_dir("segment-missing-metadata-preserve");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let validators = EntityValidators::default();
    tokio::fs::write(&temp_path, vec![0_u8; plan.total_bytes as usize])
        .await
        .unwrap();

    let error = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .expect_err("missing segment metadata with an existing partial should not reset progress");

    assert_eq!(error.category, FailureCategory::Resume);
    assert!(error.message.contains("Resume metadata is missing"));
    assert_eq!(
        error.resume_metadata_issue,
        Some(SegmentResumeMetadataIssue::Missing)
    );
    assert!(temp_path.exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn corrupt_segment_metadata_preserves_preallocated_partial_and_requires_restart() {
    let root = test_download_runtime_dir("segment-corrupt-metadata-preserve");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let validators = EntityValidators::default();
    tokio::fs::write(&temp_path, vec![0_u8; plan.total_bytes as usize])
        .await
        .unwrap();
    tokio::fs::write(segment_meta_path(&temp_path), b"not json")
        .await
        .unwrap();

    let error = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .expect_err("corrupt segment metadata with an existing partial should not reset progress");

    assert_eq!(error.category, FailureCategory::Resume);
    assert!(error.message.contains("Resume metadata is missing"));
    assert_eq!(
        error.resume_metadata_issue,
        Some(SegmentResumeMetadataIssue::Corrupt)
    );
    assert!(temp_path.exists());
    assert!(segment_meta_path(&temp_path).exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn direct_segment_buffered_writer_appends_chunks_after_initial_seek() {
    let root = test_download_runtime_dir("direct-segment-buffered-writer");
    let temp_path = root.join("download.bin.part");

    prepare_direct_segment_file(&temp_path, 12).await.unwrap();
    let mut writer = open_direct_segment_writer_at(&temp_path, 4).await.unwrap();
    write_segment_chunk(&mut writer, b"ru").await.unwrap();
    write_segment_chunk(&mut writer, b"st").await.unwrap();
    flush_segment_writer(&mut writer).await.unwrap();

    let bytes = tokio::fs::read(&temp_path).await.unwrap();
    assert_eq!(&bytes[4..8], b"rust");
    assert!(!segment_path(&temp_path, 0).exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[test]
fn non_rate_limit_segment_errors_do_not_reduce_future_segment_caps() {
    let _guard = segment_host_score_test_guard();
    reset_segment_host_scores_for_tests();
    let now = Instant::now();
    let key = "hoster:network-noise";
    let error = download_error(
        FailureCategory::Network,
        "Download failed: error decoding response body".into(),
        true,
    );

    for offset in 0..4 {
        let decision = record_segment_reconnect_pressure_for_error(
            key,
            16,
            &error,
            now + Duration::from_secs(offset),
        );
        assert_eq!(decision.reduced_target, None);
    }

    let profile = profile_for_effective_http_url_with_pressure_key_at(
        "https://cdn.example.com/downloads/game.rar",
        Some(key),
        now + Duration::from_secs(5),
    );
    assert_eq!(profile.initial_segments, 4);
    assert_eq!(profile.soft_max_segments, 4);
    assert_eq!(profile.max_segments, 8);
}

async fn spawn_one_response_server(
    response: &'static str,
) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = vec![0_u8; 4096];
        let read = socket.read(&mut buffer).await.unwrap();
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        socket.write_all(response.as_bytes()).await.unwrap();
        request
    });

    (format!("http://{address}/download.bin"), handle)
}

fn three_segment_test_plan() -> RangePlan {
    RangePlan {
        total_bytes: 12,
        segments: vec![
            ByteRange { start: 0, end: 3 },
            ByteRange { start: 4, end: 7 },
            ByteRange { start: 8, end: 11 },
        ],
    }
}

fn new_segment_state_for_test(
    plan: &RangePlan,
    validators: EntityValidators,
) -> SegmentedDownloadState {
    SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: plan.total_bytes,
        validators,
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: plan
            .segments
            .iter()
            .copied()
            .enumerate()
            .map(|(index, range)| SegmentProgress {
                index,
                range,
                downloaded_bytes: 0,
                completed: false,
            })
            .collect(),
    }
}

fn segmented_state_for_test(
    total_bytes: u64,
    ranges: Vec<(u64, u64, u64, bool)>,
) -> SegmentedDownloadState {
    SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: ranges
            .into_iter()
            .enumerate()
            .map(
                |(index, (start, end, downloaded_bytes, completed))| SegmentProgress {
                    index,
                    range: ByteRange { start, end },
                    downloaded_bytes,
                    completed,
                },
            )
            .collect(),
    }
}

fn test_download_runtime_dir(name: &str) -> PathBuf {
    let root = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn archive_test_entry(root: &Path, name: &str, contents: &[u8]) -> crate::state::BulkArchiveEntry {
    let source_path = root.join(name);
    std::fs::write(&source_path, contents).unwrap();
    crate::state::BulkArchiveEntry {
        source_path,
        archive_name: name.into(),
    }
}

#[derive(Default)]
struct RecordingArchiveExtractor {
    calls: TestMutex<Vec<PathBuf>>,
    output_dirs: TestMutex<Vec<PathBuf>>,
}

impl ArchiveExtractor for RecordingArchiveExtractor {
    fn extract(&self, first_part: &Path, output_dir: &Path) -> Result<(), String> {
        self.calls.lock().unwrap().push(first_part.to_path_buf());
        self.output_dirs
            .lock()
            .unwrap()
            .push(output_dir.to_path_buf());
        let stem = first_part
            .file_name()
            .and_then(|value| value.to_str())
            .and_then(|name| name.split('.').next())
            .unwrap_or("Archive");
        let output_path = output_dir.join(stem).join("content.bin");
        std::fs::create_dir_all(output_path.parent().unwrap()).unwrap();
        std::fs::write(output_path, stem.as_bytes()).unwrap();
        Ok(())
    }
}

struct FlatContentArchiveExtractor;

impl ArchiveExtractor for FlatContentArchiveExtractor {
    fn extract(&self, _first_part: &Path, output_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(output_dir).unwrap();
        std::fs::write(output_dir.join("content.bin"), b"duplicate").unwrap();
        Ok(())
    }
}

struct SymlinkArchiveExtractor;

impl ArchiveExtractor for SymlinkArchiveExtractor {
    fn extract(&self, _first_part: &Path, output_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(output_dir).unwrap();
        let target = output_dir.join("target.bin");
        let link = output_dir.join("linked.bin");
        std::fs::write(&target, b"target").unwrap();
        create_file_symlink_for_test(&target, &link)
    }
}

#[cfg(unix)]
fn create_file_symlink_for_test(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn create_file_symlink_for_test(target: &Path, link: &Path) -> Result<(), String> {
    std::os::windows::fs::symlink_file(target, link)
        .map_err(|_| "symlink creation is not available in this test environment".to_string())
}

#[derive(Default)]
struct LockOnceArchiveExtractor {
    calls: TestMutex<usize>,
}

impl ArchiveExtractor for LockOnceArchiveExtractor {
    fn extract(&self, first_part: &Path, output_dir: &Path) -> Result<(), String> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        if *calls == 1 {
            return Err(seven_zip_failure_message(
                first_part,
                Some(2),
                "ERROR: The process cannot access the file because it is being used by another process.",
            ));
        }

        let output_path = output_dir.join("Game").join("content.bin");
        std::fs::create_dir_all(output_path.parent().unwrap()).unwrap();
        std::fs::write(output_path, b"Game").unwrap();
        Ok(())
    }
}

fn extracting_staging_dirs(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.contains(".extracting-"))
        })
        .collect()
}

fn http_segment_policy_task(
    is_bulk_member: bool,
    resolved_from_url: Option<&str>,
) -> crate::state::DownloadTask {
    crate::state::DownloadTask {
        id: "job_policy".into(),
        url: "https://cdn.example.com/file.bin".into(),
        filename: "file.bin".into(),
        transfer_kind: TransferKind::Http,
        torrent: None,
        handoff_auth: None,
        resolved_from_url: resolved_from_url.map(str::to_string),
        source: None,
        is_bulk_member,
        bulk_archive_id: is_bulk_member.then_some("bulk_policy".into()),
        retry_attempts: 0,
        target_path: PathBuf::from("C:/Downloads/file.bin"),
        temp_path: PathBuf::from("C:/Downloads/file.bin.part"),
    }
}

async fn spawn_cookie_required_server() -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = vec![0_u8; 4096];
        let read = socket.read(&mut buffer).await.unwrap();
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        let response = if request.to_ascii_lowercase().contains("cookie: session=abc") {
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
        } else {
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n"
        };
        socket.write_all(response.as_bytes()).await.unwrap();
        request
    });

    (format!("http://{address}/download.bin"), handle)
}

#[tokio::test]
async fn sha256_digest_reads_file_contents() {
    let root = test_download_runtime_dir("sha256-digest");
    let path = root.join("hello.txt");
    tokio::fs::write(&path, b"hello").await.unwrap();

    let digest = compute_sha256(&path).await.unwrap();

    assert_eq!(
        digest,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );

    let _ = tokio::fs::remove_dir_all(root).await;
}
