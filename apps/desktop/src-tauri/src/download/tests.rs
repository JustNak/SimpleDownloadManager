use super::*;
use crate::storage::{DownloadJob, HandoffAuth, HandoffAuthHeader, JobState, TorrentInfo};
use std::future::pending;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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

#[test]
fn http_status_errors_are_classified_by_recoverability() {
    let unavailable = error_for_http_status(StatusCode::SERVICE_UNAVAILABLE, false);
    assert_eq!(unavailable.category, FailureCategory::Server);
    assert!(unavailable.retryable);

    let not_found = error_for_http_status(StatusCode::NOT_FOUND, false);
    assert_eq!(not_found.category, FailureCategory::Http);
    assert!(!not_found.retryable);
}

#[test]
fn retry_delay_caps_at_last_configured_delay() {
    assert_eq!(retry_delay_for_attempt(0), REQUEST_RETRY_DELAYS[0]);
    assert_eq!(
        retry_delay_for_attempt(99),
        *REQUEST_RETRY_DELAYS.last().unwrap()
    );
}

#[test]
fn create_bulk_archive_sync_writes_zip_with_distinct_entry_names() {
    let root = test_download_runtime_dir("bulk-archive-native-zip");
    let source_a = root.join("source-a.txt");
    let source_b = root.join("source-b.txt");
    std::fs::write(&source_a, b"alpha").unwrap();
    std::fs::write(&source_b, b"bravo").unwrap();
    let output_path = root.join("downloads.zip");

    let archive = BulkArchiveReady {
        archive_id: "bulk_1".into(),
        output_path: output_path.clone(),
        entries: vec![
            crate::state::BulkArchiveEntry {
                source_path: source_a,
                archive_name: "file.txt".into(),
            },
            crate::state::BulkArchiveEntry {
                source_path: source_b,
                archive_name: "file (1).txt".into(),
            },
        ],
    };

    let result = create_bulk_archive_sync(archive).expect("archive should be created");

    assert_eq!(result, output_path);
    assert_eq!(
        zip_central_directory_names(&result),
        vec!["file.txt".to_string(), "file (1).txt".to_string()]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_bulk_archive_sync_rejects_missing_source() {
    let root = test_download_runtime_dir("bulk-archive-missing-source");
    let output_path = root.join("downloads.zip");
    let archive = BulkArchiveReady {
        archive_id: "bulk_2".into(),
        output_path: output_path.clone(),
        entries: vec![crate::state::BulkArchiveEntry {
            source_path: root.join("missing.txt"),
            archive_name: "missing.txt".into(),
        }],
    };

    let error = create_bulk_archive_sync(archive).expect_err("missing source should fail");

    assert!(error.contains("missing.txt"));
    assert!(!output_path.exists());

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn torrent_metadata_add_returns_canceled_when_job_is_canceled() {
    let state = SharedState::for_tests(
        test_storage_path("torrent-metadata-canceled"),
        vec![torrent_job("job_1", JobState::Canceled)],
    );

    let outcome = tokio::time::timeout(
        Duration::from_secs(1),
        add_torrent_with_controls(
            &state,
            "job_1",
            pending::<Result<TorrentAddSessionOutcome, String>>(),
            Duration::from_secs(60),
            Duration::from_millis(1),
        ),
    )
    .await
    .expect("metadata helper should observe canceled job")
    .expect("canceled job should not fail");

    assert!(matches!(
        outcome,
        TorrentAddOutcome::Interrupted(DownloadOutcome::Canceled)
    ));
}

#[tokio::test]
async fn torrent_metadata_timeout_is_retryable_torrent_error() {
    let state = SharedState::for_tests(
        test_storage_path("torrent-metadata-timeout"),
        vec![torrent_job("job_1", JobState::Starting)],
    );

    let error = add_torrent_with_controls(
        &state,
        "job_1",
        pending::<Result<TorrentAddSessionOutcome, String>>(),
        Duration::from_millis(1),
        Duration::from_secs(60),
    )
    .await
    .expect_err("metadata timeout should fail");

    assert_eq!(error.category, FailureCategory::Torrent);
    assert!(error.retryable);
    assert_eq!(
        error.message,
        "Torrent metadata lookup timed out after 60 seconds. Add trackers or retry later."
    );
}

#[test]
fn torrent_metadata_timeout_is_sixty_seconds() {
    assert_eq!(TORRENT_METADATA_TIMEOUT, Duration::from_secs(60));
}

#[test]
fn seeding_transition_releases_download_scheduler_slot_once() {
    assert!(seeding_transition_releases_download_slot(
        JobState::Queued,
        JobState::Seeding,
    ));
    assert!(seeding_transition_releases_download_slot(
        JobState::Starting,
        JobState::Seeding,
    ));
    assert!(seeding_transition_releases_download_slot(
        JobState::Downloading,
        JobState::Seeding,
    ));
    assert!(!seeding_transition_releases_download_slot(
        JobState::Seeding,
        JobState::Seeding,
    ));
    assert!(!seeding_transition_releases_download_slot(
        JobState::Starting,
        JobState::Downloading,
    ));
    assert!(!seeding_transition_releases_download_slot(
        JobState::Downloading,
        JobState::Paused,
    ));
    assert!(!seeding_transition_releases_download_slot(
        JobState::Downloading,
        JobState::Completed,
    ));
}

#[test]
fn torrent_metadata_timeout_cleanup_runs_before_retryable_error_returns() {
    let source = include_str!("torrent.rs");
    let timeout_branch = source
        .find("if is_torrent_metadata_timeout_error(&error)")
        .expect("torrent metadata timeout branch should exist");
    let cleanup_call = source[timeout_branch..]
        .find("cleanup_pending_torrent_metadata(")
        .expect("timeout branch should clean up pending metadata")
        + timeout_branch;
    let retryable_return = source[cleanup_call..]
        .find("return Err(error);")
        .expect("timeout branch should return the retryable error after cleanup")
        + cleanup_call;

    assert!(
        cleanup_call < retryable_return,
        "pending torrent metadata cleanup must run before the retryable timeout error is returned"
    );
}

#[test]
fn tracker_first_metadata_outcomes_have_user_visible_diagnostics() {
    assert_eq!(
        tracker_first_metadata_diagnostic_message(&TrackerFirstMetadataOutcome::Resolved),
        "Tracker-first torrent metadata resolved"
    );
    assert_eq!(
            tracker_first_metadata_diagnostic_message(&TrackerFirstMetadataOutcome::TimedOut),
            "Tracker-first torrent metadata timed out after 15 seconds; falling back to the main DHT session"
        );
    assert_eq!(
            tracker_first_metadata_diagnostic_message(&TrackerFirstMetadataOutcome::Failed(
                "tracker unavailable".into()
            )),
            "Tracker-first torrent metadata failed; falling back to the main DHT session: tracker unavailable"
        );
}

#[test]
fn torrent_resume_path_diagnostics_distinguish_resume_and_readd() {
    assert_eq!(
        torrent_resume_existing_session_message(),
        "Resumed torrent from saved session"
    );
    assert_eq!(
        torrent_restore_existing_seeding_session_message(),
        "Restored torrent seeding from saved session"
    );
    assert_eq!(
        torrent_readd_for_verification_message(),
        "No saved torrent session found; re-adding torrent for piece verification"
    );
    assert_eq!(
        torrent_restore_recheck_existing_files_message(),
        "No saved seeding session found; rechecking existing files before seeding"
    );
    assert!(!torrent_has_resume_identity(None));
    assert!(torrent_has_resume_identity(Some(&TorrentInfo {
        engine_id: Some(7),
        ..TorrentInfo::default()
    })));
    assert!(torrent_has_resume_identity(Some(&TorrentInfo {
        info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        ..TorrentInfo::default()
    })));
    assert!(!is_torrent_seeding_restore(None));
    assert!(is_torrent_seeding_restore(Some(&TorrentInfo {
        seeding_started_at: Some(123_456),
        ..TorrentInfo::default()
    })));
}

#[test]
fn stale_torrent_completion_detects_empty_magnet_target() {
    let target_dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("stale-torrent-empty-target-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&target_dir);
    std::fs::create_dir_all(&target_dir).unwrap();

    assert!(target_payload_appears_empty(&target_dir.join("missing")));

    let update = crate::state::TorrentRuntimeSnapshot {
        engine_id: 42,
        info_hash: "420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        name: Some("Stale Torrent".into()),
        total_files: Some(1),
        peers: Some(0),
        seeds: None,
        downloaded_bytes: 8 * 1024,
        total_bytes: 8 * 1024,
        uploaded_bytes: 0,
        fetched_bytes: 0,
        download_speed: 0,
        upload_speed: 0,
        eta: None,
        phase: crate::state::TorrentRuntimePhase::Live,
        finished: true,
        error: None,
        diagnostics: None,
    };

    assert!(target_payload_appears_empty(&target_dir));
    assert!(is_stale_torrent_completion(
        crate::torrent::TorrentSourceKind::Magnet,
        true,
        &update,
        &target_dir,
    ));

    let mut fetched_update = update.clone();
    fetched_update.fetched_bytes = 512;
    assert!(!is_stale_torrent_completion(
        crate::torrent::TorrentSourceKind::Magnet,
        true,
        &fetched_update,
        &target_dir,
    ));

    std::fs::write(target_dir.join("payload.bin"), [1_u8]).unwrap();
    assert!(!target_payload_appears_empty(&target_dir));
    assert!(!is_stale_torrent_completion(
        crate::torrent::TorrentSourceKind::Magnet,
        true,
        &update,
        &target_dir,
    ));

    let _ = std::fs::remove_dir_all(target_dir);
}

#[test]
fn stale_torrent_completion_ignores_non_initial_or_file_torrent_snapshots() {
    let target_dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("stale-torrent-guards-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&target_dir);
    std::fs::create_dir_all(&target_dir).unwrap();

    let update = crate::state::TorrentRuntimeSnapshot {
        engine_id: 42,
        info_hash: "420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        name: Some("Stale Torrent".into()),
        total_files: Some(1),
        peers: Some(0),
        seeds: None,
        downloaded_bytes: 8 * 1024,
        total_bytes: 8 * 1024,
        uploaded_bytes: 0,
        fetched_bytes: 0,
        download_speed: 0,
        upload_speed: 0,
        eta: None,
        phase: crate::state::TorrentRuntimePhase::Live,
        finished: true,
        error: None,
        diagnostics: None,
    };

    assert!(!is_stale_torrent_completion(
        crate::torrent::TorrentSourceKind::TorrentFile,
        true,
        &update,
        &target_dir,
    ));
    assert!(!is_stale_torrent_completion(
        crate::torrent::TorrentSourceKind::Magnet,
        false,
        &update,
        &target_dir,
    ));

    let _ = std::fs::remove_dir_all(target_dir);
}

#[test]
fn fresh_magnet_reused_session_forces_readd_but_restore_does_not() {
    let prepared_source = PreparedTorrentSource {
        source: "magnet:?xt=urn:btih:420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        source_kind: TorrentSourceKind::Magnet,
        fallback_trackers_added: 0,
        fallback_trackers_for_options: Vec::new(),
        tracker_first_metadata: true,
    };
    let reused = TorrentAddSessionOutcome {
        engine_id: 42,
        reused_existing_session: true,
    };

    assert!(should_readd_fresh_reused_session(
        Some(&TorrentInfo::default()),
        &prepared_source,
        reused,
    ));
    assert!(!should_readd_fresh_reused_session(
        Some(&TorrentInfo {
            seeding_started_at: Some(123_456),
            ..TorrentInfo::default()
        }),
        &prepared_source,
        reused,
    ));
    assert!(!should_readd_fresh_reused_session(
        Some(&TorrentInfo::default()),
        &prepared_source,
        TorrentAddSessionOutcome {
            engine_id: 42,
            reused_existing_session: false,
        },
    ));
}

#[test]
fn protected_restore_rejects_live_peer_fetch_before_completion() {
    let update = crate::state::TorrentRuntimeSnapshot {
        engine_id: 42,
        info_hash: "420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        name: Some("Need for Speed - Most Wanted".into()),
        total_files: Some(2),
        peers: Some(1),
        seeds: None,
        downloaded_bytes: 1024 * 1024,
        total_bytes: 3 * 1024 * 1024,
        uploaded_bytes: 0,
        fetched_bytes: 512 * 1024,
        download_speed: 128 * 1024,
        upload_speed: 0,
        eta: Some(15),
        phase: crate::state::TorrentRuntimePhase::Live,
        finished: false,
        error: None,
        diagnostics: None,
    };

    assert_eq!(
        torrent_restore_validation_failure(&update),
        Some(torrent_restore_peer_download_blocked_message()),
        "prior seeding restore must not keep downloading from peers under a restore label"
    );
}

#[test]
fn torrent_protected_restore_allows_idle_live_state_for_watchdog_recovery() {
    let update = crate::state::TorrentRuntimeSnapshot {
        engine_id: 42,
        info_hash: "420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        name: Some("Ubuntu".into()),
        total_files: None,
        peers: Some(12),
        seeds: None,
        downloaded_bytes: 0,
        total_bytes: 0,
        uploaded_bytes: 0,
        fetched_bytes: 0,
        download_speed: 0,
        upload_speed: 0,
        eta: None,
        phase: crate::state::TorrentRuntimePhase::Live,
        finished: false,
        error: None,
        diagnostics: None,
    };

    assert_eq!(
            torrent_restore_validation_failure(&update),
            None,
            "idle live restore sessions should be handled by the restore watchdog instead of immediate peer-download failure"
        );
}

#[test]
fn torrent_restore_watchdog_readds_once_then_stalls_after_second_idle_window() {
    let started_at = Instant::now();
    let idle_update = crate::state::TorrentRuntimeSnapshot {
        engine_id: 42,
        info_hash: "420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        name: None,
        total_files: None,
        peers: None,
        seeds: None,
        downloaded_bytes: 0,
        total_bytes: 0,
        uploaded_bytes: 0,
        fetched_bytes: 0,
        download_speed: 0,
        upload_speed: 0,
        eta: None,
        phase: crate::state::TorrentRuntimePhase::Initializing,
        finished: false,
        error: None,
        diagnostics: None,
    };
    let mut watchdog = TorrentRestoreWatchdog::new(started_at);

    assert_eq!(
        watchdog.observe(&idle_update, started_at + Duration::from_secs(44)),
        TorrentRestoreWatchdogDecision::Continue
    );
    assert_eq!(
        watchdog.observe(&idle_update, started_at + Duration::from_secs(45)),
        TorrentRestoreWatchdogDecision::Recheck
    );
    assert_eq!(
        watchdog.observe(&idle_update, started_at + Duration::from_secs(134)),
        TorrentRestoreWatchdogDecision::Continue
    );
    assert_eq!(
        watchdog.observe(&idle_update, started_at + Duration::from_secs(135)),
        TorrentRestoreWatchdogDecision::Stalled
    );
}

#[test]
fn torrent_restore_watchdog_resets_when_validation_reports_local_progress() {
    let started_at = Instant::now();
    let mut watchdog = TorrentRestoreWatchdog::new(started_at);
    let progress_update = crate::state::TorrentRuntimeSnapshot {
        engine_id: 42,
        info_hash: "420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        name: None,
        total_files: None,
        peers: None,
        seeds: None,
        downloaded_bytes: 1024,
        total_bytes: 2048,
        uploaded_bytes: 0,
        fetched_bytes: 0,
        download_speed: 0,
        upload_speed: 0,
        eta: None,
        phase: crate::state::TorrentRuntimePhase::Paused,
        finished: false,
        error: None,
        diagnostics: None,
    };

    assert_eq!(
        watchdog.observe(&progress_update, started_at + Duration::from_secs(50)),
        TorrentRestoreWatchdogDecision::Continue,
        "local verification progress should reset the idle timer"
    );
}

#[test]
fn torrent_peer_watchdog_diagnose_mode_reports_without_actions() {
    let started_at = Instant::now();
    let update = low_throughput_update();
    let mut watchdog =
        TorrentPeerConnectionWatchdog::new(TorrentPeerConnectionWatchdogMode::Diagnose, started_at);

    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(60)),
        TorrentPeerConnectionWatchdogDecision::Report
    );
    assert_eq!(
            watchdog.observe(&update, started_at + Duration::from_secs(121)),
            TorrentPeerConnectionWatchdogDecision::Report,
            "diagnose mode should keep reporting sustained peer issues without mutating the torrent session"
        );
}

#[test]
fn torrent_peer_watchdog_experimental_mode_refreshes_then_readds_once() {
    let started_at = Instant::now();
    let update = low_throughput_update();
    let mut watchdog = TorrentPeerConnectionWatchdog::new(
        TorrentPeerConnectionWatchdogMode::Experimental,
        started_at,
    );

    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(59)),
        TorrentPeerConnectionWatchdogDecision::Continue
    );
    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(60)),
        TorrentPeerConnectionWatchdogDecision::RefreshPeers
    );
    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(119)),
        TorrentPeerConnectionWatchdogDecision::Continue
    );
    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(120)),
        TorrentPeerConnectionWatchdogDecision::ReaddTorrent
    );
    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(240)),
        TorrentPeerConnectionWatchdogDecision::Report,
        "experimental mode should not keep refreshing or re-adding the same job attempt"
    );
}

