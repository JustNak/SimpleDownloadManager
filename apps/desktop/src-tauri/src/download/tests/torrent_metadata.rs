use super::*;

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
        "Torrent metadata lookup timed out after 20 seconds. Add trackers or retry later."
    );
}

#[test]
fn torrent_metadata_timeout_is_twenty_seconds() {
    assert_eq!(TORRENT_METADATA_TIMEOUT, Duration::from_secs(20));
}

#[test]
fn torrent_metadata_recovery_stages_escalate_without_generic_retries() {
    let first = torrent_metadata_recovery_stage(0);
    assert!(first.use_tracker_first);
    assert!(!first.reset_engine_before_retry);
    assert_eq!(first.timeout, TORRENT_METADATA_TIMEOUT);

    let expanded_retry = torrent_metadata_recovery_stage(1);
    assert!(expanded_retry.use_tracker_first);
    assert!(!expanded_retry.reset_engine_before_retry);
    assert_eq!(expanded_retry.timeout, TORRENT_METADATA_TIMEOUT);

    assert!(torrent_metadata_recovery_stage(2).is_final_failure);
    assert!(!torrent_metadata_recovery_stage(2).reset_engine_before_retry);
}

#[test]
fn torrent_metadata_recovery_final_message_does_not_reference_engine_reset() {
    let message = torrent_metadata_recovery_failure_message(false);

    assert!(message.contains("tracker acceleration"));
    assert!(message.contains("DHT"));
    assert!(message.contains("expanded tracker"));
    assert!(!message.contains("engine-reset"));
    assert!(!message.contains("could not reset"));
}

#[test]
fn torrent_metadata_recovery_final_error_is_terminal_torrent_failure() {
    let error = torrent_metadata_recovery_failure_error(false);

    assert_eq!(error.category, FailureCategory::Torrent);
    assert!(!error.retryable);
    assert!(error.message.contains("expanded tracker"));
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
fn torrent_metadata_timeout_cleanup_runs_before_staged_retry() {
    let source = include_str!("../torrent.rs");
    let timeout_branch = source
        .find("Err(error) if is_torrent_metadata_timeout_error(&error)")
        .expect("torrent metadata timeout branch should exist");
    let cleanup_call = source[timeout_branch..]
        .find("cleanup_pending_torrent_metadata(")
        .expect("timeout branch should clean up pending metadata")
        + timeout_branch;

    assert!(
        cleanup_call > timeout_branch,
        "pending torrent metadata cleanup must run before the next recovery stage"
    );
    assert!(source.contains("torrent_metadata_recovery_failure_message"));
}

#[test]
fn tracker_first_metadata_outcomes_have_user_visible_diagnostics() {
    assert_eq!(
        tracker_first_metadata_diagnostic_message(&TrackerFirstMetadataOutcome::Resolved {
            initial_peers: 7,
        }),
        "Tracker-first torrent metadata resolved; initial peer handoff enabled with 7 peers"
    );
    assert_eq!(
        tracker_first_metadata_diagnostic_message(&TrackerFirstMetadataOutcome::TimedOut),
        "Tracker-first torrent metadata accelerator timed out after 8 seconds; main DHT/tracker lookup is already running"
    );
    assert_eq!(
        tracker_first_metadata_diagnostic_message(&TrackerFirstMetadataOutcome::Failed(
            "tracker unavailable".into()
        )),
        "Tracker-first torrent metadata accelerator failed while main DHT/tracker lookup continued: tracker unavailable"
    );
    assert_eq!(
        tracker_first_metadata_diagnostic_message(
            &TrackerFirstMetadataOutcome::SupersededByMainSession
        ),
        "Main torrent metadata lookup resolved first; tracker-first accelerator canceled"
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
        info_hash_hint: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        original_tracker_count: 0,
        custom_trackers_added: 0,
        fallback_trackers_added: 0,
        fallback_trackers_for_options: Vec::new(),
        tracker_protocol_counts: crate::torrent::TorrentTrackerProtocolCounts::default(),
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
    let startup_update = low_throughput_update_before_first_payload();
    let update = low_throughput_update();
    let mut watchdog =
        TorrentPeerConnectionWatchdog::new(TorrentPeerConnectionWatchdogMode::Diagnose, started_at);

    assert_eq!(
        watchdog.observe(&startup_update, started_at + Duration::from_secs(5)),
        TorrentPeerConnectionWatchdogDecision::Report,
        "diagnose mode should report early peer-discovery stalls without mutating the torrent session"
    );
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
fn torrent_peer_watchdog_recover_mode_refreshes_once_then_reports() {
    let started_at = Instant::now();
    let update = low_throughput_update();
    let mut watchdog =
        TorrentPeerConnectionWatchdog::new(TorrentPeerConnectionWatchdogMode::Recover, started_at);

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
        TorrentPeerConnectionWatchdogDecision::Report,
        "recover mode should report after the first refresh instead of re-adding and forcing verification"
    );
    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(300)),
        TorrentPeerConnectionWatchdogDecision::Report,
        "recover mode should remain non-destructive on later peer stalls"
    );
}

