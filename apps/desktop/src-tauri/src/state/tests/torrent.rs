use super::*;

#[tokio::test]
async fn prepare_external_use_pauses_seeding_torrent_and_waits_for_worker_release() {
    let download_dir = test_runtime_dir("prepare-seeding-external-use");
    let target_path = download_dir.join("Example Torrent");
    std::fs::create_dir_all(&target_path).unwrap();
    let mut job = download_job("job_32", JobState::Seeding, ResumeSupport::Unsupported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.progress = 100.0;
    job.target_path = target_path.display().to_string();
    job.temp_path = download_dir
        .join(".torrent-state")
        .join("job_32")
        .display()
        .to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_32".into());
    }

    let release_state = state.clone();
    let release_handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(25)).await;
        let mut runtime = release_state.inner.write().await;
        runtime.active_workers.remove("job_32");
    });

    let preparation = state
        .prepare_job_for_external_use_with_wait(
            "job_32",
            Duration::from_secs(1),
            Duration::from_millis(5),
        )
        .await
        .expect("seeding torrent should be prepared for external use");
    release_handle.await.unwrap();

    assert!(preparation.paused_torrent);
    let runtime = state.inner.read().await;
    let job = &runtime.jobs[0];
    assert_eq!(job.state, JobState::Paused);
    assert!(!runtime.active_workers.contains("job_32"));
    drop(runtime);

    let resolved = state.resolve_openable_path("job_32").await.unwrap();
    assert_eq!(resolved, target_path);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn prepare_external_use_leaves_non_seeding_jobs_running_state_unchanged() {
    let download_dir = test_runtime_dir("prepare-non-seeding-external-use");
    let http_job = download_job(
        "job_33",
        JobState::Downloading,
        ResumeSupport::Supported,
        20,
    );
    let mut completed_torrent =
        download_job("job_34", JobState::Completed, ResumeSupport::Supported, 100);
    completed_torrent.transfer_kind = TransferKind::Torrent;
    completed_torrent.target_path = download_dir.join("Done").display().to_string();
    completed_torrent.temp_path = download_dir
        .join(".torrent-state")
        .join("job_34")
        .display()
        .to_string();
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![http_job, completed_torrent],
    );

    let http_preparation = state
        .prepare_job_for_external_use_with_wait(
            "job_33",
            Duration::from_millis(50),
            Duration::from_millis(5),
        )
        .await
        .expect("http job should not need torrent preparation");
    let torrent_preparation = state
        .prepare_job_for_external_use_with_wait(
            "job_34",
            Duration::from_millis(50),
            Duration::from_millis(5),
        )
        .await
        .expect("completed torrent should not need pausing");

    assert!(!http_preparation.paused_torrent);
    assert!(!torrent_preparation.paused_torrent);
    let runtime = state.inner.read().await;
    assert_eq!(runtime.jobs[0].state, JobState::Downloading);
    assert_eq!(runtime.jobs[1].state, JobState::Completed);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn external_reseed_attempt_queues_paused_torrent() {
    let download_dir = test_runtime_dir("external-reseed-queues-paused");
    let mut job = download_job("job_35a", JobState::Paused, ResumeSupport::Unsupported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.progress = 100.0;
    job.target_path = download_dir.join("torrent").display().to_string();
    job.temp_path = download_dir.join(".torrent-state").display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    state.begin_external_reseed("job_35a").await;
    let attempt = state
        .queue_external_reseed_attempt("job_35a")
        .await
        .expect("external reseed queue attempt should succeed");

    assert!(matches!(attempt, ExternalReseedAttempt::Queued(_)));
    let runtime = state.inner.read().await;
    assert_eq!(runtime.jobs[0].state, JobState::Queued);
    assert!(state.is_external_reseed_pending("job_35a").await);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn external_reseed_file_access_failure_restores_paused_for_retry() {
    let download_dir = test_runtime_dir("external-reseed-file-lock-retry");
    let mut job = download_job("job_35b", JobState::Paused, ResumeSupport::Unsupported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.progress = 100.0;
    job.target_path = download_dir.join("torrent").display().to_string();
    job.temp_path = download_dir.join(".torrent-state").display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    state.begin_external_reseed("job_35b").await;
    {
        let mut runtime = state.inner.write().await;
        runtime.jobs[0].state = JobState::Starting;
        runtime.active_workers.insert("job_35b".into());
    }

    let snapshot = state
        .handle_external_reseed_failure(
            "job_35b",
            "The process cannot access the file because it is being used by another process.",
            FailureCategory::Torrent,
        )
        .await
        .expect("external reseed failure handling should succeed")
        .expect("file access failures should be retried");

    assert_eq!(snapshot.jobs[0].state, JobState::Paused);
    assert!(snapshot.jobs[0]
        .error
        .as_deref()
        .unwrap()
        .contains("being used"));
    let runtime = state.inner.read().await;
    assert!(!runtime.active_workers.contains("job_35b"));
    drop(runtime);
    assert!(state.is_external_reseed_pending("job_35b").await);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn external_reseed_non_file_access_failure_is_not_intercepted() {
    let download_dir = test_runtime_dir("external-reseed-non-file-error");
    let mut job = download_job("job_35c", JobState::Paused, ResumeSupport::Unsupported, 100);
    job.transfer_kind = TransferKind::Torrent;
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    state.begin_external_reseed("job_35c").await;
    {
        let mut runtime = state.inner.write().await;
        runtime.jobs[0].state = JobState::Starting;
        runtime.active_workers.insert("job_35c".into());
    }
    let handled = state
        .handle_external_reseed_failure(
            "job_35c",
            "Torrent metadata is invalid.",
            FailureCategory::Torrent,
        )
        .await
        .expect("external reseed failure handling should succeed");

    assert!(handled.is_none());
    assert!(state.is_external_reseed_pending("job_35c").await);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_session_cache_clear_blocks_active_torrents() {
    let download_dir = test_runtime_dir("torrent-cache-active");
    let mut active_job = download_job("job_1", JobState::Seeding, ResumeSupport::Unsupported, 100);
    active_job.transfer_kind = TransferKind::Torrent;
    active_job.torrent = Some(TorrentInfo {
        engine_id: Some(7),
        info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        uploaded_bytes: 2048,
        ratio: 1.0,
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active_job]);

    let error = state
        .prepare_torrent_session_cache_clear()
        .await
        .expect_err("active torrents should block cache clearing");

    assert_eq!(error.code, "TORRENT_CACHE_ACTIVE");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_session_cache_clear_blocks_active_torrent_worker_even_when_paused() {
    let download_dir = test_runtime_dir("torrent-cache-active-worker");
    let mut paused_job = download_job("job_1", JobState::Paused, ResumeSupport::Unsupported, 100);
    paused_job.transfer_kind = TransferKind::Torrent;
    paused_job.torrent = Some(TorrentInfo {
        engine_id: Some(7),
        info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        uploaded_bytes: 2048,
        ratio: 1.0,
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![paused_job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_1".into());
    }

    let error = state
        .prepare_torrent_session_cache_clear()
        .await
        .expect_err("active torrent workers should block cache clearing");

    assert_eq!(error.code, "TORRENT_CACHE_ACTIVE");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_session_cache_clear_resets_runtime_identity_and_reseed_state() {
    let download_dir = test_runtime_dir("torrent-cache-runtime-reset");
    let mut paused_job = download_job("job_1", JobState::Paused, ResumeSupport::Unsupported, 100);
    paused_job.transfer_kind = TransferKind::Torrent;
    paused_job.downloaded_bytes = 4096;
    paused_job.torrent = Some(TorrentInfo {
        engine_id: Some(7),
        info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        uploaded_bytes: 2048,
        last_runtime_uploaded_bytes: Some(1024),
        fetched_bytes: 512,
        last_runtime_fetched_bytes: Some(256),
        ratio: 0.5,
        seeding_started_at: Some(1234),
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![paused_job]);
    state.begin_external_reseed("job_1").await;

    let result = state
        .prepare_torrent_session_cache_clear()
        .await
        .expect("paused torrents should allow cache clearing");

    assert_eq!(result.torrents.len(), 1);
    assert_eq!(result.torrents[0].engine_id, Some(7));
    let snapshot_job = result
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_1")
        .expect("job should remain persisted");
    let torrent = snapshot_job.torrent.as_ref().expect("torrent metadata");
    assert_eq!(torrent.engine_id, None);
    assert_eq!(
        torrent.info_hash.as_deref(),
        Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8")
    );
    assert_eq!(torrent.uploaded_bytes, 2048);
    assert_eq!(torrent.fetched_bytes, 512);
    assert_eq!(torrent.seeding_started_at, Some(1234));
    assert_eq!(torrent.last_runtime_uploaded_bytes, None);
    assert_eq!(torrent.last_runtime_fetched_bytes, None);
    assert!(!state.is_external_reseed_pending("job_1").await);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn released_seeding_pause_clears_engine_identity_and_preserves_seed_state() {
    let download_dir = test_runtime_dir("torrent-release-seeding-pause");
    let storage_path = download_dir.join("state.json");
    let mut paused_job = download_job("job_1", JobState::Paused, ResumeSupport::Unsupported, 100);
    paused_job.transfer_kind = TransferKind::Torrent;
    paused_job.downloaded_bytes = 4096;
    paused_job.total_bytes = 4096;
    paused_job.torrent = Some(TorrentInfo {
        engine_id: Some(7),
        info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        uploaded_bytes: 2048,
        last_runtime_uploaded_bytes: Some(1024),
        fetched_bytes: 512,
        last_runtime_fetched_bytes: Some(256),
        ratio: 0.5,
        seeding_started_at: Some(1234),
        diagnostics: Some(TorrentRuntimeDiagnostics::default()),
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(storage_path.clone(), vec![paused_job]);

    let snapshot = state
        .mark_torrent_engine_session_released("job_1")
        .await
        .expect("released seeding pause should update torrent identity");

    let job = &snapshot.jobs[0];
    assert_eq!(job.state, JobState::Paused);
    assert_eq!(job.downloaded_bytes, 4096);
    let torrent = job.torrent.as_ref().expect("torrent metadata");
    assert_eq!(torrent.engine_id, None);
    assert_eq!(
        torrent.info_hash.as_deref(),
        Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8")
    );
    assert_eq!(torrent.uploaded_bytes, 2048);
    assert_eq!(torrent.fetched_bytes, 512);
    assert_eq!(torrent.ratio, 0.5);
    assert_eq!(torrent.seeding_started_at, Some(1234));
    assert_eq!(torrent.last_runtime_uploaded_bytes, None);
    assert_eq!(torrent.last_runtime_fetched_bytes, None);
    assert_eq!(torrent.diagnostics, None);

    let persisted = load_persisted_state(&storage_path).expect("persisted state should load");
    let persisted_torrent = persisted.jobs[0]
        .torrent
        .as_ref()
        .expect("persisted torrent metadata");
    assert_eq!(persisted_torrent.engine_id, None);
    assert_eq!(persisted_torrent.uploaded_bytes, 2048);
    assert_eq!(persisted_torrent.seeding_started_at, Some(1234));

    let runtime = state.inner.read().await;
    assert!(runtime.diagnostic_events.iter().any(|event| {
        event.level == DiagnosticLevel::Info
            && event.category == "torrent"
            && event.message.contains("Released torrent engine session")
    }));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_pause_requires_release_wait_only_for_active_seeding_torrent() {
    let download_dir = test_runtime_dir("torrent-pause-release-required");
    let mut seeding_job = download_job("job_1", JobState::Seeding, ResumeSupport::Unsupported, 100);
    seeding_job.transfer_kind = TransferKind::Torrent;
    seeding_job.torrent = Some(TorrentInfo::default());
    let mut paused_job = download_job("job_2", JobState::Paused, ResumeSupport::Unsupported, 100);
    paused_job.transfer_kind = TransferKind::Torrent;
    paused_job.torrent = Some(TorrentInfo::default());
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![seeding_job, paused_job],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_1".into());
        runtime.active_workers.insert("job_2".into());
    }

    assert!(state
        .torrent_pause_requires_worker_release("job_1")
        .await
        .expect("seeding torrent should be inspected"));
    assert!(!state
        .torrent_pause_requires_worker_release("job_2")
        .await
        .expect("paused torrent should be inspected"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn torrent_session_cache_directory_clear_removes_session_without_payload() {
    let download_dir = test_runtime_dir("torrent-cache-directory-clear");
    let session_dir = download_dir.join("torrent-session");
    let payload_dir = download_dir.join("Torrent").join("Fedora");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::create_dir_all(&payload_dir).unwrap();
    std::fs::write(session_dir.join("session.json"), b"stale").unwrap();
    std::fs::write(payload_dir.join("payload.bin"), b"payload").unwrap();

    let result = clear_torrent_session_cache_directory(&download_dir)
        .expect("session cache clear should finish");

    assert!(result.cleared);
    assert!(!result.pending_restart);
    assert_eq!(result.session_path, session_dir.display().to_string());
    assert!(!session_dir.exists());
    assert!(payload_dir.join("payload.bin").exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn pending_torrent_session_cache_clear_runs_before_startup() {
    let download_dir = test_runtime_dir("torrent-cache-pending-startup");
    let session_dir = download_dir.join("torrent-session");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::write(session_dir.join("session.json"), b"stale").unwrap();
    std::fs::write(
        pending_torrent_session_cache_clear_path(&download_dir),
        b"pending",
    )
    .unwrap();

    apply_pending_torrent_session_cache_clear(&download_dir);

    assert!(!session_dir.exists());
    assert!(!pending_torrent_session_cache_clear_path(&download_dir).exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_upload_counter_preserves_migrated_total_on_first_runtime_snapshot() {
    let download_dir = test_runtime_dir("torrent-upload-migrated-total");
    let mut job = download_job("job_36", JobState::Seeding, ResumeSupport::Unsupported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.downloaded_bytes = 1024;
    job.total_bytes = 1024;
    job.torrent = Some(TorrentInfo {
        uploaded_bytes: 2048,
        ratio: 2.0,
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    state
        .update_torrent_progress("job_36", torrent_runtime_update(128, 1024, true), false)
        .await
        .expect("torrent progress should update");
    let snapshot = state
        .update_torrent_progress("job_36", torrent_runtime_update(384, 1024, true), false)
        .await
        .expect("torrent progress should update");

    let torrent = snapshot.jobs[0].torrent.as_ref().expect("torrent metadata");
    assert_eq!(torrent.uploaded_bytes, 2304);
    assert_eq!(torrent.last_runtime_uploaded_bytes, Some(384));
    assert_eq!(torrent.ratio, 2.25);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_upload_counter_adds_new_runtime_epoch_after_reset() {
    let download_dir = test_runtime_dir("torrent-upload-runtime-reset");
    let mut job = download_job("job_37", JobState::Seeding, ResumeSupport::Unsupported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.downloaded_bytes = 1024;
    job.total_bytes = 1024;
    job.torrent = Some(TorrentInfo {
        uploaded_bytes: 2048,
        ratio: 2.0,
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    state
        .update_torrent_progress("job_37", torrent_runtime_update(700, 1024, true), false)
        .await
        .expect("torrent progress should update");
    state
        .update_torrent_progress("job_37", torrent_runtime_update(50, 1024, true), false)
        .await
        .expect("torrent progress should update");
    let snapshot = state
        .update_torrent_progress("job_37", torrent_runtime_update(80, 1024, true), false)
        .await
        .expect("torrent progress should update");

    let torrent = snapshot.jobs[0].torrent.as_ref().expect("torrent metadata");
    assert_eq!(torrent.uploaded_bytes, 2128);
    assert_eq!(torrent.last_runtime_uploaded_bytes, Some(80));
    assert_eq!(torrent.ratio, 2128.0 / 1024.0);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_upload_counter_tracks_deltas_without_double_counting_runtime_total() {
    let download_dir = test_runtime_dir("torrent-upload-runtime-deltas");
    let mut job = download_job("job_38", JobState::Seeding, ResumeSupport::Unsupported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.downloaded_bytes = 1024;
    job.total_bytes = 1024;
    job.torrent = Some(TorrentInfo::default());
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    state
        .update_torrent_progress("job_38", torrent_runtime_update(100, 1024, true), false)
        .await
        .expect("torrent progress should update");
    let snapshot = state
        .update_torrent_progress("job_38", torrent_runtime_update(150, 1024, true), false)
        .await
        .expect("torrent progress should update");

    let torrent = snapshot.jobs[0].torrent.as_ref().expect("torrent metadata");
    assert_eq!(torrent.uploaded_bytes, 150);
    assert_eq!(torrent.last_runtime_uploaded_bytes, Some(150));
    assert_eq!(torrent.ratio, 150.0 / 1024.0);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_fetched_counter_tracks_deltas_without_progress_jumps() {
    let download_dir = test_runtime_dir("torrent-fetched-runtime-deltas");
    let mut job = download_job(
        "job_39a",
        JobState::Downloading,
        ResumeSupport::Unsupported,
        0,
    );
    job.transfer_kind = TransferKind::Torrent;
    job.downloaded_bytes = 10 * 1024 * 1024 * 1024;
    job.total_bytes = 20 * 1024 * 1024 * 1024;
    job.torrent = Some(TorrentInfo::default());
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let mut first_update = torrent_runtime_update(0, 10 * 1024 * 1024 * 1024, false);
    first_update.total_bytes = 20 * 1024 * 1024 * 1024;
    first_update.fetched_bytes = 512 * 1024;
    state
        .update_torrent_progress("job_39a", first_update, false)
        .await
        .expect("torrent progress should update");

    let mut second_update = torrent_runtime_update(0, 10 * 1024 * 1024 * 1024, false);
    second_update.total_bytes = 20 * 1024 * 1024 * 1024;
    second_update.fetched_bytes = 2 * 1024 * 1024;
    let snapshot = state
        .update_torrent_progress("job_39a", second_update, false)
        .await
        .expect("torrent progress should update");

    let torrent = snapshot.jobs[0].torrent.as_ref().expect("torrent metadata");
    assert_eq!(snapshot.jobs[0].downloaded_bytes, 10 * 1024 * 1024 * 1024);
    assert_eq!(torrent.fetched_bytes, 2 * 1024 * 1024);
    assert_eq!(torrent.last_runtime_fetched_bytes, Some(2 * 1024 * 1024));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_fetched_counter_adds_new_runtime_epoch_after_reset() {
    let download_dir = test_runtime_dir("torrent-fetched-runtime-reset");
    let mut job = download_job(
        "job_39b",
        JobState::Downloading,
        ResumeSupport::Unsupported,
        0,
    );
    job.transfer_kind = TransferKind::Torrent;
    job.downloaded_bytes = 1024;
    job.total_bytes = 4096;
    job.torrent = Some(TorrentInfo {
        fetched_bytes: 2048,
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let mut first_update = torrent_runtime_update(0, 1024, false);
    first_update.total_bytes = 4096;
    first_update.fetched_bytes = 700;
    state
        .update_torrent_progress("job_39b", first_update, false)
        .await
        .expect("torrent progress should update");
    let mut reset_update = torrent_runtime_update(0, 1024, false);
    reset_update.total_bytes = 4096;
    reset_update.fetched_bytes = 50;
    state
        .update_torrent_progress("job_39b", reset_update, false)
        .await
        .expect("torrent progress should update");
    let mut final_update = torrent_runtime_update(0, 1024, false);
    final_update.total_bytes = 4096;
    final_update.fetched_bytes = 80;
    let snapshot = state
        .update_torrent_progress("job_39b", final_update, false)
        .await
        .expect("torrent progress should update");

    let torrent = snapshot.jobs[0].torrent.as_ref().expect("torrent metadata");
    assert_eq!(torrent.fetched_bytes, 2128);
    assert_eq!(torrent.last_runtime_fetched_bytes, Some(80));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_progress_keeps_live_download_speed_until_seeding() {
    let download_dir = test_runtime_dir("torrent-progress-live-speed");
    let mut job = download_job(
        "job_39",
        JobState::Downloading,
        ResumeSupport::Unsupported,
        0,
    );
    job.transfer_kind = TransferKind::Torrent;
    job.torrent = Some(TorrentInfo::default());
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let mut update = torrent_runtime_update(0, 1024, false);
    update.total_bytes = 4096;
    update.download_speed = 123_456;
    let snapshot = state
        .update_torrent_progress("job_39", update.clone(), false)
        .await
        .expect("torrent progress should update");

    assert_eq!(snapshot.jobs[0].state, JobState::Downloading);
    assert_eq!(snapshot.jobs[0].speed, 123_456);

    let mut finished_update = update;
    finished_update.downloaded_bytes = 4096;
    finished_update.total_bytes = 4096;
    finished_update.download_speed = 999_999;
    finished_update.upload_speed = 456_789;
    finished_update.finished = true;
    let snapshot = state
        .update_torrent_progress("job_39", finished_update, false)
        .await
        .expect("torrent progress should update");

    assert_eq!(snapshot.jobs[0].state, JobState::Seeding);
    assert_eq!(snapshot.jobs[0].speed, 456_789);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_progress_delta_keeps_live_download_speed_until_seeding() {
    let download_dir = test_runtime_dir("torrent-progress-delta-live-speed");
    let mut job = download_job(
        "job_39_delta",
        JobState::Downloading,
        ResumeSupport::Unsupported,
        0,
    );
    job.transfer_kind = TransferKind::Torrent;
    job.torrent = Some(TorrentInfo::default());
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let mut update = torrent_runtime_update(0, 1024, false);
    update.total_bytes = 4096;
    update.download_speed = 123_456;
    let delta = state
        .update_torrent_progress_delta("job_39_delta", update.clone(), false)
        .await
        .expect("torrent progress delta should update");

    assert_eq!(delta.job.state, JobState::Downloading);
    assert_eq!(delta.job.speed, 123_456);
    assert_eq!(delta.job.eta, 0);

    let mut finished_update = update;
    finished_update.downloaded_bytes = 4096;
    finished_update.total_bytes = 4096;
    finished_update.download_speed = 999_999;
    finished_update.upload_speed = 456_789;
    finished_update.finished = true;
    let delta = state
        .update_torrent_progress_delta("job_39_delta", finished_update, false)
        .await
        .expect("torrent progress delta should update");

    assert_eq!(delta.job.state, JobState::Seeding);
    assert_eq!(delta.job.speed, 456_789);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_progress_uses_runtime_eta_until_seeding() {
    let download_dir = test_runtime_dir("torrent-progress-live-eta");
    let mut job = download_job(
        "job_39c",
        JobState::Downloading,
        ResumeSupport::Unsupported,
        0,
    );
    job.transfer_kind = TransferKind::Torrent;
    job.torrent = Some(TorrentInfo::default());
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let mut update = torrent_runtime_update(0, 1024, false);
    update.total_bytes = 4096;
    update.eta = Some(321);
    let snapshot = state
        .update_torrent_progress("job_39c", update.clone(), false)
        .await
        .expect("torrent progress should update");

    assert_eq!(snapshot.jobs[0].state, JobState::Downloading);
    assert_eq!(snapshot.jobs[0].eta, 321);

    let mut finished_update = update;
    finished_update.downloaded_bytes = 4096;
    finished_update.total_bytes = 4096;
    finished_update.finished = true;
    finished_update.eta = Some(999);
    let snapshot = state
        .update_torrent_progress("job_39c", finished_update, false)
        .await
        .expect("torrent progress should update");

    assert_eq!(snapshot.jobs[0].state, JobState::Seeding);
    assert_eq!(snapshot.jobs[0].eta, 0);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_final_pause_snapshot_updates_counters_without_unpausing_job() {
    let download_dir = test_runtime_dir("torrent-final-pause-snapshot");
    let storage_path = download_dir.join("state.json");
    let mut job = download_job("job_39d", JobState::Paused, ResumeSupport::Unsupported, 25);
    job.transfer_kind = TransferKind::Torrent;
    job.downloaded_bytes = 1024;
    job.total_bytes = 4096;
    job.speed = 0;
    job.eta = 0;
    job.torrent = Some(TorrentInfo {
        uploaded_bytes: 100,
        last_runtime_uploaded_bytes: Some(40),
        fetched_bytes: 200,
        last_runtime_fetched_bytes: Some(80),
        ratio: 100.0 / 1024.0,
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(storage_path.clone(), vec![job]);

    let mut final_update = torrent_runtime_update(70, 2048, false);
    final_update.total_bytes = 4096;
    final_update.fetched_bytes = 180;
    final_update.download_speed = 55_000;
    final_update.eta = Some(120);
    let snapshot = state
        .update_torrent_progress("job_39d", final_update, true)
        .await
        .expect("final torrent pause snapshot should persist");

    assert_eq!(snapshot.jobs[0].state, JobState::Paused);
    assert_eq!(snapshot.jobs[0].downloaded_bytes, 2048);
    assert_eq!(snapshot.jobs[0].speed, 0);
    assert_eq!(snapshot.jobs[0].eta, 0);
    let torrent = snapshot.jobs[0].torrent.as_ref().expect("torrent metadata");
    assert_eq!(torrent.uploaded_bytes, 130);
    assert_eq!(torrent.fetched_bytes, 300);
    assert_eq!(torrent.ratio, 130.0 / 2048.0);

    let persisted = load_persisted_state(&storage_path).expect("persisted state should load");
    let persisted_torrent = persisted.jobs[0]
        .torrent
        .as_ref()
        .expect("persisted torrent metadata");
    assert_eq!(persisted.jobs[0].state, JobState::Paused);
    assert_eq!(persisted.jobs[0].downloaded_bytes, 2048);
    assert_eq!(persisted_torrent.uploaded_bytes, 130);
    assert_eq!(persisted_torrent.fetched_bytes, 300);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn seeding_restore_file_access_failure_pauses_and_queues_external_retry() {
    let download_dir = test_runtime_dir("seeding-restore-file-access");
    let mut job = download_job("job_1", JobState::Starting, ResumeSupport::Unsupported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.target_path = download_dir.join("torrent-output").display().to_string();
    job.temp_path = download_dir
        .join(".torrent-state")
        .join("job_1")
        .display()
        .to_string();
    job.torrent = Some(TorrentInfo {
        engine_id: Some(42),
        info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        uploaded_bytes: 2048,
        fetched_bytes: 1024,
        ratio: 2.0,
        seeding_started_at: Some(123_456),
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_1".into());
    }

    let handled = state
        .handle_torrent_seeding_restore_failure(
            "job_1",
            "The process cannot access the file because it is being used by another process.",
            FailureCategory::Torrent,
        )
        .await
        .expect("restore failure should be handled")
        .expect("seeding restore should pause instead of failing");

    assert!(handled.retry_reseed);
    assert_eq!(handled.snapshot.jobs[0].state, JobState::Paused);
    assert_ne!(handled.snapshot.jobs[0].state, JobState::Failed);
    assert_eq!(
        handled.snapshot.jobs[0]
            .torrent
            .as_ref()
            .and_then(|torrent| torrent.seeding_started_at),
        Some(123_456)
    );
    let runtime = state.inner.read().await;
    assert!(!runtime.active_workers.contains("job_1"));
    assert!(runtime.external_reseed_jobs.contains("job_1"));
    assert!(runtime
        .diagnostic_events
        .iter()
        .any(|event| event.level == DiagnosticLevel::Warning
            && event.message.contains("Automatic seeding restore")
            && event.message.contains("waiting for external file access")));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn seeding_restore_non_file_failure_pauses_with_attention_instead_of_failing() {
    let download_dir = test_runtime_dir("seeding-restore-non-file");
    let mut job = download_job("job_1", JobState::Starting, ResumeSupport::Unsupported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.torrent = Some(TorrentInfo {
        info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        seeding_started_at: Some(123_456),
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_1".into());
    }

    let handled = state
        .handle_torrent_seeding_restore_failure(
            "job_1",
            "Torrent metadata lookup timed out after 60 seconds. Add trackers or retry later.",
            FailureCategory::Torrent,
        )
        .await
        .expect("restore failure should be handled")
        .expect("seeding restore should pause instead of failing");

    assert!(!handled.retry_reseed);
    assert_eq!(handled.snapshot.jobs[0].state, JobState::Paused);
    assert_eq!(
        handled.snapshot.jobs[0].failure_category,
        Some(FailureCategory::Torrent)
    );
    assert_eq!(
        handled.snapshot.jobs[0]
            .torrent
            .as_ref()
            .and_then(|torrent| torrent.seeding_started_at),
        Some(123_456)
    );
    let runtime = state.inner.read().await;
    assert!(!runtime.active_workers.contains("job_1"));
    assert!(!runtime.external_reseed_jobs.contains("job_1"));
    assert!(runtime
        .diagnostic_events
        .iter()
        .any(|event| event.level == DiagnosticLevel::Warning
            && event.message.contains("Paused seeding restore")));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn stale_torrent_completion_reset_requeues_verification_without_losing_identity() {
    let download_dir = test_runtime_dir("stale-torrent-reset");
    let mut job = download_job("job_1", JobState::Seeding, ResumeSupport::Unsupported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.url = "magnet:?xt=urn:btih:420f3778a160fbe6eb0a67c8470256be13b0ecc8".into();
    job.target_path = download_dir.join("empty-output").display().to_string();
    job.temp_path = download_dir
        .join(".torrent-state")
        .join("job_1")
        .display()
        .to_string();
    job.downloaded_bytes = 8 * 1024;
    job.total_bytes = 8 * 1024;
    job.speed = 4096;
    job.eta = 15;
    job.error = Some("previous false seeding".into());
    job.failure_category = Some(FailureCategory::Torrent);
    job.torrent = Some(TorrentInfo {
        engine_id: Some(42),
        info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        name: Some("Stale Torrent".into()),
        total_files: Some(2),
        peers: Some(0),
        seeds: None,
        uploaded_bytes: 0,
        last_runtime_uploaded_bytes: Some(0),
        fetched_bytes: 0,
        last_runtime_fetched_bytes: Some(0),
        ratio: 0.0,
        seeding_started_at: Some(123_456),
        diagnostics: None,
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_1".into());
    }

    let snapshot = state
        .reset_stale_torrent_completion_for_recheck(
            "job_1",
            Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        )
        .await
        .expect("stale torrent completion should reset");

    let job = &snapshot.jobs[0];
    assert_eq!(job.state, JobState::Starting);
    assert_eq!(job.downloaded_bytes, 0);
    assert_eq!(job.total_bytes, 0);
    assert_eq!(job.progress, 0.0);
    assert_eq!(job.speed, 0);
    assert_eq!(job.eta, 0);
    assert_eq!(job.error, None);
    assert_eq!(job.failure_category, None);

    let torrent = job.torrent.as_ref().expect("torrent metadata remains");
    assert_eq!(
        torrent.info_hash.as_deref(),
        Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8")
    );
    assert_eq!(torrent.name.as_deref(), Some("Stale Torrent"));
    assert_eq!(torrent.engine_id, None);
    assert_eq!(torrent.uploaded_bytes, 0);
    assert_eq!(torrent.last_runtime_uploaded_bytes, None);
    assert_eq!(torrent.fetched_bytes, 0);
    assert_eq!(torrent.last_runtime_fetched_bytes, None);
    assert_eq!(torrent.ratio, 0.0);
    assert_eq!(torrent.seeding_started_at, None);

    let runtime = state.inner.read().await;
    assert!(runtime.active_workers.contains("job_1"));
    assert!(runtime
        .diagnostic_events
        .iter()
        .any(|event| event.level == DiagnosticLevel::Warning
            && event.message.contains("Cleared stale torrent verification")));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn torrent_restore_runtime_reset_preserves_seeding_history() {
    let download_dir = test_runtime_dir("restore-runtime-reset");
    let mut job = download_job(
        "job_1",
        JobState::Downloading,
        ResumeSupport::Unsupported,
        100,
    );
    job.transfer_kind = TransferKind::Torrent;
    job.downloaded_bytes = 8 * 1024;
    job.total_bytes = 8 * 1024;
    job.progress = 100.0;
    job.speed = 1024;
    job.eta = 5;
    job.error = Some("previous restore issue".into());
    job.failure_category = Some(FailureCategory::Torrent);
    job.torrent = Some(TorrentInfo {
        engine_id: Some(42),
        info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        name: Some("Ubuntu".into()),
        total_files: Some(1),
        peers: Some(12),
        seeds: Some(3),
        uploaded_bytes: 5 * 1024,
        last_runtime_uploaded_bytes: Some(1024),
        fetched_bytes: 2 * 1024,
        last_runtime_fetched_bytes: Some(512),
        ratio: 2.5,
        seeding_started_at: Some(123_456),
        diagnostics: Some(TorrentRuntimeDiagnostics::default()),
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let snapshot = state
        .reset_torrent_restore_runtime_for_recheck(
            "job_1",
            Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        )
        .await
        .expect("restore runtime reset should succeed");

    let job = &snapshot.jobs[0];
    assert_eq!(job.state, JobState::Starting);
    assert_eq!(job.downloaded_bytes, 0);
    assert_eq!(job.total_bytes, 0);
    assert_eq!(job.progress, 0.0);
    assert_eq!(job.error, None);
    assert_eq!(job.failure_category, None);
    let torrent = job.torrent.as_ref().expect("torrent metadata remains");
    assert_eq!(torrent.engine_id, None);
    assert_eq!(
        torrent.info_hash.as_deref(),
        Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8")
    );
    assert_eq!(torrent.uploaded_bytes, 5 * 1024);
    assert_eq!(torrent.fetched_bytes, 2 * 1024);
    assert_eq!(torrent.ratio, 2.5);
    assert_eq!(torrent.seeding_started_at, Some(123_456));
    assert_eq!(torrent.last_runtime_uploaded_bytes, None);
    assert_eq!(torrent.last_runtime_fetched_bytes, None);
    assert_eq!(torrent.diagnostics, None);

    let runtime = state.inner.read().await;
    assert!(runtime.diagnostic_events.iter().any(|event| {
        event.level == DiagnosticLevel::Warning
            && event.message.contains("Rechecking seeding restore")
    }));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn seed_policy_defaults_to_forever_and_supports_limits() {
    let mut settings = Settings::default();
    assert_eq!(
        settings.torrent.peer_connection_watchdog_mode,
        TorrentPeerConnectionWatchdogMode::Assist,
        "peer connection watchdog should default to non-destructive safe assist"
    );
    assert!(!should_stop_seeding(&settings.torrent, 9.0, 24 * 60 * 60));

    settings.torrent.seed_mode = TorrentSeedMode::Ratio;
    settings.torrent.seed_ratio_limit = 1.5;
    assert!(!should_stop_seeding(&settings.torrent, 1.49, 60));
    assert!(should_stop_seeding(&settings.torrent, 1.5, 60));

    settings.torrent.seed_mode = TorrentSeedMode::Time;
    settings.torrent.seed_time_limit_minutes = 30;
    assert!(!should_stop_seeding(&settings.torrent, 0.1, 29 * 60));
    assert!(should_stop_seeding(&settings.torrent, 0.1, 30 * 60));

    settings.torrent.seed_mode = TorrentSeedMode::RatioOrTime;
    settings.torrent.seed_ratio_limit = 2.0;
    settings.torrent.seed_time_limit_minutes = 120;
    assert!(should_stop_seeding(&settings.torrent, 2.0, 10));
    assert!(should_stop_seeding(&settings.torrent, 0.5, 120 * 60));
}

#[tokio::test]
async fn torrent_retry_attempt_diagnostic_uses_torrent_specific_wording() {
    let download_dir = test_runtime_dir("torrent-retry-diagnostic-wording");
    let mut job = download_job(
        "job_torrent_retry",
        JobState::Failed,
        ResumeSupport::Unknown,
        0,
    );
    job.transfer_kind = TransferKind::Torrent;
    job.filename = "Example Torrent".into();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    state
        .record_retry_attempt("job_torrent_retry", 1)
        .await
        .expect("retry attempt should record");

    let runtime = state.inner.read().await;
    assert!(runtime.diagnostic_events.iter().any(|event| {
        event.level == DiagnosticLevel::Warning
            && event.category == "download"
            && event.job_id.as_deref() == Some("job_torrent_retry")
            && event.message == "Torrent retry attempt 1 for Example Torrent"
    }));

    let _ = std::fs::remove_dir_all(download_dir);
}