#[test]
fn protected_restore_resolves_sibling_payload_for_generated_placeholder_target() {
    let target_dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("restore-target-repair-{}", std::process::id()));
    let placeholder = target_dir.join("torrent-a634dc94");
    let payload = target_dir.join("Need for Speed - Most Wanted [FitGirl Repack]");
    let _ = std::fs::remove_dir_all(&target_dir);
    std::fs::create_dir_all(&placeholder).unwrap();
    std::fs::create_dir_all(&payload).unwrap();
    std::fs::write(payload.join("payload.bin"), [1_u8]).unwrap();

    let resolved = protected_restore_payload_target(
        &placeholder,
        Some(&TorrentInfo {
            name: Some("Need for Speed - Most Wanted [FitGirl Repack]".into()),
            seeding_started_at: Some(123_456),
            uploaded_bytes: 21 * 1024 * 1024,
            fetched_bytes: 4 * 1024 * 1024 * 1024,
            ..TorrentInfo::default()
        }),
        "Need for Speed - Most Wanted [FitGirl Repack]",
    );

    assert_eq!(
            resolved,
            TorrentRestoreTarget::Repaired(payload),
            "restore should use the existing payload folder instead of the empty generated magnet placeholder"
        );

    let _ = std::fs::remove_dir_all(target_dir);
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
            average_piece_download_millis: None,
            listen_port: Some(42000),
            listener_fallback: false,
            peer_samples: Vec::new(),
        }),
    }
}

