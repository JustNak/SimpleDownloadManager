use super::*;

#[test]
fn restart_reset_clears_partial_progress_and_failure_metadata() {
    let mut job = DownloadJob {
        id: "job_1".into(),
        url: "https://example.com/file.zip".into(),
        filename: "file.zip".into(),
        source: None,
        transfer_kind: TransferKind::Http,
        integrity_check: None,
        torrent: None,
        state: JobState::Failed,
        removal_state: None,
        created_at: 1,
        progress: 42.0,
        total_bytes: 100,
        downloaded_bytes: 42,
        speed: 2048,
        eta: 12,
        active_segments: None,
        planned_segments: None,
        error: Some("server closed the connection".into()),
        failure_category: Some(FailureCategory::Network),
        resume_support: ResumeSupport::Supported,
        retry_attempts: 2,
        auto_restart_attempts: 0,
        resolved_from_url: None,
        hoster_preflight: None,
        target_path: "C:/Downloads/file.zip".into(),
        temp_path: "C:/Downloads/file.zip.part".into(),
        artifact_exists: None,
        bulk_archive: None,
    };

    reset_job_for_restart(&mut job);

    assert_eq!(job.state, JobState::Queued);
    assert_eq!(job.progress, 0.0);
    assert_eq!(job.total_bytes, 0);
    assert_eq!(job.downloaded_bytes, 0);
    assert_eq!(job.speed, 0);
    assert_eq!(job.eta, 0);
    assert_eq!(job.error, None);
    assert_eq!(job.failure_category, None);
    assert_eq!(job.resume_support, ResumeSupport::Unknown);
    assert_eq!(job.retry_attempts, 0);
    assert_eq!(job.auto_restart_attempts, 0);
}