#[test]
fn torrent_peer_watchdog_recover_mode_keeps_reporting_after_refresh() {
    let started_at = Instant::now();
    let update = low_throughput_update();
    let mut watchdog =
        TorrentPeerConnectionWatchdog::new(TorrentPeerConnectionWatchdogMode::Recover, started_at);

    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(60)),
        TorrentPeerConnectionWatchdogDecision::RefreshPeers
    );
    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(120)),
        TorrentPeerConnectionWatchdogDecision::Report
    );

    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(240)),
        TorrentPeerConnectionWatchdogDecision::Report,
        "recover mode should continue reporting instead of escalating to re-add or reset"
    );
}

#[test]
fn torrent_peer_watchdog_assist_mode_refreshes_startup_and_low_ramp_once_each() {
    let started_at = Instant::now();
    let startup_update = low_throughput_update_before_first_payload();
    let mut low_ramp_update = low_throughput_update();
    low_ramp_update.peers = Some(1);
    if let Some(diagnostics) = low_ramp_update.diagnostics.as_mut() {
        diagnostics.live_peers = 1;
        diagnostics.connecting_peers = 2;
        diagnostics.dead_peers = 61;
    }
    let mut watchdog =
        TorrentPeerConnectionWatchdog::new(TorrentPeerConnectionWatchdogMode::Assist, started_at);

    assert_eq!(
        watchdog.observe(&startup_update, started_at + Duration::from_secs(4)),
        TorrentPeerConnectionWatchdogDecision::Continue,
        "assist mode should not churn immediately before the startup discovery window"
    );
    assert_eq!(
        watchdog.observe(&startup_update, started_at + Duration::from_secs(5)),
        TorrentPeerConnectionWatchdogDecision::RefreshPeers,
        "assist mode should perform one safe refresh near the startup discovery window"
    );
    assert_eq!(
        watchdog.observe(&startup_update, started_at + Duration::from_secs(10)),
        TorrentPeerConnectionWatchdogDecision::Continue,
        "assist mode should avoid repeated startup refreshes after the early assist action"
    );
    assert_eq!(
        watchdog.observe(&low_ramp_update, started_at + Duration::from_secs(19)),
        TorrentPeerConnectionWatchdogDecision::Continue,
        "assist mode should wait for the low-ramp discovery window"
    );
    assert_eq!(
        watchdog.observe(&low_ramp_update, started_at + Duration::from_secs(20)),
        TorrentPeerConnectionWatchdogDecision::RefreshPeers,
        "assist mode should perform one later safe refresh when ramp is still constrained"
    );
    assert_eq!(
        watchdog.observe(&low_ramp_update, started_at + Duration::from_secs(25)),
        TorrentPeerConnectionWatchdogDecision::Continue,
        "assist mode should not escalate past non-destructive peer refreshes"
    );
}