#[test]
fn restore_target_repair_cleans_only_empty_generated_placeholder() {
    let target_dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!(
            "restore-placeholder-cleanup-{}",
            std::process::id()
        ));
    let empty_placeholder = target_dir.join("torrent-a634dc94");
    let nonempty_placeholder = target_dir.join("torrent-deadbeef");
    let payload = target_dir.join("Need for Speed - Most Wanted [FitGirl Repack]");
    let _ = std::fs::remove_dir_all(&target_dir);
    std::fs::create_dir_all(&empty_placeholder).unwrap();
    std::fs::create_dir_all(&nonempty_placeholder).unwrap();
    std::fs::write(nonempty_placeholder.join("keep.bin"), [1_u8]).unwrap();
    std::fs::create_dir_all(&payload).unwrap();
    std::fs::write(payload.join("payload.bin"), [1_u8]).unwrap();

    cleanup_empty_generated_torrent_placeholder(&empty_placeholder, &payload);
    cleanup_empty_generated_torrent_placeholder(&nonempty_placeholder, &payload);

    assert!(
        !empty_placeholder.exists(),
        "empty generated torrent-* placeholder should be removed after path repair"
    );
    assert!(
        nonempty_placeholder.exists(),
        "non-empty generated placeholder should not be removed by best-effort cleanup"
    );

    let _ = std::fs::remove_dir_all(target_dir);
}