#[tokio::test]
async fn restart_job_removes_partial_file_and_segment_metadata() {
    let download_dir = test_runtime_dir("restart-removes-segment-metadata");
    let target_path = download_dir.join("file.zip");
    let temp_path = download_dir.join("file.zip.part");
    let meta_path = PathBuf::from(format!("{}.meta", temp_path.display()));
    std::fs::write(&temp_path, b"partial").unwrap();
    std::fs::write(&meta_path, b"{\"segments\":[]}").unwrap();

    let mut job = download_job(
        "job_restart_meta",
        JobState::Failed,
        ResumeSupport::Supported,
        50,
    );
    job.target_path = target_path.display().to_string();
    job.temp_path = temp_path.display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    state
        .restart_job("job_restart_meta")
        .await
        .expect("restart should queue the job from zero");

    assert!(!temp_path.exists());
    assert!(!meta_path.exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn destructive_cancel_delete_removes_segment_sidecars() {
    let download_dir = test_runtime_dir("cancel-delete-removes-segment-sidecars");
    let target_path = download_dir.join("segmented.zip");
    let temp_path = download_dir.join("segmented.zip.part");
    let sidecars = write_partial_sidecars(&temp_path);

    let mut job = download_job(
        "job_segment_cancel",
        JobState::Failed,
        ResumeSupport::Supported,
        50,
    );
    job.target_path = target_path.display().to_string();
    job.temp_path = temp_path.display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    let ids = vec!["job_segment_cancel".to_string()];

    let prepared = state
        .cancel_jobs_for_delete(&ids)
        .await
        .expect("cancel should prepare destructive cleanup");
    let snapshot = state
        .run_destructive_cleanup(prepared.jobs)
        .await
        .expect("destructive cleanup should finish");

    assert!(snapshot.jobs.is_empty());
    assert_partial_artifacts_removed(&temp_path, &sidecars);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn delete_job_from_disk_removes_segment_sidecars() {
    let download_dir = test_runtime_dir("delete-job-removes-segment-sidecars");
    let target_path = download_dir.join("segmented.zip");
    let temp_path = download_dir.join("segmented.zip.part");
    let sidecars = write_partial_sidecars(&temp_path);

    let mut job = download_job(
        "job_segment_direct",
        JobState::Failed,
        ResumeSupport::Supported,
        50,
    );
    job.target_path = target_path.display().to_string();
    job.temp_path = temp_path.display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let snapshot = state
        .delete_job("job_segment_direct", true)
        .await
        .expect("delete from disk should remove partial artifacts");

    assert!(snapshot.jobs.is_empty());
    assert_partial_artifacts_removed(&temp_path, &sidecars);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn restart_reset_clears_torrent_runtime_metadata_without_changing_paths() {
    let mut job = DownloadJob {
        id: "job_1".into(),
        url: "magnet:?xt=urn:btih:420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        filename: "torrent".into(),
        source: None,
        transfer_kind: TransferKind::Torrent,
        integrity_check: None,
        torrent: Some(TorrentInfo {
            engine_id: Some(12),
            info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
            name: Some("Example Torrent".into()),
            total_files: Some(3),
            peers: Some(9),
            seeds: Some(2),
            uploaded_bytes: 4096,
            last_runtime_uploaded_bytes: Some(1024),
            fetched_bytes: 8192,
            last_runtime_fetched_bytes: Some(2048),
            ratio: 2.0,
            seeding_started_at: Some(123456),
            diagnostics: None,
        }),
        state: JobState::Paused,
        removal_state: None,
        created_at: 1,
        progress: 100.0,
        total_bytes: 4096,
        downloaded_bytes: 4096,
        speed: 2048,
        eta: 0,
        active_segments: None,
        planned_segments: None,
        error: Some("previous torrent error".into()),
        failure_category: Some(FailureCategory::Torrent),
        resume_support: ResumeSupport::Unsupported,
        retry_attempts: 2,
        auto_restart_attempts: 0,
        resolved_from_url: None,
        hoster_preflight: None,
        target_path: "C:/Downloads/example-torrent".into(),
        temp_path: "C:/Downloads/.torrent-state/job_1".into(),
        artifact_exists: None,
        bulk_archive: None,
    };
    let target_path = job.target_path.clone();
    let temp_path = job.temp_path.clone();

    reset_job_for_restart(&mut job);

    assert_eq!(job.transfer_kind, TransferKind::Torrent);
    assert_eq!(job.torrent, Some(TorrentInfo::default()));
    assert_eq!(job.target_path, target_path);
    assert_eq!(job.temp_path, temp_path);
}

#[test]
fn bulk_archive_status_updates_all_archive_members() {
    let archive = BulkArchiveInfo {
        id: "bulk_1".into(),
        name: "bundle.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Pending,
        requires_extraction: None,
        output_path: None,
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let mut first = download_job("job_1", JobState::Completed, ResumeSupport::Supported, 100);
    let mut second = download_job("job_2", JobState::Completed, ResumeSupport::Supported, 100);
    first.bulk_archive = Some(archive.clone());
    second.bulk_archive = Some(archive);
    let mut state = runtime_state_with_jobs(vec![
        first,
        second,
        download_job("job_3", JobState::Completed, ResumeSupport::Supported, 100),
    ]);

    state.mark_bulk_archive_status_in_memory(
        "bulk_1",
        BulkArchiveStatus::Compressing,
        Some(true),
        Some("C:/Downloads/bundle.zip".into()),
        None,
        None,
        Some(BulkFinalizeMode::Zip),
        Some(1024),
        Some(0),
    );

    let mut archive_members = state
        .jobs
        .iter()
        .filter_map(|job| job.bulk_archive.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(archive_members.len(), 2);
    assert!(archive_members
        .iter()
        .all(|archive| archive.archive_status == BulkArchiveStatus::Compressing));
    assert!(archive_members
        .iter()
        .all(|archive| archive.output_path.as_deref() == Some("C:/Downloads/bundle.zip")));
    assert!(archive_members
        .iter()
        .all(|archive| archive.requires_extraction == Some(true)));

    state.mark_bulk_archive_status_in_memory(
        "bulk_1",
        BulkArchiveStatus::Failed,
        None,
        Some("C:/Downloads/bundle.zip".into()),
        Some("zip failed".into()),
        None,
        None,
        None,
        None,
    );
    archive_members = state
        .jobs
        .iter()
        .filter_map(|job| job.bulk_archive.as_ref())
        .collect::<Vec<_>>();
    assert!(archive_members
        .iter()
        .all(|archive| archive.archive_status == BulkArchiveStatus::Failed));
    assert!(archive_members
        .iter()
        .all(|archive| archive.error.as_deref() == Some("zip failed")));

    state.mark_bulk_archive_status_in_memory(
        "bulk_1",
        BulkArchiveStatus::Completed,
        None,
        Some("C:/Downloads/bundle.zip".into()),
        None,
        Some("cleanup warning".into()),
        Some(BulkFinalizeMode::Zip),
        Some(1024),
        Some(1024),
    );
    archive_members = state
        .jobs
        .iter()
        .filter_map(|job| job.bulk_archive.as_ref())
        .collect::<Vec<_>>();
    assert!(archive_members
        .iter()
        .all(|archive| archive.archive_status == BulkArchiveStatus::Completed));
    assert!(archive_members
        .iter()
        .all(|archive| archive.error.is_none()));
    assert!(archive_members
        .iter()
        .all(|archive| archive.warning.as_deref() == Some("cleanup warning")));
}

#[tokio::test]
async fn remove_active_job_rejects_without_freeing_worker_slot() {
    let download_dir = test_runtime_dir("remove-active-job");
    let mut active_job = download_job("job_1", JobState::Downloading, ResumeSupport::Supported, 10);
    active_job.target_path = download_dir.join("active.zip").display().to_string();
    active_job.temp_path = download_dir.join("active.zip.part").display().to_string();
    let mut queued_job = download_job("job_2", JobState::Queued, ResumeSupport::Unknown, 0);
    queued_job.target_path = download_dir.join("queued.zip").display().to_string();
    queued_job.temp_path = download_dir.join("queued.zip.part").display().to_string();
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active_job, queued_job],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.active_workers.insert("job_1".into());
    }

    let error = state.remove_job("job_1").await.unwrap_err();

    assert_eq!(error.code, "INTERNAL_ERROR");
    assert!(error.message.contains("Pause or cancel"));
    let runtime = state.inner.read().await;
    assert!(runtime.active_workers.contains("job_1"));
    assert_eq!(runtime.jobs.len(), 2);
    drop(runtime);

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should still work");
    assert!(tasks.is_empty());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn remove_canceled_torrent_job_clears_stale_worker_slot() {
    let download_dir = test_runtime_dir("remove-canceled-torrent");
    let mut canceled_job = download_job("job_1", JobState::Canceled, ResumeSupport::Unsupported, 0);
    canceled_job.transfer_kind = TransferKind::Torrent;
    canceled_job.torrent = Some(TorrentInfo::default());
    canceled_job.target_path = download_dir.join("torrent-a634dc94").display().to_string();
    canceled_job.temp_path = download_dir
        .join(".torrent-state")
        .join("job_1")
        .display()
        .to_string();
    std::fs::create_dir_all(&canceled_job.temp_path).unwrap();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![canceled_job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_1".into());
    }

    let snapshot = state
        .remove_job("job_1")
        .await
        .expect("canceled torrent should be removable even while worker cleanup is pending");

    assert!(snapshot.jobs.is_empty());
    let runtime = state.inner.read().await;
    assert!(!runtime.active_workers.contains("job_1"));
    drop(runtime);
    assert!(!download_dir.join(".torrent-state").join("job_1").exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn delete_canceled_torrent_job_with_files_clears_stale_worker_slot() {
    let download_dir = test_runtime_dir("delete-canceled-torrent");
    let target_path = download_dir.join("torrent-a634dc94");
    let temp_path = download_dir.join(".torrent-state").join("job_1");
    std::fs::create_dir_all(&target_path).unwrap();
    std::fs::create_dir_all(&temp_path).unwrap();
    let mut canceled_job = download_job("job_1", JobState::Canceled, ResumeSupport::Unsupported, 0);
    canceled_job.transfer_kind = TransferKind::Torrent;
    canceled_job.torrent = Some(TorrentInfo::default());
    canceled_job.target_path = target_path.display().to_string();
    canceled_job.temp_path = temp_path.display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![canceled_job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_1".into());
    }

    let snapshot = state
        .delete_job("job_1", true)
        .await
        .expect("delete from disk should work for canceled torrents with stale workers");

    assert!(snapshot.jobs.is_empty());
    let runtime = state.inner.read().await;
    assert!(!runtime.active_workers.contains("job_1"));
    drop(runtime);
    assert!(!target_path.exists());
    assert!(!temp_path.exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn delete_completed_bulk_member_from_disk_removes_archive_output() {
    let download_dir = test_runtime_dir("delete-bulk-archive-output");
    let archive_path = download_dir.join("bulk-download.zip");
    let part_path = download_dir.join("Game.part01.rar");
    std::fs::write(&archive_path, b"archive").unwrap();
    std::fs::write(&part_path, b"part").unwrap();
    let mut job = download_job("job_1", JobState::Completed, ResumeSupport::Supported, 100);
    job.target_path = part_path.display().to_string();
    job.temp_path = part_path.with_extension("rar.part").display().to_string();
    job.bulk_archive = Some(BulkArchiveInfo {
        id: "bulk_1".into(),
        name: "bulk-download.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Completed,
        requires_extraction: None,
        output_path: Some(archive_path.display().to_string()),
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let snapshot = state
        .delete_job("job_1", true)
        .await
        .expect("bulk member disk deletion should remove the completed archive output");

    assert!(snapshot.jobs.is_empty());
    assert!(!archive_path.exists());
    assert!(!part_path.exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_member_auto_restart_candidate_accepts_transient_pending_http_members() {
    let download_dir = test_runtime_dir("bulk-auto-restart-candidate");
    let mut job = download_job(
        "job_auto",
        JobState::Downloading,
        ResumeSupport::Supported,
        42,
    );
    job.resolved_from_url = Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar".into());
    job.auto_restart_attempts = 0;
    job.bulk_archive = Some(BulkArchiveInfo {
        id: "bulk_auto".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Pending,
        requires_extraction: None,
        output_path: None,
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            auto_retry_attempts: 2,
            bulk: BulkDownloadSettings {
                auto_retry_override_enabled: true,
                auto_retry_attempts: 5,
                ..BulkDownloadSettings::default()
            },
            ..Settings::default()
        })
        .await
        .unwrap();

    for (category, message, retryable, expected_mode) in [
        (
            FailureCategory::Network,
            "Download failed: operation timed out",
            true,
            Some(BulkMemberAutoRestartMode::PreservePartial),
        ),
        (
            FailureCategory::Server,
            "Download request failed with HTTP 503 Service Unavailable.",
            true,
            Some(BulkMemberAutoRestartMode::PreservePartial),
        ),
        (
            FailureCategory::Http,
            "Download request failed with HTTP 403 Forbidden.",
            false,
            Some(BulkMemberAutoRestartMode::PreservePartial),
        ),
        (
            FailureCategory::Resume,
            "The remote server rejected the resume request.",
            false,
            None,
        ),
    ] {
        let candidate = state
            .bulk_member_auto_restart_candidate("job_auto", category, message, retryable)
            .await
            .expect("candidate lookup should succeed");
        assert_eq!(
            candidate.as_ref().map(|candidate| candidate.mode),
            expected_mode
        );
        if let Some(candidate) = candidate {
            assert_eq!(candidate.attempt, 1);
            assert_eq!(candidate.max_attempts, 5);
            assert_eq!(
                candidate.resolved_from_url.as_deref(),
                Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar")
            );
        }
    }

    assert!(state
        .bulk_member_auto_restart_candidate(
            "job_auto",
            FailureCategory::Disk,
            "Could not write download chunk",
            false,
        )
        .await
        .unwrap()
        .is_none());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_member_auto_restart_candidate_preserves_after_preserved_attempt() {
    let download_dir = test_runtime_dir("bulk-auto-restart-preserve-after-preserve");
    let archive = bulk_archive_info(&download_dir, "bulk_auto_reset_after_preserve");
    let mut job = download_job(
        "job_auto",
        JobState::Downloading,
        ResumeSupport::Supported,
        42,
    );
    job.bulk_archive = Some(archive);
    job.auto_restart_attempts = 1;
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            auto_retry_attempts: 3,
            ..Settings::default()
        })
        .await
        .unwrap();

    let candidate = state
        .bulk_member_auto_restart_candidate(
            "job_auto",
            FailureCategory::Network,
            "Download failed: connection closed",
            true,
        )
        .await
        .expect("candidate lookup should succeed")
        .expect("second recovery should be eligible");

    assert_eq!(candidate.attempt, 2);
    assert_eq!(candidate.max_attempts, 3);
    assert_eq!(candidate.mode, BulkMemberAutoRestartMode::PreservePartial);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_member_auto_restart_candidate_rejects_direct_nonretryable_http_failures() {
    let download_dir = test_runtime_dir("bulk-auto-restart-direct-http-rejected");
    let archive = bulk_archive_info(&download_dir, "bulk_auto_direct_http");

    let mut direct = download_job(
        "job_direct",
        JobState::Downloading,
        ResumeSupport::Supported,
        42,
    );
    direct.url = "https://example.com/missing.part01.rar".into();
    direct.bulk_archive = Some(archive.clone());

    let mut hoster = download_job(
        "job_hoster",
        JobState::Downloading,
        ResumeSupport::Supported,
        42,
    );
    hoster.url = "https://dl.fuckingfast.co/dl/expired-token".into();
    hoster.resolved_from_url = Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar".into());
    hoster.bulk_archive = Some(archive);

    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![direct, hoster]);
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            auto_retry_attempts: 2,
            ..Settings::default()
        })
        .await
        .unwrap();

    assert!(state
        .bulk_member_auto_restart_candidate(
            "job_direct",
            FailureCategory::Http,
            "Download request failed with HTTP 404 Not Found.",
            false,
        )
        .await
        .unwrap()
        .is_none());

    let candidate = state
        .bulk_member_auto_restart_candidate(
            "job_hoster",
            FailureCategory::Http,
            "Download request failed with HTTP 404 Not Found.",
            false,
        )
        .await
        .unwrap()
        .expect("expired hoster token should be recoverable");
    assert_eq!(candidate.mode, BulkMemberAutoRestartMode::PreservePartial);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_member_auto_restart_candidate_rejects_exhausted_non_bulk_and_failed_archive_jobs() {
    let download_dir = test_runtime_dir("bulk-auto-restart-rejected");
    let archive = |status: BulkArchiveStatus| BulkArchiveInfo {
        id: format!("bulk_{status:?}"),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: status,
        requires_extraction: None,
        output_path: None,
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let mut exhausted = download_job(
        "job_exhausted",
        JobState::Downloading,
        ResumeSupport::Supported,
        42,
    );
    exhausted.bulk_archive = Some(archive(BulkArchiveStatus::Pending));
    exhausted.auto_restart_attempts = 2;

    let non_bulk = download_job(
        "job_plain",
        JobState::Downloading,
        ResumeSupport::Supported,
        42,
    );

    let mut failed_archive = download_job(
        "job_failed_archive",
        JobState::Downloading,
        ResumeSupport::Supported,
        42,
    );
    failed_archive.bulk_archive = Some(archive(BulkArchiveStatus::Failed));

    let mut torrent_bulk = download_job(
        "job_torrent",
        JobState::Downloading,
        ResumeSupport::Supported,
        42,
    );
    torrent_bulk.transfer_kind = TransferKind::Torrent;
    torrent_bulk.torrent = Some(TorrentInfo::default());
    torrent_bulk.bulk_archive = Some(archive(BulkArchiveStatus::Pending));

    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![exhausted, non_bulk, failed_archive, torrent_bulk],
    );
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            auto_retry_attempts: 2,
            ..Settings::default()
        })
        .await
        .unwrap();

    for id in [
        "job_exhausted",
        "job_plain",
        "job_failed_archive",
        "job_torrent",
    ] {
        assert!(
            state
                .bulk_member_auto_restart_candidate(
                    id,
                    FailureCategory::Network,
                    "Download failed: connection closed",
                    true,
                )
                .await
                .unwrap()
                .is_none(),
            "{id} should not be eligible for bulk auto-restart"
        );
    }

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_member_slow_recovery_state_accepts_pending_http_bulk_members() {
    let download_dir = test_runtime_dir("bulk-slow-recovery-state");
    let mut job = download_job(
        "job_bulk_slow",
        JobState::Downloading,
        ResumeSupport::Supported,
        1024,
    );
    job.retry_attempts = 1;
    job.bulk_archive = Some(BulkArchiveInfo {
        id: "bulk_slow".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Pending,
        requires_extraction: None,
        output_path: None,
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            auto_retry_attempts: 4,
            ..Settings::default()
        })
        .await
        .unwrap();

    assert_eq!(
        state
            .bulk_member_slow_recovery_state("job_bulk_slow")
            .await
            .unwrap(),
        Some(BulkMemberSlowRecoveryState {
            retry_attempts: 1,
            max_retry_attempts: 4,
        })
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_member_slow_recovery_state_rejects_non_pending_http_bulk_members() {
    let download_dir = test_runtime_dir("bulk-slow-recovery-state-rejected");
    let archive = |id: &str, status: BulkArchiveStatus| BulkArchiveInfo {
        id: id.into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: status,
        requires_extraction: None,
        output_path: None,
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let non_bulk = download_job(
        "job_plain_slow",
        JobState::Downloading,
        ResumeSupport::Supported,
        1024,
    );
    let mut completed_archive = download_job(
        "job_completed_bulk_slow",
        JobState::Downloading,
        ResumeSupport::Supported,
        1024,
    );
    completed_archive.bulk_archive =
        Some(archive("bulk_completed_slow", BulkArchiveStatus::Completed));
    let mut torrent_bulk = download_job(
        "job_torrent_bulk_slow",
        JobState::Downloading,
        ResumeSupport::Supported,
        1024,
    );
    torrent_bulk.transfer_kind = TransferKind::Torrent;
    torrent_bulk.torrent = Some(TorrentInfo::default());
    torrent_bulk.bulk_archive = Some(archive("bulk_torrent_slow", BulkArchiveStatus::Pending));

    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![non_bulk, completed_archive, torrent_bulk],
    );
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            auto_retry_attempts: 4,
            ..Settings::default()
        })
        .await
        .unwrap();

    for id in [
        "job_plain_slow",
        "job_completed_bulk_slow",
        "job_torrent_bulk_slow",
        "job_missing_slow",
    ] {
        assert!(
            state
                .bulk_member_slow_recovery_state(id)
                .await
                .unwrap()
                .is_none(),
            "{id} should not be eligible for bulk slow recovery"
        );
    }

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn auto_restart_bulk_member_resets_partial_state_and_preserves_bulk_identity() {
    let download_dir = test_runtime_dir("bulk-auto-restart-reset");
    let target_path = download_dir.join("Game.part01.rar");
    let temp_path = download_dir.join("Game.part01.rar.part");
    std::fs::write(&temp_path, b"partial").unwrap();

    let bulk_archive = BulkArchiveInfo {
        id: "bulk_auto_reset".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Pending,
        requires_extraction: None,
        output_path: None,
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let mut job = download_job(
        "job_auto_reset",
        JobState::Downloading,
        ResumeSupport::Supported,
        42,
    );
    job.url = "https://dl.fuckingfast.co/dl/old-token".into();
    job.filename = "Game.part01.rar".into();
    job.target_path = target_path.display().to_string();
    job.temp_path = temp_path.display().to_string();
    job.error = Some("HTTP 403".into());
    job.failure_category = Some(FailureCategory::Http);
    job.retry_attempts = 3;
    job.auto_restart_attempts = 1;
    job.resolved_from_url = Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar".into());
    job.bulk_archive = Some(bulk_archive.clone());
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_auto_reset".into());
    }

    let snapshot = state
        .auto_restart_bulk_member(
            "job_auto_reset",
            "https://dl.fuckingfast.co/dl/new-token".into(),
            BulkMemberAutoRestartMode::ResetPartial,
            2,
            5,
            FailureCategory::Resume,
            "The remote server rejected the resume request.",
        )
        .await
        .expect("auto-restart should reset and queue the member");

    let restarted = snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_auto_reset")
        .expect("job should remain in queue");
    assert_eq!(restarted.state, JobState::Queued);
    assert_eq!(restarted.url, "https://dl.fuckingfast.co/dl/new-token");
    assert_eq!(restarted.filename, "Game.part01.rar");
    assert_eq!(restarted.target_path, target_path.display().to_string());
    assert_eq!(restarted.progress, 0.0);
    assert_eq!(restarted.total_bytes, 0);
    assert_eq!(restarted.downloaded_bytes, 0);
    assert_eq!(restarted.resume_support, ResumeSupport::Unknown);
    assert_eq!(restarted.retry_attempts, 0);
    assert_eq!(restarted.auto_restart_attempts, 2);
    assert_eq!(
        restarted.resolved_from_url.as_deref(),
        Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar")
    );
    assert_eq!(restarted.bulk_archive.as_ref(), Some(&bulk_archive));
    assert!(!temp_path.exists());
    assert!(!state
        .inner
        .read()
        .await
        .active_workers
        .contains("job_auto_reset"));
    let diagnostics = state
        .diagnostics_snapshot(HostRegistrationDiagnostics {
            status: HostRegistrationStatus::Configured,
            entries: Vec::new(),
        })
        .await;
    assert!(diagnostics.recent_events.iter().any(|event| {
        event.message.contains("reset partial")
            && event.message.contains("attempt 2/5")
            && event.message.contains("resume")
    }));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn auto_restart_bulk_member_preserves_partial_state_and_clears_hoster_health() {
    let download_dir = test_runtime_dir("bulk-auto-restart-preserve");
    let target_path = download_dir.join("Game.part01.rar");
    let temp_path = download_dir.join("Game.part01.rar.part");
    std::fs::write(&temp_path, b"partial").unwrap();

    let archive = bulk_archive_info(&download_dir, "bulk_auto_preserve");
    let mut job = download_job(
        "job_auto_preserve",
        JobState::Downloading,
        ResumeSupport::Supported,
        50,
    );
    job.url = "https://dl.fuckingfast.co/dl/old-token".into();
    job.filename = "Game.part01.rar".into();
    job.target_path = target_path.display().to_string();
    job.temp_path = temp_path.display().to_string();
    job.downloaded_bytes = 512;
    job.total_bytes = 1024;
    job.progress = 50.0;
    job.resolved_from_url = Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar".into());
    job.bulk_archive = Some(archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_auto_preserve".into());
        seed_healthy_bulk_hoster_health(&mut runtime, "job_auto_preserve", 96 * 1024);
    }

    let snapshot = state
        .auto_restart_bulk_member(
            "job_auto_preserve",
            "https://fuckingfast.co/ecw0lw398okf#Game.part01.rar".into(),
            BulkMemberAutoRestartMode::PreservePartial,
            1,
            4,
            FailureCategory::Network,
            "Download failed: connection closed",
        )
        .await
        .expect("preserve recovery should queue the member");

    let recovered = snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_auto_preserve")
        .expect("job should remain in queue");
    assert_eq!(recovered.state, JobState::Queued);
    assert_eq!(
        recovered.url,
        "https://fuckingfast.co/ecw0lw398okf#Game.part01.rar"
    );
    assert_eq!(recovered.downloaded_bytes, 512);
    assert_eq!(recovered.total_bytes, 1024);
    assert_eq!(recovered.progress, 50.0);
    assert_eq!(recovered.resume_support, ResumeSupport::Supported);
    assert_eq!(recovered.retry_attempts, 0);
    assert_eq!(recovered.auto_restart_attempts, 1);
    assert_eq!(recovered.error, None);
    assert_eq!(recovered.failure_category, None);
    assert!(temp_path.exists());
    let runtime = state.inner.read().await;
    assert!(!runtime.active_workers.contains("job_auto_preserve"));
    assert!(!runtime
        .bulk_hoster_worker_health
        .contains_key("job_auto_preserve"));
    drop(runtime);
    let diagnostics = state
        .diagnostics_snapshot(HostRegistrationDiagnostics {
            status: HostRegistrationStatus::Configured,
            entries: Vec::new(),
        })
        .await;
    assert!(diagnostics.recent_events.iter().any(|event| {
        event.message.contains("preserve partial")
            && event.message.contains("attempt 1/4")
            && event.message.contains("network")
    }));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn repeated_network_bulk_auto_restart_preserves_partial_state() {
    let download_dir = test_runtime_dir("bulk-auto-restart-mode-preserve-repeat");
    let archive = bulk_archive_info(&download_dir, "bulk_auto_preserve_repeat");
    let mut job = download_job(
        "job_auto_preserve_repeat",
        JobState::Downloading,
        ResumeSupport::Supported,
        50,
    );
    job.bulk_archive = Some(archive);
    job.downloaded_bytes = 128 * 1024 * 1024;
    job.total_bytes = 500 * 1024 * 1024;
    job.auto_restart_attempts = 2;

    assert_eq!(
        bulk_member_auto_restart_mode(
            &job,
            FailureCategory::Network,
            "Download failed: error decoding response body",
            true,
        ),
        Some(BulkMemberAutoRestartMode::PreservePartial)
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_member_retry_candidates_accept_failed_pending_http_members() {
    let download_dir = test_runtime_dir("bulk-member-retry-candidates");
    let bulk_archive = BulkArchiveInfo {
        id: "bulk_member_retry".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Pending,
        requires_extraction: None,
        output_path: None,
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };

    let mut hoster_member = download_job(
        "job_hoster_failed",
        JobState::Failed,
        ResumeSupport::Supported,
        42,
    );
    hoster_member.url = "https://dl.fuckingfast.co/dl/old-token".into();
    hoster_member.resolved_from_url =
        Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar".into());
    hoster_member.bulk_archive = Some(bulk_archive.clone());

    let mut direct_member = download_job(
        "job_direct_failed",
        JobState::Failed,
        ResumeSupport::Supported,
        25,
    );
    direct_member.url = "https://example.com/Game.part02.rar".into();
    direct_member.bulk_archive = Some(bulk_archive.clone());

    let mut completed_member = download_job(
        "job_completed",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    completed_member.bulk_archive = Some(bulk_archive);

    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![hoster_member, direct_member, completed_member],
    );

    let candidates = state
        .bulk_member_retry_candidates("bulk_member_retry")
        .await
        .expect("failed pending HTTP bulk members should be candidates");

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].id, "job_hoster_failed");
    assert_eq!(
        candidates[0].source_url,
        "https://fuckingfast.co/ecw0lw398okf#Game.part01.rar"
    );
    assert_eq!(
        candidates[0].resolved_from_url.as_deref(),
        Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar")
    );
    assert_eq!(candidates[1].id, "job_direct_failed");
    assert_eq!(
        candidates[1].source_url,
        "https://example.com/Game.part02.rar"
    );
    assert_eq!(candidates[1].resolved_from_url, None);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_member_retry_candidates_reject_nonfailed_nonhttp_and_nonpending_archives() {
    let download_dir = test_runtime_dir("bulk-member-retry-rejected");
    let archive = |id: &str, status: BulkArchiveStatus| BulkArchiveInfo {
        id: id.into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: status,
        requires_extraction: None,
        output_path: None,
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };

    let mut active_member = download_job(
        "job_active",
        JobState::Downloading,
        ResumeSupport::Supported,
        10,
    );
    active_member.bulk_archive = Some(archive("bulk_rejected", BulkArchiveStatus::Pending));

    let mut completed_member = download_job(
        "job_completed",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    completed_member.bulk_archive = Some(archive("bulk_rejected", BulkArchiveStatus::Pending));

    let mut canceled_member = download_job(
        "job_canceled",
        JobState::Canceled,
        ResumeSupport::Supported,
        10,
    );
    canceled_member.bulk_archive = Some(archive("bulk_rejected", BulkArchiveStatus::Pending));

    let mut torrent_failed = download_job(
        "job_torrent_failed",
        JobState::Failed,
        ResumeSupport::Supported,
        10,
    );
    torrent_failed.transfer_kind = TransferKind::Torrent;
    torrent_failed.torrent = Some(TorrentInfo::default());
    torrent_failed.bulk_archive = Some(archive("bulk_rejected", BulkArchiveStatus::Pending));

    let non_bulk_failed = download_job(
        "job_non_bulk_failed",
        JobState::Failed,
        ResumeSupport::Supported,
        10,
    );

    let mut failed_archive_member = download_job(
        "job_failed_archive_member",
        JobState::Failed,
        ResumeSupport::Supported,
        10,
    );
    failed_archive_member.bulk_archive =
        Some(archive("bulk_archive_failed", BulkArchiveStatus::Failed));

    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![
            active_member,
            completed_member,
            canceled_member,
            torrent_failed,
            non_bulk_failed,
            failed_archive_member,
        ],
    );

    assert!(state
        .bulk_member_retry_candidates("bulk_rejected")
        .await
        .unwrap_err()
        .contains("No failed bulk member downloads"));
    assert!(state
        .bulk_member_retry_candidates("bulk_archive_failed")
        .await
        .unwrap_err()
        .contains("pending"));
    assert!(state
        .bulk_member_retry_candidates("bulk_missing")
        .await
        .unwrap_err()
        .contains("not found"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn retry_bulk_member_preserves_partial_state_and_bulk_identity() {
    let download_dir = test_runtime_dir("bulk-member-manual-retry-preserve");
    let target_path = download_dir.join("Game.part01.rar");
    let temp_path = download_dir.join("Game.part01.rar.part");
    let meta_path = PathBuf::from(format!("{}.meta", temp_path.display()));
    std::fs::write(&temp_path, b"partial").unwrap();
    std::fs::write(&meta_path, b"{\"segments\":[]}").unwrap();

    let bulk_archive = BulkArchiveInfo {
        id: "bulk_manual_retry".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Pending,
        requires_extraction: None,
        output_path: None,
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let mut job = download_job(
        "job_manual_retry",
        JobState::Failed,
        ResumeSupport::Supported,
        42,
    );
    job.url = "https://dl.fuckingfast.co/dl/old-token".into();
    job.filename = "Game.part01.rar".into();
    job.target_path = target_path.display().to_string();
    job.temp_path = temp_path.display().to_string();
    job.error = Some("HTTP 403".into());
    job.failure_category = Some(FailureCategory::Http);
    job.retry_attempts = 3;
    job.auto_restart_attempts = 4;
    job.downloaded_bytes = 512;
    job.total_bytes = 1024;
    job.progress = 50.0;
    job.resolved_from_url = Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar".into());
    job.bulk_archive = Some(bulk_archive.clone());

    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    let snapshot = state
        .retry_bulk_member(
            "job_manual_retry",
            "https://dl.fuckingfast.co/dl/new-token".into(),
        )
        .await
        .expect("manual bulk member retry should preserve partial state and queue the member");

    let retried = snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_manual_retry")
        .expect("job should remain in queue");
    assert_eq!(retried.state, JobState::Queued);
    assert_eq!(retried.url, "https://dl.fuckingfast.co/dl/new-token");
    assert_eq!(retried.filename, "Game.part01.rar");
    assert_eq!(retried.target_path, target_path.display().to_string());
    assert_eq!(retried.progress, 50.0);
    assert_eq!(retried.total_bytes, 1024);
    assert_eq!(retried.downloaded_bytes, 512);
    assert_eq!(retried.error, None);
    assert_eq!(retried.failure_category, None);
    assert_eq!(retried.resume_support, ResumeSupport::Supported);
    assert_eq!(retried.retry_attempts, 0);
    assert_eq!(retried.auto_restart_attempts, 4);
    assert_eq!(
        retried.resolved_from_url.as_deref(),
        Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar")
    );
    assert_eq!(retried.bulk_archive.as_ref(), Some(&bulk_archive));
    assert!(temp_path.exists());
    assert!(meta_path.exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn recoverable_partial_detection_uses_existing_partial_file_when_counters_are_zero() {
    let download_dir = test_runtime_dir("recoverable-partial-file-detected");
    let temp_path = download_dir.join("big-bulk-member.bin.part");
    std::fs::write(&temp_path, b"partial bytes from an interrupted transfer").unwrap();

    let mut job = download_job(
        "job_partial_file_only",
        JobState::Downloading,
        ResumeSupport::Supported,
        0,
    );
    job.temp_path = temp_path.display().to_string();
    job.total_bytes = 1024;
    job.bulk_archive = Some(bulk_archive_info(&download_dir, "bulk_partial_file_only"));

    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    assert!(
        state
            .has_recoverable_partial_download("job_partial_file_only")
            .await,
        "an existing non-empty partial file should be recoverable even before progress counters were flushed"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn delete_failed_bulk_members_from_disk_removes_downloaded_parts() {
    let download_dir = test_runtime_dir("delete-failed-bulk-parts");
    let part_1 = download_dir.join("Game.part01.rar");
    let part_2 = download_dir.join("Game.part02.rar");
    std::fs::write(&part_1, b"first").unwrap();
    std::fs::write(&part_2, b"second").unwrap();

    let bulk_archive = BulkArchiveInfo {
        id: "bulk_failed_delete".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Failed,
        requires_extraction: None,
        output_path: Some(download_dir.join("Game.zip").display().to_string()),
        error: Some("locked".into()),
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let mut job_1 = download_job("job_1", JobState::Completed, ResumeSupport::Supported, 100);
    job_1.target_path = part_1.display().to_string();
    job_1.temp_path = part_1.with_extension("rar.part").display().to_string();
    job_1.bulk_archive = Some(bulk_archive.clone());
    let mut job_2 = download_job("job_2", JobState::Completed, ResumeSupport::Supported, 100);
    job_2.target_path = part_2.display().to_string();
    job_2.temp_path = part_2.with_extension("rar.part").display().to_string();
    job_2.bulk_archive = Some(bulk_archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job_1, job_2]);

    state
        .delete_job("job_1", true)
        .await
        .expect("first failed bulk member should delete from disk");
    let snapshot = state
        .delete_job("job_2", true)
        .await
        .expect("second failed bulk member should delete from disk");

    assert!(snapshot.jobs.is_empty());
    assert!(!part_1.exists());
    assert!(!part_2.exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn delete_canceled_active_job_waits_for_worker_release_before_disk_cleanup() {
    let download_dir = test_runtime_dir("delete-canceled-active-waits");
    let target_path = download_dir.join("active-file.zip");
    let temp_path = download_dir.join("active-file.zip.part");
    std::fs::write(&target_path, b"target").unwrap();
    std::fs::write(&temp_path, b"partial").unwrap();

    let mut canceled_job = download_job("job_1", JobState::Canceled, ResumeSupport::Supported, 25);
    canceled_job.target_path = target_path.display().to_string();
    canceled_job.temp_path = temp_path.display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![canceled_job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_1".into());
    }

    let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release_state = state.clone();
    let release_flag = released.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        release_state
            .inner
            .write()
            .await
            .active_workers
            .remove("job_1");
        release_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    let snapshot = state
        .delete_job("job_1", true)
        .await
        .expect("canceled active deletion should wait for the worker to release");

    assert!(
        released.load(std::sync::atomic::Ordering::SeqCst),
        "delete should wait for the active worker release before removing files"
    );
    assert!(snapshot.jobs.is_empty());
    assert!(!target_path.exists());
    assert!(!temp_path.exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn cancel_jobs_marks_many_jobs_without_waiting_for_active_worker_release() {
    let download_dir = test_runtime_dir("cancel-jobs-batch-active");
    let active_temp = download_dir.join("active.zip.part");
    let queued_temp = download_dir.join("queued.zip.part");
    std::fs::write(&active_temp, b"active partial").unwrap();
    std::fs::write(&queued_temp, b"queued partial").unwrap();

    let archive = bulk_archive_info(&download_dir, "bulk_cancel_batch");
    let mut active = protected_hoster_bulk_job("job_active", archive);
    active.state = JobState::Downloading;
    active.downloaded_bytes = 64;
    active.progress = 64.0;
    active.temp_path = active_temp.display().to_string();
    let mut queued = download_job("job_queued", JobState::Queued, ResumeSupport::Unknown, 0);
    queued.temp_path = queued_temp.display().to_string();
    let completed = download_job(
        "job_completed",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active, queued, completed],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_active".into());
        seed_healthy_bulk_hoster_health(&mut runtime, "job_active", 96 * 1024);
    }
    state
        .handoff_auth
        .write()
        .await
        .insert("job_active".into(), HandoffAuth { headers: vec![] });

    let ids = vec!["job_active".to_string(), "job_queued".to_string()];
    let snapshot = tokio::time::timeout(Duration::from_millis(100), state.cancel_jobs(&ids))
        .await
        .expect("batch cancel should not wait for active worker file handles")
        .expect("batch cancel should succeed");

    let active_snapshot = snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_active")
        .expect("active job should remain visible");
    assert_eq!(active_snapshot.state, JobState::Canceled);
    assert_eq!(active_snapshot.downloaded_bytes, 0);
    assert_eq!(active_snapshot.progress, 0.0);
    assert!(
        active_temp.exists(),
        "active worker owns cleanup until it exits"
    );
    assert!(
        !queued_temp.exists(),
        "inactive queued temp artifact should be removed during cancel"
    );
    assert_eq!(
        snapshot
            .jobs
            .iter()
            .find(|job| job.id == "job_completed")
            .unwrap()
            .state,
        JobState::Completed
    );

    let runtime = state.inner.read().await;
    assert!(runtime.active_workers.contains("job_active"));
    assert!(!runtime.bulk_hoster_worker_health.contains_key("job_active"));
    drop(runtime);
    assert!(!state.has_handoff_auth("job_active").await);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn cancel_jobs_for_delete_cancels_unfinished_but_preserves_completed_until_cleanup() {
    let download_dir = test_runtime_dir("cancel-delete-preserves-completed");
    let active_temp = download_dir.join("active.zip.part");
    let failed_temp = download_dir.join("failed.zip.part");
    let completed_target = download_dir.join("completed.zip");
    std::fs::write(&active_temp, b"active partial").unwrap();
    std::fs::write(&failed_temp, b"failed partial").unwrap();
    std::fs::write(&completed_target, b"complete").unwrap();

    let archive = bulk_archive_info(&download_dir, "bulk_cancel_delete");
    let mut active = protected_hoster_bulk_job("job_active", archive);
    active.state = JobState::Downloading;
    active.downloaded_bytes = 64;
    active.progress = 64.0;
    active.temp_path = active_temp.display().to_string();
    let mut failed = download_job("job_failed", JobState::Failed, ResumeSupport::Unknown, 25);
    failed.temp_path = failed_temp.display().to_string();
    failed.error = Some("network failed".into());
    let mut completed = download_job(
        "job_completed",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    completed.target_path = completed_target.display().to_string();
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active, failed, completed],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_active".into());
        seed_healthy_bulk_hoster_health(&mut runtime, "job_active", 96 * 1024);
    }
    state
        .handoff_auth
        .write()
        .await
        .insert("job_active".into(), HandoffAuth { headers: vec![] });

    let ids = vec![
        "job_active".to_string(),
        "job_failed".to_string(),
        "job_completed".to_string(),
    ];
    let prepared = tokio::time::timeout(
        Duration::from_millis(100),
        state.cancel_jobs_for_delete(&ids),
    )
    .await
    .expect("destructive cancel should not wait for active file handles")
    .expect("destructive cancel should succeed");

    let active_snapshot = prepared
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_active")
        .unwrap();
    let failed_snapshot = prepared
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_failed")
        .unwrap();
    let completed_snapshot = prepared
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_completed")
        .unwrap();
    assert_eq!(active_snapshot.state, JobState::Canceled);
    assert_eq!(failed_snapshot.state, JobState::Canceled);
    assert_eq!(completed_snapshot.state, JobState::Completed);
    assert!(
        completed_target.exists(),
        "cleanup finalizer deletes completed files later"
    );
    assert!(state
        .inner
        .read()
        .await
        .active_workers
        .contains("job_active"));
    assert!(!state.has_handoff_auth("job_active").await);

    let runtime = state.inner.read().await;
    assert!(!runtime.bulk_hoster_worker_health.contains_key("job_active"));
    drop(runtime);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn cancel_jobs_for_delete_marks_requested_jobs_removing_and_returns_cleanup_manifest() {
    let download_dir = test_runtime_dir("cancel-delete-removing-manifest");
    let queued_temp = download_dir.join("queued.bin.part");
    let completed_target = download_dir.join("completed.bin");
    std::fs::write(&queued_temp, b"queued partial").unwrap();
    std::fs::write(&completed_target, b"completed").unwrap();

    let mut queued = download_job("job_queued", JobState::Queued, ResumeSupport::Unknown, 0);
    queued.temp_path = queued_temp.display().to_string();
    let mut completed = download_job(
        "job_completed",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    completed.target_path = completed_target.display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![queued, completed]);

    let ids = vec!["job_queued".to_string(), "job_completed".to_string()];
    let prepared = state
        .cancel_jobs_for_delete(&ids)
        .await
        .expect("destructive cancel should prepare removing cleanup");

    assert_eq!(prepared.jobs.len(), 2);
    let queued_snapshot = prepared
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_queued")
        .unwrap();
    let completed_snapshot = prepared
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_completed")
        .unwrap();
    assert_eq!(queued_snapshot.state, JobState::Canceled);
    assert_eq!(queued_snapshot.removal_state, Some(RemovalState::Removing));
    assert_eq!(completed_snapshot.state, JobState::Completed);
    assert_eq!(
        completed_snapshot.removal_state,
        Some(RemovalState::Removing)
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn delete_canceled_jobs_after_release_removes_active_and_completed_bulk_files() {
    let download_dir = test_runtime_dir("cancel-delete-after-release");
    let active_temp = download_dir.join("active.zip.part");
    let completed_target = download_dir.join("completed.part001.rar");
    let archive_output = download_dir.join("Bulk").join("Game");
    std::fs::write(&active_temp, b"active partial").unwrap();
    std::fs::write(&completed_target, b"complete").unwrap();
    std::fs::create_dir_all(&archive_output).unwrap();
    std::fs::write(archive_output.join("payload.bin"), b"archive output").unwrap();

    let archive = BulkArchiveInfo {
        archive_status: BulkArchiveStatus::Completed,
        output_path: Some(archive_output.display().to_string()),
        ..bulk_archive_info(&download_dir, "bulk_cancel_delete_cleanup")
    };
    let mut active = download_job(
        "job_active",
        JobState::Downloading,
        ResumeSupport::Supported,
        50,
    );
    active.temp_path = active_temp.display().to_string();
    let mut completed = download_job(
        "job_completed",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    completed.target_path = completed_target.display().to_string();
    completed.bulk_archive = Some(archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active, completed]);
    {
        state
            .inner
            .write()
            .await
            .active_workers
            .insert("job_active".into());
    }

    let ids = vec!["job_active".to_string(), "job_completed".to_string()];
    state
        .cancel_jobs_for_delete(&ids)
        .await
        .expect("destructive cancel should mark active job canceled");

    let release_state = state.clone();
    let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release_flag = released.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        release_state
            .inner
            .write()
            .await
            .active_workers
            .remove("job_active");
        release_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    let snapshot = state
        .delete_canceled_jobs_after_release(&ids)
        .await
        .expect("cleanup finalizer should remove canceled and completed requested jobs");

    assert!(released.load(std::sync::atomic::Ordering::SeqCst));
    assert!(snapshot.jobs.is_empty());
    assert!(!active_temp.exists());
    assert!(!completed_target.exists());
    assert!(!archive_output.exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn destructive_cleanup_removes_rows_using_captured_paths_after_release() {
    let download_dir = test_runtime_dir("destructive-cleanup-captured-paths");
    let active_temp = download_dir.join("active.bin.part");
    let completed_target = download_dir.join("completed.bin");
    std::fs::write(&active_temp, b"active partial").unwrap();
    std::fs::write(&completed_target, b"complete").unwrap();

    let mut active = download_job(
        "job_active",
        JobState::Downloading,
        ResumeSupport::Supported,
        50,
    );
    active.temp_path = active_temp.display().to_string();
    let mut completed = download_job(
        "job_completed",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    completed.target_path = completed_target.display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active, completed]);
    {
        state
            .inner
            .write()
            .await
            .active_workers
            .insert("job_active".into());
    }

    let ids = vec!["job_active".to_string(), "job_completed".to_string()];
    let prepared = state
        .cancel_jobs_for_delete(&ids)
        .await
        .expect("destructive cancel should prepare cleanup from captured paths");

    let release_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        release_state
            .inner
            .write()
            .await
            .active_workers
            .remove("job_active");
    });

    let snapshot = state
        .run_destructive_cleanup(prepared.jobs)
        .await
        .expect("cleanup should remove rows using manifest paths");

    assert!(snapshot.jobs.is_empty());
    assert!(!active_temp.exists());
    assert!(!completed_target.exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn delete_canceled_torrent_after_release_waits_before_payload_cleanup() {
    let download_dir = test_runtime_dir("cancel-delete-torrent-waits");
    let target_path = download_dir.join("torrent-a634dc94");
    let temp_path = download_dir.join(".torrent-state").join("job_1");
    std::fs::create_dir_all(&target_path).unwrap();
    std::fs::write(target_path.join("payload.bin"), b"payload").unwrap();
    std::fs::create_dir_all(&temp_path).unwrap();
    let mut canceled_job = download_job("job_1", JobState::Canceled, ResumeSupport::Unsupported, 0);
    canceled_job.transfer_kind = TransferKind::Torrent;
    canceled_job.torrent = Some(TorrentInfo::default());
    canceled_job.target_path = target_path.display().to_string();
    canceled_job.temp_path = temp_path.display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![canceled_job]);
    {
        state
            .inner
            .write()
            .await
            .active_workers
            .insert("job_1".into());
    }

    let release_state = state.clone();
    let released = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release_flag = released.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        release_state
            .inner
            .write()
            .await
            .active_workers
            .remove("job_1");
        release_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    let snapshot = state
        .delete_canceled_jobs_after_release(&["job_1".to_string()])
        .await
        .expect("torrent cleanup should wait for release and then delete payload");

    assert!(released.load(std::sync::atomic::Ordering::SeqCst));
    assert!(snapshot.jobs.is_empty());
    assert!(!target_path.exists());
    assert!(!temp_path.exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn cancel_delete_cleanup_failure_keeps_canceled_row_with_diagnostic() {
    let download_dir = test_runtime_dir("cancel-delete-cleanup-failure");
    let mut job = download_job("job_locked", JobState::Queued, ResumeSupport::Unknown, 0);
    job.target_path = "\0".to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    let ids = vec!["job_locked".to_string()];

    let prepared = state
        .cancel_jobs_for_delete(&ids)
        .await
        .expect("cancel should prepare cleanup");

    let snapshot = state
        .run_destructive_cleanup(prepared.jobs)
        .await
        .expect("cleanup errors should be recorded instead of failing the finalizer");

    let remaining = snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_locked")
        .unwrap();
    assert_eq!(remaining.state, JobState::Canceled);
    assert_eq!(remaining.removal_state, Some(RemovalState::CleanupFailed));
    let runtime = state.inner.read().await;
    assert!(runtime.diagnostic_events.iter().any(|event| {
        event.level == DiagnosticLevel::Warning
            && event.job_id.as_deref() == Some("job_locked")
            && event
                .message
                .contains("Could not delete canceled download files")
    }));
    drop(runtime);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn delete_job_missing_ids_are_idempotent() {
    let download_dir = test_runtime_dir("delete-missing-idempotent");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());

    state
        .delete_job("missing", false)
        .await
        .expect("removing an already deleted row should be a no-op");
    state
        .delete_job("missing", true)
        .await
        .expect("deleting already deleted files should be a no-op");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn delete_paused_seeding_torrent_waits_for_worker_release_and_clears_reseed() {
    let download_dir = test_runtime_dir("delete-paused-seeding-torrent");
    let target_path = download_dir.join("seeded-output");
    let temp_path = download_dir.join(".torrent-state").join("job_1");
    std::fs::create_dir_all(&target_path).unwrap();
    std::fs::write(target_path.join("payload.bin"), b"payload").unwrap();
    std::fs::create_dir_all(&temp_path).unwrap();
    let mut paused_job = download_job("job_1", JobState::Paused, ResumeSupport::Unsupported, 100);
    paused_job.transfer_kind = TransferKind::Torrent;
    paused_job.progress = 100.0;
    paused_job.target_path = target_path.display().to_string();
    paused_job.temp_path = temp_path.display().to_string();
    paused_job.torrent = Some(TorrentInfo {
        engine_id: Some(42),
        info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        seeding_started_at: Some(123_456),
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![paused_job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_1".into());
        runtime.external_reseed_jobs.insert("job_1".into());
    }
    let release_state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        release_state
            .inner
            .write()
            .await
            .active_workers
            .remove("job_1");
    });

    let snapshot = state
        .delete_job("job_1", true)
        .await
        .expect("paused seeding torrent deletion should wait briefly for worker release");

    assert!(snapshot.jobs.is_empty());
    let runtime = state.inner.read().await;
    assert!(!runtime.active_workers.contains("job_1"));
    assert!(!runtime.external_reseed_jobs.contains("job_1"));
    drop(runtime);
    assert!(!target_path.exists());
    assert!(!temp_path.exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

fn write_partial_sidecars(temp_path: &Path) -> Vec<PathBuf> {
    let sidecars = vec![
        temp_path.to_path_buf(),
        PathBuf::from(format!("{}.meta", temp_path.display())),
        PathBuf::from(format!("{}.meta.tmp", temp_path.display())),
        PathBuf::from(format!("{}.meta.bak", temp_path.display())),
        PathBuf::from(format!("{}.meta.42.tmp", temp_path.display())),
        PathBuf::from(format!("{}.seg0", temp_path.display())),
    ];

    for path in &sidecars {
        std::fs::write(path, b"partial").unwrap();
    }

    sidecars
}

fn assert_partial_artifacts_removed(temp_path: &Path, sidecars: &[PathBuf]) {
    assert!(!temp_path.exists(), "partial file should be removed");
    for path in sidecars {
        assert!(
            !path.exists(),
            "partial sidecar should be removed: {}",
            path.display()
        );
    }
}