#[test]
fn torrent_peer_watchdog_assist_skips_startup_refresh_when_connections_are_active() {
    let started_at = Instant::now();
    let mut update = low_throughput_update_before_first_payload();
    if let Some(diagnostics) = update.diagnostics.as_mut() {
        diagnostics.connecting_peers = 6;
    }
    let mut watchdog =
        TorrentPeerConnectionWatchdog::new(TorrentPeerConnectionWatchdogMode::Assist, started_at);

    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(5)),
        TorrentPeerConnectionWatchdogDecision::Continue,
        "assist mode should not pause/unpause while enough peer connection attempts are already in flight"
    );
}

#[test]
fn torrent_peer_watchdog_defaults_to_assist_mode() {
    assert_eq!(
        TorrentPeerConnectionWatchdogMode::default(),
        TorrentPeerConnectionWatchdogMode::Assist
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
    let source = include_str!("../torrent.rs");
    let production_source = source;
    let channel = production_source
        .find("spawn_tracker_first_metadata_diagnostics(")
        .expect("torrent add flow should create a diagnostics channel");
    let add_source = production_source[channel..]
        .find("add_prepared_torrent_with_controls(")
        .expect("torrent add flow should pass diagnostics to the controlled add helper")
        + channel;
    let argument = production_source[add_source..]
        .find("tracker_first_diagnostics,")
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

    let fallback_tracker_count = 8;
    record_fallback_tracker_usage(&state, "job_1", fallback_tracker_count, "magnet").await;

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
        format!("Added {fallback_tracker_count} fallback trackers for magnet metadata lookup")
    );
    assert_eq!(event.job_id.as_deref(), Some("job_1"));
}

#[test]
fn torrent_engine_config_tracks_immutable_session_settings_only() {
    let root = PathBuf::from("C:/sdm-test");
    let app_data_dir = root.join("data");
    let settings = crate::storage::Settings {
        download_directory: root.join("downloads").display().to_string(),
        torrent: crate::storage::TorrentSettings {
            download_directory: root.join("torrents").display().to_string(),
            upload_limit_kib_per_second: 128,
            ..Default::default()
        },
        ..Default::default()
    };

    let base = TorrentEngineConfig::from_settings(&settings, app_data_dir.clone());

    let mut upload_changed = settings.clone();
    upload_changed.torrent.upload_limit_kib_per_second = 256;
    assert_eq!(
        base,
        TorrentEngineConfig::from_settings(&upload_changed, app_data_dir.clone())
    );

    let mut listener_changed = settings;
    listener_changed.torrent.port_forwarding_enabled = true;
    listener_changed.torrent.port_forwarding_port = 42_123;
    assert_ne!(
        base,
        TorrentEngineConfig::from_settings(&listener_changed, app_data_dir)
    );
}

#[test]
fn torrent_engine_refresh_action_recreates_only_when_idle() {
    let current = TorrentEngineConfig {
        default_output_folder: PathBuf::from("C:/Downloads/Torrent"),
        data_dir: PathBuf::from("C:/Data"),
        port_forwarding_enabled: false,
        port_forwarding_port: 42_000,
    };
    let mut desired = current.clone();
    desired.port_forwarding_enabled = true;
    desired.port_forwarding_port = 42_123;

    assert_eq!(
        torrent_engine_refresh_action(None, &desired, true),
        TorrentEngineRefreshAction::Create
    );
    assert_eq!(
        torrent_engine_refresh_action(Some(&current), &current, false),
        TorrentEngineRefreshAction::Reuse
    );
    assert_eq!(
        torrent_engine_refresh_action(Some(&current), &desired, false),
        TorrentEngineRefreshAction::Recreate
    );
    assert_eq!(
        torrent_engine_refresh_action(Some(&current), &desired, true),
        TorrentEngineRefreshAction::Defer
    );
}