#[test]
fn live_seeding_detects_missing_payload_before_recreating_folder() {
    let target_dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("seeding-missing-payload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&target_dir);
    let update = crate::state::TorrentRuntimeSnapshot {
        engine_id: 42,
        info_hash: "420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        name: Some("Need for Speed - Most Wanted".into()),
        total_files: Some(2),
        peers: Some(1),
        seeds: None,
        downloaded_bytes: 3 * 1024 * 1024,
        total_bytes: 3 * 1024 * 1024,
        uploaded_bytes: 1024,
        fetched_bytes: 3 * 1024 * 1024,
        download_speed: 0,
        upload_speed: 128,
        eta: None,
        phase: crate::state::TorrentRuntimePhase::Live,
        finished: true,
        error: None,
        diagnostics: None,
    };

    assert!(
        torrent_seeding_payload_disappeared(&update, &target_dir),
        "missing target payload while rqbit reports live seeding should stop the session"
    );

    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(target_dir.join("payload.bin"), [1_u8]).unwrap();
    assert!(
        !torrent_seeding_payload_disappeared(&update, &target_dir),
        "existing payload should keep normal seeding behavior"
    );

    let _ = std::fs::remove_dir_all(target_dir);
}

#[test]
fn torrent_add_flow_wires_tracker_first_diagnostics_channel() {
    let source = include_str!("torrent.rs");
    let production_source = source
        .split("#[cfg(test)]")
        .next()
        .expect("download source should contain production code");
    let channel = production_source
        .find("spawn_tracker_first_metadata_diagnostics(")
        .expect("torrent add flow should create a diagnostics channel");
    let add_source = production_source[channel..]
        .find("add_prepared_torrent_with_controls(")
        .expect("torrent add flow should pass diagnostics to the controlled add helper")
        + channel;
    let argument = production_source[add_source..]
        .find("Some(tracker_first_diagnostics)")
        .expect("tracker-first diagnostics sender should be passed into the add helper")
        + add_source;

    assert!(
        channel < add_source && add_source < argument,
        "tracker-first diagnostics should be wired before metadata resolution starts"
    );
}

#[tokio::test]
async fn fallback_tracker_usage_records_diagnostic_event() {
    let state = SharedState::for_tests(
        test_storage_path("torrent-fallback-trackers-diagnostic"),
        vec![torrent_job("job_1", JobState::Starting)],
    );

    record_fallback_tracker_usage(&state, "job_1", 8, "magnet").await;

    let snapshot = state
        .diagnostics_snapshot(crate::storage::HostRegistrationDiagnostics {
            status: crate::storage::HostRegistrationStatus::Missing,
            entries: Vec::new(),
        })
        .await;
    let event = snapshot
        .recent_events
        .last()
        .expect("fallback diagnostic event");
    assert_eq!(event.level, DiagnosticLevel::Info);
    assert_eq!(event.category, "torrent");
    assert_eq!(
        event.message,
        "Added 8 fallback trackers for magnet metadata lookup"
    );
    assert_eq!(event.job_id.as_deref(), Some("job_1"));
}

#[test]
fn resume_support_uses_partial_content_before_header_hints() {
    assert_eq!(
        derive_resume_support_from_parts(StatusCode::PARTIAL_CONTENT, 10, None),
        ResumeSupport::Supported
    );
    assert_eq!(
        derive_resume_support_from_parts(StatusCode::OK, 10, Some("bytes")),
        ResumeSupport::Unsupported
    );
    assert_eq!(
        derive_resume_support_from_parts(StatusCode::OK, 0, Some("bytes")),
        ResumeSupport::Supported
    );
    assert_eq!(
        derive_resume_support_from_parts(StatusCode::OK, 0, None),
        ResumeSupport::Unknown
    );
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
        created_at: 1,
        progress: 0.0,
        total_bytes: 0,
        downloaded_bytes: 0,
        speed: 0,
        eta: 0,
        error: None,
        failure_category: None,
        resume_support: ResumeSupport::Unknown,
        retry_attempts: 0,
        target_path: format!("C:/Downloads/torrent-{id}"),
        temp_path: format!("C:/Downloads/torrent-{id}.part"),
        artifact_exists: None,
        bulk_archive: None,
    }
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

#[test]
fn preflight_metadata_uses_head_headers() {
    let metadata = derive_preflight_metadata_from_parts(
        Some(4_096),
        Some("bytes"),
        Some("attachment; filename=\"server-report.pdf\""),
        "https://example.com/download",
    );

    assert_eq!(metadata.total_bytes, Some(4_096));
    assert_eq!(metadata.resume_support, ResumeSupport::Supported);
    assert_eq!(metadata.filename.as_deref(), Some("server-report.pdf"));
}

#[test]
fn content_disposition_filename_avoids_windows_reserved_device_names() {
    assert_eq!(
        parse_content_disposition_filename("attachment; filename=\"CON\"").as_deref(),
        Some("CON_")
    );
    assert_eq!(
        parse_content_disposition_filename("attachment; filename=\"con.txt\"").as_deref(),
        Some("con.txt_")
    );
}

#[test]
fn content_disposition_plain_filename_decodes_percent_encoded_name() {
    assert_eq!(
            parse_content_disposition_filename(
                "attachment; filename=\"%5BNanakoRaws%5D%20Tensei%20Shitara%20Slime%20S4%20-%2002.mkv\""
            )
            .as_deref(),
            Some("[NanakoRaws] Tensei Shitara Slime S4 - 02.mkv")
        );
}

#[test]
fn url_filename_decodes_percent_encoded_path_segment() {
    let filename = derive_filename_from_url(
            "https://example.com/%5BNanakoRaws%5D%20Tensei%20Shitara%20Slime%20Datta%20Ken%20S4%20-%2002%20%28AT-X%20TV%201080p%20HEVC%20AAC%29.mkv",
        );

    assert_eq!(
        filename.as_deref(),
        Some("[NanakoRaws] Tensei Shitara Slime Datta Ken S4 - 02 (AT-X TV 1080p HEVC AAC).mkv")
    );
}

#[test]
fn speed_limit_throttle_calculates_remaining_delay() {
    assert_eq!(
        throttle_delay_for_limit(1024, 4096, Duration::from_secs(2)),
        Some(Duration::from_secs(2))
    );
    assert_eq!(
        throttle_delay_for_limit(1024, 4096, Duration::from_secs(4)),
        None
    );
    assert_eq!(
        throttle_delay_for_limit(0, 4096, Duration::from_secs(0)),
        None
    );
}

#[test]
fn balanced_range_plan_uses_target_size_and_caps_at_six_segments() {
    let profile = performance_profile(DownloadPerformanceMode::Balanced);
    let minimum_plan =
        plan_segmented_ranges(32 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("balanced mode should segment range-capable files at 32 MiB");
    let capped_plan =
        plan_segmented_ranges(512 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("large range-capable files should use segmented downloading");
    let plan = plan_segmented_ranges(256 * 1024 * 1024, ResumeSupport::Supported, None, profile)
        .expect("large range-capable files should use segmented downloading");

    assert_eq!(minimum_plan.segments.len(), 2);
    assert_eq!(plan.segments.len(), 4);
    assert_eq!(capped_plan.segments.len(), 6);
    assert_eq!(
        plan.segments[0],
        ByteRange {
            start: 0,
            end: 67_108_863
        }
    );
    assert_eq!(
        plan.segments[3],
        ByteRange {
            start: 201_326_592,
            end: 268_435_455,
        }
    );
}

#[test]
fn fast_range_plan_uses_target_size_and_caps_at_twelve_segments() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let minimum_plan =
        plan_segmented_ranges(16 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("fast mode should segment range-capable files at 16 MiB");
    let capped_plan =
        plan_segmented_ranges(1024 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("large fast downloads should use capped segmented downloading");

    assert_eq!(minimum_plan.segments.len(), 2);
    assert_eq!(capped_plan.segments.len(), 12);
}

#[test]
fn range_plan_falls_back_for_stable_small_unknown_or_limited_downloads() {
    assert!(plan_segmented_ranges(
        256 * 1024 * 1024,
        ResumeSupport::Supported,
        None,
        performance_profile(DownloadPerformanceMode::Stable),
    )
    .is_none());
    assert!(plan_segmented_ranges(
        16 * 1024 * 1024,
        ResumeSupport::Supported,
        None,
        performance_profile(DownloadPerformanceMode::Balanced),
    )
    .is_none());
    assert!(plan_segmented_ranges(
        256 * 1024 * 1024,
        ResumeSupport::Unknown,
        None,
        performance_profile(DownloadPerformanceMode::Balanced),
    )
    .is_none());
    assert!(plan_segmented_ranges(
        256 * 1024 * 1024,
        ResumeSupport::Supported,
        Some(1024),
        performance_profile(DownloadPerformanceMode::Balanced),
    )
    .is_none());
}

#[test]
fn content_range_validation_rejects_mismatched_segments() {
    assert!(content_range_matches(
        "bytes 1048576-2097151/4194304",
        ByteRange {
            start: 1_048_576,
            end: 2_097_151,
        },
        4_194_304,
    ));
    assert!(!content_range_matches(
        "bytes 0-2097151/4194304",
        ByteRange {
            start: 1_048_576,
            end: 2_097_151,
        },
        4_194_304,
    ));
    assert!(!content_range_matches(
        "bytes 1048576-2097151/9999999",
        ByteRange {
            start: 1_048_576,
            end: 2_097_151,
        },
        4_194_304,
    ));
}

#[test]
fn probed_range_metadata_wins_when_head_size_disagrees() {
    let merged = merge_preflight_metadata(
        Some(PreflightMetadata {
            total_bytes: Some(64),
            resume_support: ResumeSupport::Supported,
            filename: Some("head.bin".into()),
        }),
        PreflightMetadata {
            total_bytes: Some(128),
            resume_support: ResumeSupport::Supported,
            filename: Some("probe.bin".into()),
        },
    );

    assert_eq!(merged.total_bytes, Some(128));
    assert_eq!(merged.filename.as_deref(), Some("head.bin"));
}

#[test]
fn rolling_speed_smoothing_avoids_one_sample_collapse() {
    let mut speed = RollingSpeed::default();

    assert_eq!(
        speed.record_sample(8 * 1024 * 1024, Duration::from_secs(1)),
        8 * 1024 * 1024
    );
    let smoothed = speed.record_sample(512, Duration::from_secs(1));

    assert!(
        smoothed > 1024 * 1024,
        "one tiny sample should not collapse the displayed speed to near zero"
    );
}

#[test]
fn low_speed_recovery_retries_only_after_sustained_unlimited_slowdown() {
    let profile = performance_profile(DownloadPerformanceMode::Balanced);
    let mut monitor = LowSpeedMonitor::new(profile);

    assert_eq!(
        monitor.observe(4 * 1024, Duration::from_secs(10), false),
        LowSpeedDecision::Continue
    );
    assert_eq!(
        monitor.observe(4 * 1024, Duration::from_secs(21), false),
        LowSpeedDecision::Retry
    );
    assert_eq!(
        monitor.observe(0, Duration::from_secs(30), true),
        LowSpeedDecision::Continue
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

#[test]
fn host_range_backoff_expires_after_ten_minutes() {
    let backoff = RangeBackoffRegistry::default();
    let now = Instant::now();
    let url = "https://example.com/downloads/file.zip";

    assert!(!backoff.is_backed_off(url, now));
    backoff.record_rejection(url, now);

    assert!(backoff.is_backed_off(url, now + Duration::from_secs(599)));
    assert!(!backoff.is_backed_off(url, now + RANGE_BACKOFF_DURATION));
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

    let metadata = probe_range_metadata(&client, &url, None)
        .await
        .expect("range probe should derive metadata from partial content");
    let request = request_handle.await.unwrap();
    let request_lower = request.to_ascii_lowercase();

    assert!(request_lower.contains("range: bytes=0-0"));
    assert!(request_lower.contains("accept-encoding: identity"));
    assert_eq!(metadata.total_bytes, Some(33_554_432));
    assert_eq!(metadata.resume_support, ResumeSupport::Supported);
    assert_eq!(metadata.filename.as_deref(), Some("probe.bin"));
}

#[tokio::test]
async fn send_request_asks_for_identity_encoding() {
    let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
    let (url, request_handle) = spawn_one_response_server(response).await;
    let client = download_client().unwrap();

    let _response = send_request(&client, &url, 0, None).await.unwrap();
    let request = request_handle.await.unwrap();

    assert!(request
        .to_ascii_lowercase()
        .contains("accept-encoding: identity"));
}

#[tokio::test]
async fn send_request_applies_authenticated_handoff_headers() {
    let (url, request_handle) = spawn_cookie_required_server().await;
    let client = download_client().unwrap();
    let auth = HandoffAuth {
        headers: vec![HandoffAuthHeader {
            name: "Cookie".into(),
            value: "session=abc".into(),
        }],
    };

    let response = send_request(&client, &url, 0, Some(&auth)).await.unwrap();
    let request = request_handle.await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(request.to_ascii_lowercase().contains("cookie: session=abc"));
    assert!(request
        .to_ascii_lowercase()
        .contains("accept-encoding: identity"));
}

#[tokio::test]
async fn protected_handoff_access_probe_rejects_missing_browser_auth() {
    let (url, request_handle) = spawn_cookie_required_server().await;

    let error = probe_browser_handoff_access(&url, None)
        .await
        .expect_err("missing browser auth should reject protected downloads before queuing");
    let request = request_handle.await.unwrap();

    assert_eq!(error.code, "PROTECTED_DOWNLOAD_AUTH_REQUIRED");
    assert_eq!(error.status, Some(403));
    assert!(request.to_ascii_lowercase().contains("range: bytes=0-0"));
    assert!(request
        .to_ascii_lowercase()
        .contains("accept-encoding: identity"));
}

#[tokio::test]
async fn protected_handoff_access_probe_accepts_captured_browser_auth() {
    let (url, request_handle) = spawn_cookie_required_server().await;
    let auth = HandoffAuth {
        headers: vec![HandoffAuthHeader {
            name: "Cookie".into(),
            value: "session=abc".into(),
        }],
    };

    let result = probe_browser_handoff_access(&url, Some(&auth)).await;
    let request = request_handle.await.unwrap();

    assert!(result.is_ok());
    assert!(request.to_ascii_lowercase().contains("cookie: session=abc"));
    assert!(request.to_ascii_lowercase().contains("range: bytes=0-0"));
}

#[test]
fn authenticated_redirect_policy_rejects_cross_origin_redirects() {
    assert!(redirect_keeps_origin(
        "https://chatgpt.com/backend-api/estuary/content?id=file_123",
        "https://chatgpt.com/backend-api/estuary/content?id=file_456",
    ));
    assert!(!redirect_keeps_origin(
        "https://chatgpt.com/backend-api/estuary/content?id=file_123",
        "https://cdn.example.com/file.pdf",
    ));
}

#[test]
fn segmented_progress_counters_track_totals_without_shared_mutex() {
    let counters = SegmentedProgressCounters::new(vec![10, 20, 0]);

    assert_eq!(counters.total_downloaded(), 30);
    counters.store_segment_bytes(2, 5);
    counters.add_sample_bytes(7);

    assert_eq!(counters.total_downloaded(), 35);
    assert_eq!(counters.drain_sample_bytes(), 7);
    assert_eq!(counters.drain_sample_bytes(), 0);
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

    let mut state = load_or_create_segment_state(&temp_path, &plan)
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
    tokio::fs::write(segment_path(&temp_path, 0), vec![1_u8; 4])
        .await
        .unwrap();

    let mut reloaded = load_or_create_segment_state(&temp_path, &plan)
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

#[test]
fn range_rejection_after_probe_requests_single_stream_fallback() {
    let resume_error = download_error(
        FailureCategory::Resume,
        "The server did not honor a segmented range request.".into(),
        false,
    );
    let network_error =
        download_error(FailureCategory::Network, "The network failed.".into(), true);

    assert!(segmented_error_allows_single_stream_fallback(&resume_error));
    assert!(!segmented_error_allows_single_stream_fallback(
        &network_error
    ));
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

fn test_download_runtime_dir(name: &str) -> PathBuf {
    let root = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn zip_central_directory_names(path: &Path) -> Vec<String> {
    let bytes = std::fs::read(path).expect("zip should be readable");
    let eocd_index = bytes
        .windows(4)
        .rposition(|window| window == [0x50, 0x4b, 0x05, 0x06])
        .expect("zip end of central directory should exist");
    let entry_count =
        u16::from_le_bytes(bytes[eocd_index + 10..eocd_index + 12].try_into().unwrap()) as usize;
    let mut offset =
        u32::from_le_bytes(bytes[eocd_index + 16..eocd_index + 20].try_into().unwrap()) as usize;

    let mut names = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        assert_eq!(&bytes[offset..offset + 4], &[0x50, 0x4b, 0x01, 0x02]);
        let name_len =
            u16::from_le_bytes(bytes[offset + 28..offset + 30].try_into().unwrap()) as usize;
        let extra_len =
            u16::from_le_bytes(bytes[offset + 30..offset + 32].try_into().unwrap()) as usize;
        let comment_len =
            u16::from_le_bytes(bytes[offset + 32..offset + 34].try_into().unwrap()) as usize;
        let name_start = offset + 46;
        let name_end = name_start + name_len;
        names.push(String::from_utf8(bytes[name_start..name_end].to_vec()).unwrap());
        offset = name_end + extra_len + comment_len;
    }

    names
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
    let root = std::env::temp_dir().join(format!(
        "sdm-sha256-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    tokio::fs::create_dir_all(&root).await.unwrap();
    let path = root.join("hello.txt");
    tokio::fs::write(&path, b"hello").await.unwrap();

    let digest = compute_sha256(&path).await.unwrap();

    assert_eq!(
        digest,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segmented_sidecar_resets_progress_when_partial_file_is_missing() {
    let root = test_download_runtime_dir("segment-missing-partial");
    let temp_path = root.join("download.bin.part");
    let plan = RangePlan {
        total_bytes: 12,
        segments: vec![
            ByteRange { start: 0, end: 3 },
            ByteRange { start: 4, end: 7 },
            ByteRange { start: 8, end: 11 },
        ],
    };

    let mut state = load_or_create_segment_state(&temp_path, &plan)
        .await
        .unwrap();
    state.segments[0].downloaded_bytes = 4;
    state.segments[0].completed = true;
    state.segments[1].downloaded_bytes = 2;
    persist_segment_state(&temp_path, &state).await.unwrap();

    refresh_segment_completion_from_disk(&temp_path, &mut state).await;

    assert_eq!(state.segments[0].downloaded_bytes, 0);
    assert!(!state.segments[0].completed);
    assert_eq!(state.segments[1].downloaded_bytes, 0);
    assert!(!state.segments[1].completed);

    persist_segment_state(&temp_path, &state).await.unwrap();
    let reloaded = load_or_create_segment_state(&temp_path, &plan)
        .await
        .unwrap();
    assert_eq!(reloaded.segments[0].downloaded_bytes, 0);
    assert!(!reloaded.segments[0].completed);
    assert_eq!(reloaded.segments[1].downloaded_bytes, 0);
    assert!(!reloaded.segments[1].completed);

    let _ = tokio::fs::remove_dir_all(root).await;
}