#[tokio::test]
async fn torrent_engine_manager_reuses_engine_for_upload_limit_only_change() {
    let root = test_download_runtime_dir("torrent-engine-upload-limit");
    let state = torrent_engine_state_for_test(
        "torrent-engine-upload-limit-state",
        &root,
        Vec::new(),
        |_| {},
    )
    .await;
    let manager = TorrentEngineManager::default();
    let first = manager.get_or_create(&state).await.unwrap();

    let mut settings = state.settings().await;
    settings.torrent.upload_limit_kib_per_second = 512;
    state.save_settings(settings).await.unwrap();
    manager.refresh_runtime_settings(&state).await.unwrap();

    let second = manager.get_or_create(&state).await.unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &second));

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn torrent_engine_manager_warms_before_claimed_torrent_workers_start() {
    let root = test_download_runtime_dir("torrent-engine-warmup");
    let state =
        torrent_engine_state_for_test("torrent-engine-warmup-state", &root, Vec::new(), |_| {})
            .await;
    let manager = TorrentEngineManager::default();
    let task = crate::state::DownloadTask {
        id: "job_torrent".into(),
        url: "magnet:?xt=urn:btih:a634dc946d49989526058626caa3bbabba4607b6".into(),
        filename: "torrent-a634dc94".into(),
        transfer_kind: TransferKind::Torrent,
        torrent: None,
        handoff_auth: None,
        resolved_from_url: None,
        source: None,
        is_bulk_member: false,
        bulk_archive_id: None,
        retry_attempts: 0,
        target_path: root.join("torrent-a634dc94"),
        temp_path: root.join("torrent-a634dc94.part"),
    };

    assert_eq!(manager.current_config().await, None);
    assert!(manager
        .warm_for_torrent_tasks(&state, std::slice::from_ref(&task))
        .await
        .expect("torrent warmup should succeed"));
    assert!(
        manager.current_config().await.is_some(),
        "torrent warmup should create the in-memory engine before worker start"
    );

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn torrent_engine_manager_drops_idle_engine_for_immutable_change_before_next_use() {
    let root = test_download_runtime_dir("torrent-engine-idle-recreate");
    let state = torrent_engine_state_for_test(
        "torrent-engine-idle-recreate-state",
        &root,
        Vec::new(),
        |_| {},
    )
    .await;
    let manager = TorrentEngineManager::default();
    let first = manager.get_or_create(&state).await.unwrap();
    let first_config = manager.current_config().await.unwrap();

    let mut settings = state.settings().await;
    settings.torrent.port_forwarding_enabled = true;
    settings.torrent.port_forwarding_port = 42_123;
    state.save_settings(settings).await.unwrap();
    manager.refresh_runtime_settings(&state).await.unwrap();

    assert_eq!(manager.current_config().await, None);
    let second = manager.get_or_create(&state).await.unwrap();
    assert!(!std::sync::Arc::ptr_eq(&first, &second));
    assert_ne!(first_config, manager.current_config().await.unwrap());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn torrent_engine_manager_defers_immutable_change_with_active_torrent_and_records_warning() {
    let root = test_download_runtime_dir("torrent-engine-active-defers");
    let state = torrent_engine_state_for_test(
        "torrent-engine-active-defers-state",
        &root,
        vec![torrent_job("job_1", JobState::Downloading)],
        |_| {},
    )
    .await;
    let manager = TorrentEngineManager::default();
    let first = manager.get_or_create(&state).await.unwrap();
    let first_config = manager.current_config().await.unwrap();

    let mut settings = state.settings().await;
    settings.torrent.port_forwarding_enabled = true;
    settings.torrent.port_forwarding_port = 42_124;
    state.save_settings(settings).await.unwrap();
    manager.refresh_runtime_settings(&state).await.unwrap();

    assert_eq!(manager.current_config().await, Some(first_config));
    let second = manager.get_or_create(&state).await.unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &second));

    let snapshot = state
        .diagnostics_snapshot(crate::storage::HostRegistrationDiagnostics {
            status: crate::storage::HostRegistrationStatus::Missing,
            entries: Vec::new(),
        })
        .await;
    assert!(snapshot.recent_events.iter().any(|event| {
        event.level == DiagnosticLevel::Warning
            && event.category == "torrent"
            && event.message.contains("restart")
    }));

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn torrent_engine_manager_cache_clear_reset_drops_idle_engine_slot() {
    let root = test_download_runtime_dir("torrent-engine-cache-reset");
    let state = torrent_engine_state_for_test(
        "torrent-engine-cache-reset-state",
        &root,
        Vec::new(),
        |_| {},
    )
    .await;
    let manager = TorrentEngineManager::default();

    let _engine = manager.get_or_create(&state).await.unwrap();
    assert!(manager.current_config().await.is_some());

    manager.clear_if_idle(&state).await.unwrap();

    assert_eq!(manager.current_config().await, None);

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn torrent_engine_manager_per_job_reset_ignores_other_queued_torrents() {
    let root = test_download_runtime_dir("torrent-engine-reset-queued-peer");
    let state = torrent_engine_state_for_test(
        "torrent-engine-reset-queued-peer-state",
        &root,
        vec![
            torrent_job("job_current", JobState::Starting),
            torrent_job("job_queued", JobState::Queued),
        ],
        |_| {},
    )
    .await;
    let manager = TorrentEngineManager::default();

    let _engine = manager.get_or_create(&state).await.unwrap();
    assert!(manager.current_config().await.is_some());

    let reset = manager
        .clear_if_no_other_torrent_work(&state, "job_current")
        .await
        .unwrap();

    assert!(
        reset,
        "other queued torrents should not block per-job engine reset"
    );
    assert_eq!(manager.current_config().await, None);

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn torrent_engine_manager_per_job_reset_blocks_other_active_torrents() {
    let root = test_download_runtime_dir("torrent-engine-reset-active-peer");
    let state = torrent_engine_state_for_test(
        "torrent-engine-reset-active-peer-state",
        &root,
        vec![
            torrent_job("job_current", JobState::Downloading),
            torrent_job("job_active", JobState::Downloading),
        ],
        |_| {},
    )
    .await;
    let manager = TorrentEngineManager::default();

    let _engine = manager.get_or_create(&state).await.unwrap();
    assert!(manager.current_config().await.is_some());

    let reset = manager
        .clear_if_no_other_torrent_work(&state, "job_current")
        .await
        .unwrap();

    assert!(
        !reset,
        "other active torrents should still block per-job engine reset"
    );
    assert!(manager.current_config().await.is_some());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[test]
fn finished_torrent_pause_releases_engine_session() {
    let mut update = torrent_runtime_update(0, 1024, 0);
    update.finished = true;

    assert!(torrent_pause_should_release_engine_session(&update));
}

#[test]
fn unfinished_torrent_pause_keeps_engine_session() {
    let update = torrent_runtime_update(0, 512, 0);

    assert!(!torrent_pause_should_release_engine_session(&update));
}

#[test]
fn cached_torrent_metadata_source_is_preferred_for_resume() {
    let storage_path = test_storage_path("torrent-cached-source-preferred");
    let app_data_dir = storage_path.parent().unwrap();
    let info_hash = "420f3778a160fbe6eb0a67c8470256be13b0ecc8";
    let metadata_path = app_data_dir
        .join("torrent-metadata")
        .join(format!("{info_hash}.torrent"));
    std::fs::create_dir_all(metadata_path.parent().unwrap()).unwrap();
    std::fs::write(
        &metadata_path,
        b"d8:announce13:http://tracker4:info4:name4:teste",
    )
    .unwrap();
    let mut job = torrent_job("job_1", JobState::Paused);
    job.url = format!("magnet:?xt=urn:btih:{info_hash}");
    let task = crate::state::DownloadTask {
        id: job.id,
        url: job.url,
        filename: job.filename,
        transfer_kind: job.transfer_kind,
        torrent: Some(TorrentInfo {
            info_hash: Some(info_hash.into()),
            ..TorrentInfo::default()
        }),
        handoff_auth: None,
        resolved_from_url: None,
        source: None,
        is_bulk_member: false,
        bulk_archive_id: None,
        retry_attempts: 0,
        target_path: PathBuf::from(job.target_path),
        temp_path: PathBuf::from(job.temp_path),
    };

    let prepared = prepare_torrent_source_for_task(&task, app_data_dir);

    assert_eq!(prepared.source_kind, TorrentSourceKind::TorrentFile);
    assert_eq!(prepared.source, metadata_path.display().to_string());

    let _ = std::fs::remove_dir_all(app_data_dir);
}

#[test]
fn cached_torrent_metadata_source_falls_back_to_original_source_when_absent() {
    let storage_path = test_storage_path("torrent-cached-source-absent");
    let app_data_dir = storage_path.parent().unwrap();
    let info_hash = "420f3778a160fbe6eb0a67c8470256be13b0ecc8";
    let magnet = format!("magnet:?xt=urn:btih:{info_hash}");
    let mut job = torrent_job("job_1", JobState::Paused);
    job.url = magnet.clone();
    let task = crate::state::DownloadTask {
        id: job.id,
        url: job.url,
        filename: job.filename,
        transfer_kind: job.transfer_kind,
        torrent: Some(TorrentInfo {
            info_hash: Some(info_hash.into()),
            ..TorrentInfo::default()
        }),
        handoff_auth: None,
        resolved_from_url: None,
        source: None,
        is_bulk_member: false,
        bulk_archive_id: None,
        retry_attempts: 0,
        target_path: PathBuf::from(job.target_path),
        temp_path: PathBuf::from(job.temp_path),
    };

    let prepared = prepare_torrent_source_for_task(&task, app_data_dir);

    assert_eq!(prepared.source_kind, TorrentSourceKind::Magnet);
    assert!(prepared.source.starts_with(&magnet));

    let _ = std::fs::remove_dir_all(app_data_dir);
}

#[test]
fn cached_torrent_metadata_source_uses_latest_info_hash_when_task_snapshot_is_stale() {
    let storage_path = test_storage_path("torrent-cached-source-latest-info-hash");
    let app_data_dir = storage_path.parent().unwrap();
    let info_hash = "420f3778a160fbe6eb0a67c8470256be13b0ecc8";
    let metadata_path = app_data_dir
        .join("torrent-metadata")
        .join(format!("{info_hash}.torrent"));
    std::fs::create_dir_all(metadata_path.parent().unwrap()).unwrap();
    std::fs::write(
        &metadata_path,
        b"d8:announce13:http://tracker4:info4:name4:teste",
    )
    .unwrap();
    let mut job = torrent_job("job_1", JobState::Paused);
    job.url = format!("magnet:?xt=urn:btih:{info_hash}");
    let task = crate::state::DownloadTask {
        id: job.id,
        url: job.url,
        filename: job.filename,
        transfer_kind: job.transfer_kind,
        torrent: None,
        handoff_auth: None,
        resolved_from_url: None,
        source: None,
        is_bulk_member: false,
        bulk_archive_id: None,
        retry_attempts: 0,
        target_path: PathBuf::from(job.target_path),
        temp_path: PathBuf::from(job.temp_path),
    };

    let prepared =
        prepare_torrent_source_for_task_with_info_hash(&task, app_data_dir, Some(info_hash));

    assert_eq!(prepared.source_kind, TorrentSourceKind::TorrentFile);
    assert_eq!(prepared.source, metadata_path.display().to_string());

    let _ = std::fs::remove_dir_all(app_data_dir);
}
