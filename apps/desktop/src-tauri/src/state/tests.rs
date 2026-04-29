use super::*;
use crate::storage::HostRegistrationStatus;

#[test]
fn queue_summary_counts_attention_jobs() {
    let state = RuntimeState {
        connection_state: ConnectionState::Connected,
        jobs: vec![
            download_job("job_1", JobState::Failed, ResumeSupport::Supported, 25),
            download_job("job_2", JobState::Paused, ResumeSupport::Unsupported, 40),
            download_job(
                "job_3",
                JobState::Downloading,
                ResumeSupport::Unsupported,
                0,
            ),
            download_job(
                "job_4",
                JobState::Completed,
                ResumeSupport::Unsupported,
                100,
            ),
            download_job("job_5", JobState::Queued, ResumeSupport::Unknown, 0),
        ],
        settings: Settings::default(),
        main_window: None,
        diagnostic_events: Vec::new(),
        next_job_number: 6,
        active_workers: HashSet::new(),
        external_reseed_jobs: HashSet::new(),
        last_host_contact: None,
    };

    let summary = state.queue_summary();

    assert_eq!(summary.attention, 2);
}

#[test]
fn save_settings_sync_persists_startup_preferences() {
    let download_dir = test_runtime_dir("save-settings-sync");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);
    let mut settings = state.settings_sync();
    settings.download_directory = download_dir.display().to_string();
    settings.start_on_startup = true;
    settings.startup_launch_mode = crate::storage::StartupLaunchMode::Tray;

    state
        .save_settings_sync(settings)
        .expect("settings should persist synchronously");

    let persisted = load_persisted_state(&download_dir.join("state.json"))
        .expect("persisted state should load");
    assert!(persisted.settings.start_on_startup);
    assert_eq!(
        persisted.settings.startup_launch_mode,
        crate::storage::StartupLaunchMode::Tray
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn authenticated_handoff_auth_is_memory_only_and_claimed_with_task() {
    let download_dir = test_runtime_dir("auth-handoff-memory");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);
    let mut settings = state.settings().await;
    settings.download_directory = download_dir.display().to_string();
    settings.extension_integration.authenticated_handoff_enabled = true;
    state.save_settings(settings).await.unwrap();

    let auth = HandoffAuth {
        headers: vec![HandoffAuthHeader {
            name: "Cookie".into(),
            value: "session=abc".into(),
        }],
    };
    let result = state
        .enqueue_download_with_options(
            "https://chatgpt.com/backend-api/estuary/content?id=file_123".into(),
            EnqueueOptions {
                source: Some(DownloadSource {
                    entry_point: "browser_download".into(),
                    browser: "chrome".into(),
                    extension_version: "0.3.41".into(),
                    page_url: None,
                    page_title: None,
                    referrer: None,
                    incognito: Some(false),
                }),
                handoff_auth: Some(auth.clone()),
                ..Default::default()
            },
        )
        .await
        .expect("protected auth handoff should enqueue without a host allowlist");

    assert!(state.has_handoff_auth(&result.job_id).await);
    let raw_state = std::fs::read_to_string(download_dir.join("state.json")).unwrap();
    assert!(!raw_state.contains("session=abc"));

    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].handoff_auth.as_ref(), Some(&auth));

    state.clear_handoff_auth(&result.job_id).await;
    assert!(!state.has_handoff_auth(&result.job_id).await);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn duplicate_enqueue_result_includes_existing_job_details() {
    let mut existing_job = download_job("job_9", JobState::Paused, ResumeSupport::Supported, 50);
    existing_job.url = "https://example.com/file.zip".into();
    existing_job.filename = "file.zip".into();

    let state = RuntimeState {
        connection_state: ConnectionState::Connected,
        jobs: vec![existing_job],
        settings: Settings::default(),
        main_window: None,
        diagnostic_events: Vec::new(),
        next_job_number: 10,
        active_workers: HashSet::new(),
        external_reseed_jobs: HashSet::new(),
        last_host_contact: None,
    };

    let result = state
        .duplicate_enqueue_result("https://example.com/file.zip")
        .expect("duplicate result");

    assert_eq!(result.status, EnqueueStatus::DuplicateExistingJob);
    assert_eq!(result.job_id, "job_9");
    assert_eq!(result.filename, "file.zip");
    assert_eq!(result.snapshot.jobs.len(), 1);
}

#[test]
fn enqueue_options_allow_duplicate_copy_with_unique_path() {
    let download_dir = test_runtime_dir("duplicate-copy");
    let mut existing_job = download_job("job_9", JobState::Paused, ResumeSupport::Supported, 50);
    existing_job.url = "https://example.com/file.zip".into();
    existing_job.filename = "file.zip".into();
    existing_job.target_path = download_dir.join("file.zip").display().to_string();
    existing_job.temp_path = download_dir.join("file.zip.part").display().to_string();

    let mut state = runtime_state_with_jobs(vec![existing_job]);
    state.settings.download_directory = download_dir.display().to_string();

    let result = state
        .enqueue_download_in_memory(
            "https://example.com/file.zip",
            EnqueueOptions {
                duplicate_policy: DuplicatePolicy::Allow,
                ..Default::default()
            },
        )
        .expect("duplicate copy should enqueue");

    assert_eq!(result.status, EnqueueStatus::Queued);
    assert_eq!(state.jobs.len(), 2);
    assert_eq!(state.jobs[1].filename, "file.zip");
    assert_eq!(
        target_parent_folder(&state.jobs[1].target_path),
        "Compressed"
    );
    assert!(state.jobs[1].target_path.ends_with("file.zip"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn enqueue_options_replace_duplicate_removes_replaceable_queue_job() {
    let download_dir = test_runtime_dir("duplicate-replace-queued");
    let mut existing_job = download_job("job_9", JobState::Queued, ResumeSupport::Supported, 0);
    existing_job.url = "https://example.com/file.zip".into();
    existing_job.filename = "file.zip".into();
    existing_job.target_path = download_dir.join("file.zip").display().to_string();
    let temp_path = download_dir.join("file.zip.part");
    std::fs::write(&temp_path, b"partial").unwrap();
    existing_job.temp_path = temp_path.display().to_string();

    let mut state = runtime_state_with_jobs(vec![existing_job]);
    state.settings.download_directory = download_dir.display().to_string();

    let result = state
        .enqueue_download_in_memory(
            "https://example.com/file.zip",
            EnqueueOptions {
                duplicate_policy: DuplicatePolicy::ReplaceExisting,
                ..Default::default()
            },
        )
        .expect("replaceable duplicate should be replaced");

    assert_eq!(result.status, EnqueueStatus::Queued);
    assert_eq!(state.jobs.len(), 1);
    assert_ne!(state.jobs[0].id, "job_9");
    assert_eq!(state.jobs[0].filename, "file.zip");
    assert!(!temp_path.exists());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn enqueue_options_replace_duplicate_rejects_active_duplicate() {
    let download_dir = test_runtime_dir("duplicate-replace-active");
    let mut existing_job =
        download_job("job_9", JobState::Downloading, ResumeSupport::Supported, 25);
    existing_job.url = "https://example.com/file.zip".into();

    let mut state = runtime_state_with_jobs(vec![existing_job]);
    state.settings.download_directory = download_dir.display().to_string();
    state.active_workers.insert("job_9".into());

    let error = state
        .enqueue_download_in_memory(
            "https://example.com/file.zip",
            EnqueueOptions {
                duplicate_policy: DuplicatePolicy::ReplaceExisting,
                ..Default::default()
            },
        )
        .expect_err("active duplicate should not be replaced");

    assert_eq!(error.code, "DUPLICATE_ACTIVE");
    assert_eq!(state.jobs.len(), 1);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn enqueue_options_replace_duplicate_keeps_completed_target_file() {
    let download_dir = test_runtime_dir("duplicate-replace-completed");
    let target_path = download_dir.join("file.zip");
    std::fs::write(&target_path, b"completed").unwrap();

    let mut existing_job =
        download_job("job_9", JobState::Completed, ResumeSupport::Supported, 100);
    existing_job.url = "https://example.com/file.zip".into();
    existing_job.filename = "file.zip".into();
    existing_job.target_path = target_path.display().to_string();
    existing_job.temp_path = format!("{}.part", existing_job.target_path);

    let mut state = runtime_state_with_jobs(vec![existing_job]);
    state.settings.download_directory = download_dir.display().to_string();

    let result = state
        .enqueue_download_in_memory(
            "https://example.com/file.zip",
            EnqueueOptions {
                duplicate_policy: DuplicatePolicy::ReplaceExisting,
                ..Default::default()
            },
        )
        .expect("completed duplicate should be replaced without deleting the artifact");

    assert_eq!(result.status, EnqueueStatus::Queued);
    assert!(target_path.exists());
    assert_eq!(std::fs::read(&target_path).unwrap(), b"completed");
    assert_ne!(PathBuf::from(&state.jobs[0].target_path), target_path);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn enqueue_options_use_directory_override_without_saving_default() {
    let default_dir = test_runtime_dir("default-dir");
    let override_dir = test_runtime_dir("override-dir");
    let mut state = runtime_state_with_jobs(Vec::new());
    state.settings.download_directory = default_dir.display().to_string();

    let result = state
        .enqueue_download_in_memory(
            "https://example.com/report.pdf",
            EnqueueOptions {
                directory_override: Some(override_dir.display().to_string()),
                ..Default::default()
            },
        )
        .expect("download should enqueue into override directory");

    assert_eq!(result.status, EnqueueStatus::Queued);
    assert!(state.jobs[0]
        .target_path
        .starts_with(&override_dir.display().to_string()));
    assert_eq!(target_parent_folder(&state.jobs[0].target_path), "Document");
    assert_eq!(
        state.settings.download_directory,
        default_dir.display().to_string()
    );

    let _ = std::fs::remove_dir_all(default_dir);
    let _ = std::fs::remove_dir_all(override_dir);
}

#[test]
fn validate_settings_creates_download_category_directories() {
    let download_dir = test_runtime_dir("category-settings");
    let mut settings = Settings {
        download_directory: download_dir.display().to_string(),
        ..Settings::default()
    };

    validate_settings(&mut settings).expect("settings should validate");

    for folder in [
        "Document",
        "Program",
        "Picture",
        "Video",
        "Compressed",
        "Music",
        "Other",
    ] {
        assert!(
            download_dir.join(folder).is_dir(),
            "{folder} category directory should exist"
        );
    }

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn enqueue_routes_downloads_into_category_directories() {
    let download_dir = test_runtime_dir("category-routing");
    let mut state = runtime_state_with_jobs(Vec::new());
    state.settings.download_directory = download_dir.display().to_string();

    for url in [
        "https://example.com/archive.zip",
        "https://example.com/setup.exe",
        "https://example.com/photo.jpg",
        "https://example.com/movie.mp4",
        "https://example.com/song.flac",
        "https://example.com/blob.custom",
    ] {
        state
            .enqueue_download_in_memory(url, EnqueueOptions::default())
            .expect("download should enqueue");
    }

    let folders = state
        .jobs
        .iter()
        .map(|job| target_parent_folder(&job.target_path))
        .collect::<Vec<_>>();

    assert_eq!(
        folders,
        vec![
            "Compressed",
            "Program",
            "Picture",
            "Video",
            "Music",
            "Other"
        ]
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn prepare_download_prompt_marks_duplicate_job() {
    let mut existing_job = download_job(
        "job_12",
        JobState::Downloading,
        ResumeSupport::Supported,
        20,
    );
    existing_job.url = "https://example.com/archive.zip".into();
    existing_job.filename = "archive.zip".into();

    let download_dir = test_runtime_dir("prompt-category");
    let mut state = runtime_state_with_jobs(vec![existing_job]);
    state.settings.download_directory = download_dir.display().to_string();
    let prompt = state
        .prepare_download_prompt(
            "prompt_1",
            "https://example.com/archive.zip",
            None,
            Some("archive.zip".into()),
            Some(4096),
        )
        .expect("prompt should be prepared");

    assert_eq!(prompt.id, "prompt_1");
    assert_eq!(prompt.filename, "archive.zip");
    assert_eq!(prompt.total_bytes, Some(4096));
    assert_eq!(
        prompt.duplicate_job.as_ref().map(|job| job.id.as_str()),
        Some("job_12")
    );
    assert_eq!(target_parent_folder(&prompt.target_path), "Compressed");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn destination_write_probe_reports_blocked_probe_path() {
    let test_dir = test_runtime_dir("destination-write-probe");
    let probe_name = "blocked-probe";
    std::fs::create_dir(test_dir.join(probe_name)).unwrap();

    let error =
        verify_download_directory_writable_with_probe_name(&test_dir, probe_name).unwrap_err();

    assert!(matches!(
        error.code,
        "DESTINATION_INVALID" | "PERMISSION_DENIED"
    ));
    assert!(error.message.contains("not writable"));

    let _ = std::fs::remove_dir_all(test_dir);
}

#[test]
fn download_filename_metadata_updates_display_name_without_moving_partial_file() {
    let mut job = download_job(
        "job_11",
        JobState::Downloading,
        ResumeSupport::Supported,
        10,
    );
    job.filename = "download.bin".into();
    job.target_path = "C:/Downloads/download.bin".into();
    job.temp_path = "C:/Downloads/download.bin.part".into();

    apply_download_filename(&mut job, "server-report.pdf");

    assert_eq!(job.filename, "server-report.pdf");
    assert_eq!(job.target_path, "C:/Downloads/download.bin");
    assert_eq!(job.temp_path, "C:/Downloads/download.bin.part");
}

#[test]
fn preflight_metadata_updates_job_size_resume_and_filename() {
    let mut job = download_job("job_12", JobState::Starting, ResumeSupport::Unknown, 0);
    job.filename = "download.bin".into();
    job.total_bytes = 0;

    apply_preflight_metadata_to_job(
        &mut job,
        Some(4_096),
        ResumeSupport::Supported,
        Some("server-report.pdf".into()),
    );

    assert_eq!(job.filename, "server-report.pdf");
    assert_eq!(job.total_bytes, 4_096);
    assert_eq!(job.resume_support, ResumeSupport::Supported);
    assert_eq!(job.progress, 0.0);
}

#[tokio::test]
async fn reveal_completed_job_errors_when_file_is_missing_even_if_parent_exists() {
    let download_dir = test_runtime_dir("reveal-missing-completed");
    let target_path = download_dir.join("missing.zip");
    let mut job = download_job("job_20", JobState::Completed, ResumeSupport::Supported, 100);
    job.target_path = target_path.display().to_string();
    job.temp_path = format!("{}.part", job.target_path);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let error = state.resolve_revealable_path("job_20").await.unwrap_err();

    assert_eq!(error.code, "INTERNAL_ERROR");
    assert!(error
        .message
        .contains("Downloaded file is missing from disk"));
    assert!(error.message.contains(&target_path.display().to_string()));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn reveal_completed_job_returns_existing_target_file() {
    let download_dir = test_runtime_dir("reveal-completed-existing");
    let target_path = download_dir.join("file.zip");
    std::fs::write(&target_path, b"downloaded").unwrap();
    let mut job = download_job("job_21", JobState::Completed, ResumeSupport::Supported, 100);
    job.target_path = target_path.display().to_string();
    job.temp_path = format!("{}.part", job.target_path);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let resolved = state.resolve_revealable_path("job_21").await.unwrap();

    assert_eq!(resolved, target_path);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn open_completed_torrent_directory_returns_target_directory() {
    let download_dir = test_runtime_dir("open-completed-torrent-directory");
    let target_path = download_dir.join("Example Torrent");
    std::fs::create_dir_all(&target_path).unwrap();
    let mut job = download_job("job_27", JobState::Completed, ResumeSupport::Supported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.target_path = target_path.display().to_string();
    job.temp_path = download_dir.join(".torrent-state").display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let resolved = state.resolve_openable_path("job_27").await.unwrap();

    assert_eq!(resolved, target_path);

    let _ = std::fs::remove_dir_all(download_dir);
}

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
async fn prepared_missing_torrent_directory_still_returns_open_error() {
    let download_dir = test_runtime_dir("prepare-missing-torrent-external-use");
    let target_path = download_dir.join("missing-torrent");
    let mut job = download_job("job_35", JobState::Seeding, ResumeSupport::Unsupported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.progress = 100.0;
    job.target_path = target_path.display().to_string();
    job.temp_path = download_dir.join(".torrent-state").display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let preparation = state
        .prepare_job_for_external_use_with_wait(
            "job_35",
            Duration::from_millis(50),
            Duration::from_millis(5),
        )
        .await
        .expect("missing torrent should still pause before resolving the path");
    let error = state.resolve_openable_path("job_35").await.unwrap_err();

    assert!(preparation.paused_torrent);
    assert_eq!(error.code, "INTERNAL_ERROR");
    assert!(error
        .message
        .contains("The downloaded file is not available on disk"));

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
async fn open_completed_http_directory_still_requires_file() {
    let download_dir = test_runtime_dir("open-http-directory-rejected");
    let target_path = download_dir.join("not-a-file");
    std::fs::create_dir_all(&target_path).unwrap();
    let mut job = download_job("job_28", JobState::Completed, ResumeSupport::Supported, 100);
    job.target_path = target_path.display().to_string();
    job.temp_path = format!("{}.part", job.target_path);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let error = state.resolve_openable_path("job_28").await.unwrap_err();

    assert_eq!(error.code, "INTERNAL_ERROR");
    assert!(error
        .message
        .contains("The downloaded file is not available on disk"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn open_missing_torrent_directory_returns_action_error() {
    let download_dir = test_runtime_dir("open-missing-torrent-directory");
    let target_path = download_dir.join("missing-torrent");
    let mut job = download_job("job_29", JobState::Completed, ResumeSupport::Supported, 100);
    job.transfer_kind = TransferKind::Torrent;
    job.target_path = target_path.display().to_string();
    job.temp_path = download_dir.join(".torrent-state").display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let error = state.resolve_openable_path("job_29").await.unwrap_err();

    assert_eq!(error.code, "INTERNAL_ERROR");
    assert!(error
        .message
        .contains("The downloaded file is not available on disk"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn reveal_interrupted_job_returns_existing_partial_file() {
    let download_dir = test_runtime_dir("reveal-partial-existing");
    let target_path = download_dir.join("file.zip");
    let temp_path = download_dir.join("file.zip.part");
    std::fs::write(&temp_path, b"partial").unwrap();
    let mut job = download_job("job_22", JobState::Failed, ResumeSupport::Supported, 50);
    job.target_path = target_path.display().to_string();
    job.temp_path = temp_path.display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let resolved = state.resolve_revealable_path("job_22").await.unwrap();

    assert_eq!(resolved, temp_path);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn reveal_unfinished_job_without_artifact_returns_parent_directory() {
    let download_dir = test_runtime_dir("reveal-parent-for-unfinished");
    let target_path = download_dir.join("future.zip");
    let mut job = download_job("job_23", JobState::Queued, ResumeSupport::Unknown, 0);
    job.target_path = target_path.display().to_string();
    job.temp_path = format!("{}.part", job.target_path);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let resolved = state.resolve_revealable_path("job_23").await.unwrap();

    assert_eq!(resolved, download_dir);

    let _ = std::fs::remove_dir_all(resolved);
}

#[test]
fn snapshot_marks_completed_artifact_existence() {
    let download_dir = test_runtime_dir("snapshot-artifact-existence");
    let existing_path = download_dir.join("exists.pdf");
    std::fs::write(&existing_path, b"done").unwrap();
    let missing_path = download_dir.join("missing.zip");

    let mut existing_job =
        download_job("job_24", JobState::Completed, ResumeSupport::Supported, 100);
    existing_job.target_path = existing_path.display().to_string();
    let mut missing_job =
        download_job("job_25", JobState::Completed, ResumeSupport::Supported, 100);
    missing_job.target_path = missing_path.display().to_string();

    let state = runtime_state_with_jobs(vec![existing_job, missing_job]);
    let snapshot = state.snapshot();

    assert_eq!(snapshot.jobs[0].artifact_exists, Some(true));
    assert_eq!(snapshot.jobs[1].artifact_exists, Some(false));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn normalize_job_populates_missing_created_at() {
    let mut job = download_job("job_26", JobState::Queued, ResumeSupport::Unknown, 0);
    job.created_at = 0;

    let normalized = normalize_job(job, &Settings::default());

    assert!(normalized.created_at > 0);
}

#[test]
fn pause_all_jobs_only_pauses_schedulable_jobs() {
    let mut state = runtime_state_with_jobs(vec![
        download_job("job_1", JobState::Queued, ResumeSupport::Unknown, 0),
        download_job("job_2", JobState::Starting, ResumeSupport::Unknown, 0),
        download_job("job_3", JobState::Downloading, ResumeSupport::Supported, 10),
        download_job("job_4", JobState::Completed, ResumeSupport::Supported, 100),
        download_job("job_5", JobState::Failed, ResumeSupport::Supported, 20),
    ]);

    state.pause_all_jobs();

    assert_eq!(state.jobs[0].state, JobState::Paused);
    assert_eq!(state.jobs[1].state, JobState::Paused);
    assert_eq!(state.jobs[2].state, JobState::Paused);
    assert_eq!(state.jobs[2].speed, 0);
    assert_eq!(state.jobs[2].eta, 0);
    assert_eq!(state.jobs[3].state, JobState::Completed);
    assert_eq!(state.jobs[4].state, JobState::Failed);
}

#[test]
fn resume_all_jobs_requeues_interrupted_jobs_and_clears_failures() {
    let mut failed_job = download_job("job_2", JobState::Failed, ResumeSupport::Supported, 20);
    failed_job.error = Some("server closed the connection".into());
    failed_job.failure_category = Some(FailureCategory::Network);
    failed_job.retry_attempts = 2;

    let mut state = runtime_state_with_jobs(vec![
        download_job("job_1", JobState::Paused, ResumeSupport::Unknown, 0),
        failed_job,
        download_job("job_3", JobState::Canceled, ResumeSupport::Unknown, 0),
        download_job("job_4", JobState::Completed, ResumeSupport::Supported, 100),
        download_job("job_5", JobState::Downloading, ResumeSupport::Supported, 10),
    ]);

    state.resume_all_jobs();

    assert_eq!(state.jobs[0].state, JobState::Queued);
    assert_eq!(state.jobs[1].state, JobState::Queued);
    assert_eq!(state.jobs[1].error, None);
    assert_eq!(state.jobs[1].failure_category, None);
    assert_eq!(state.jobs[1].retry_attempts, 0);
    assert_eq!(state.jobs[2].state, JobState::Queued);
    assert_eq!(state.jobs[3].state, JobState::Completed);
    assert_eq!(state.jobs[4].state, JobState::Downloading);
}

#[test]
fn normalize_download_url_trims_pasted_whitespace() {
    let normalized =
        normalize_download_url(" \n https://example.com/file.zip?from=clipboard \t ").unwrap();

    assert_eq!(normalized, "https://example.com/file.zip?from=clipboard");
}

#[test]
fn normalize_download_url_rejects_urls_over_protocol_limit() {
    let long_url = format!("https://example.com/{}", "a".repeat(2_048));

    let error = normalize_download_url(&long_url).unwrap_err();

    assert_eq!(error.code, "URL_TOO_LONG");
}

#[test]
fn normalize_download_url_accepts_torrent_inputs() {
    let magnet = " magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example ";
    let torrent_url = "https://example.com/releases/example.torrent";

    assert_eq!(
        normalize_download_url(magnet).unwrap(),
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example"
    );
    assert_eq!(normalize_download_url(torrent_url).unwrap(), torrent_url);
}

#[test]
fn normalize_download_url_rejects_non_torrent_non_http_schemes() {
    let error = normalize_download_url("ftp://example.com/file.torrent").unwrap_err();

    assert_eq!(error.code, "UNSUPPORTED_SCHEME");
    assert!(error.message.contains("http, https, magnet"));
}

#[test]
fn enqueue_download_in_memory_creates_torrent_job_for_magnet() {
    let download_dir = test_runtime_dir("enqueue-torrent");
    let mut state = runtime_state_with_jobs(Vec::new());
    state.settings.download_directory = download_dir.display().to_string();
    let result = state
        .enqueue_download_in_memory(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Fedora",
            EnqueueOptions::default(),
        )
        .unwrap();

    let job = result
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == result.job_id)
        .expect("queued job");
    assert_eq!(job.transfer_kind, TransferKind::Torrent);
    assert_eq!(job.filename, "Fedora");
    assert!(job.integrity_check.is_none());
    assert!(job.target_path.ends_with("Fedora"));
    assert!(job.temp_path.contains(".torrent-state"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn enqueue_download_in_memory_creates_torrent_job_for_local_file() {
    let download_dir = test_runtime_dir("enqueue-local-torrent");
    let torrent_file = download_dir.join("fixture.torrent");
    std::fs::create_dir_all(&download_dir).unwrap();
    std::fs::write(
        &torrent_file,
        b"d4:infod4:name7:fixture12:piece lengthi16e6:pieces0:e",
    )
    .unwrap();
    let mut state = runtime_state_with_jobs(Vec::new());
    state.settings.download_directory = download_dir.display().to_string();

    let result = state
        .enqueue_download_in_memory(
            &torrent_file.display().to_string(),
            EnqueueOptions {
                transfer_kind: Some(TransferKind::Torrent),
                ..Default::default()
            },
        )
        .unwrap();

    let job = result
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == result.job_id)
        .expect("queued job");
    assert_eq!(job.transfer_kind, TransferKind::Torrent);
    assert_eq!(job.filename, "fixture");
    assert_eq!(job.url, torrent_file.display().to_string());
    assert!(job.temp_path.contains(".torrent-state"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn enqueue_download_rejects_mismatched_explicit_transfer_kind() {
    let download_dir = test_runtime_dir("enqueue-torrent-mismatch");
    let mut state = runtime_state_with_jobs(Vec::new());
    state.settings.download_directory = download_dir.display().to_string();

    let error = state
        .enqueue_download_in_memory(
            "https://example.com/plain-file.zip",
            EnqueueOptions {
                transfer_kind: Some(TransferKind::Torrent),
                ..Default::default()
            },
        )
        .unwrap_err();

    assert_eq!(error.code, "INVALID_TRANSFER_KIND");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn enqueue_downloads_keeps_batch_modes_http_only() {
    let download_dir = test_runtime_dir("enqueue-batch-http-only");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();

    let error = state
        .enqueue_downloads(
            vec![
                "https://example.com/file.zip".into(),
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567".into(),
            ],
            None,
            None,
        )
        .await
        .unwrap_err();

    assert_eq!(error.code, "INVALID_TRANSFER_KIND");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn sanitize_filename_falls_back_for_dot_only_names() {
    assert_eq!(sanitize_filename("."), "download.bin");
    assert_eq!(sanitize_filename(".."), "download.bin");
    assert_eq!(sanitize_filename("  ...  "), "download.bin");
}

#[test]
fn sanitize_filename_avoids_windows_reserved_device_names() {
    assert_eq!(sanitize_filename("CON"), "CON_");
    assert_eq!(sanitize_filename("con.txt"), "con.txt_");
}

#[test]
fn filename_from_hint_cannot_escape_download_directory_with_parent_segment() {
    let filename = filename_from_hint(Some(".."), "https://example.com/archive.zip");

    assert_eq!(filename, "download.bin");
}

#[test]
fn filename_from_url_decodes_percent_encoded_path_segment() {
    let filename = filename_from_hint(
            None,
            "https://example.com/%5BNanakoRaws%5D%20Tensei%20Shitara%20Slime%20Datta%20Ken%20S4%20-%2002%20%28AT-X%20TV%201080p%20HEVC%20AAC%29.mkv",
        );

    assert_eq!(
        filename,
        "[NanakoRaws] Tensei Shitara Slime Datta Ken S4 - 02 (AT-X TV 1080p HEVC AAC).mkv"
    );
}

#[test]
fn filename_from_browser_hint_decodes_percent_encoded_name() {
    let filename = filename_from_hint(
        Some("%5BASW%5D%20Re%20Zero%20kara%20Hajimeru%20Isekai%20Seikatsu.mkv"),
        "https://example.com/download",
    );

    assert_eq!(filename, "[ASW] Re Zero kara Hajimeru Isekai Seikatsu.mkv");
}

#[test]
fn legacy_default_download_directory_is_replaced_on_load() {
    assert!(should_reset_download_directory("C:/Downloads", false, true));
    assert!(should_reset_download_directory(
        "C:\\Downloads",
        false,
        true
    ));
    assert!(should_reset_download_directory(
        "C:\\Users\\You\\Downloads",
        false,
        true
    ));
    assert!(!should_reset_download_directory(
        "D:/Custom Downloads",
        false,
        true
    ));
}

#[test]
fn normalize_extension_settings_cleans_ignored_file_extensions() {
    let mut settings = ExtensionIntegrationSettings {
        ignored_file_extensions: vec![
            " .ZIP ".into(),
            "zip".into(),
            "tar.gz".into(),
            ".exe".into(),
            "invalid/path".into(),
            String::new(),
        ],
        ..ExtensionIntegrationSettings::default()
    };

    normalize_extension_settings(&mut settings);

    assert_eq!(settings.listen_port, 1420);
    assert_eq!(
        settings.ignored_file_extensions,
        vec!["zip", "tar.gz", "exe"]
    );
}

#[test]
fn normalize_extension_settings_defaults_invalid_listen_port() {
    let mut settings = ExtensionIntegrationSettings {
        listen_port: 70_000,
        ..ExtensionIntegrationSettings::default()
    };

    normalize_extension_settings(&mut settings);

    assert_eq!(settings.listen_port, 1420);
}

#[test]
fn normalize_torrent_settings_clamps_upload_limit_and_forwarding_port() {
    let mut settings = TorrentSettings {
        seed_ratio_limit: 0.0,
        seed_time_limit_minutes: 0,
        upload_limit_kib_per_second: 10_000_000,
        port_forwarding_enabled: true,
        port_forwarding_port: 80,
        ..TorrentSettings::default()
    };

    normalize_torrent_settings(&mut settings);

    assert_eq!(settings.seed_ratio_limit, 0.1);
    assert_eq!(settings.seed_time_limit_minutes, 1);
    assert_eq!(settings.upload_limit_kib_per_second, 1_048_576);
    assert!(settings.port_forwarding_enabled);
    assert_eq!(settings.port_forwarding_port, 42000);
}

#[test]
fn expected_sha256_is_validated_and_normalized() {
    let mixed_case = "A".repeat(64);

    assert_eq!(
        normalize_expected_sha256(Some(mixed_case))
            .unwrap()
            .as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );

    let error = normalize_expected_sha256(Some("abc123".into())).unwrap_err();
    assert_eq!(error.code, "INVALID_INTEGRITY_HASH");
    assert!(error.message.contains("64 hexadecimal characters"));
}

#[tokio::test]
async fn complete_job_with_matching_sha256_marks_integrity_verified() {
    let download_dir = test_runtime_dir("integrity-match");
    let target_path = download_dir.join("hello.txt");
    let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
    let mut job = download_job("job_30", JobState::Downloading, ResumeSupport::Supported, 0);
    job.target_path = target_path.display().to_string();
    job.temp_path = format!("{}.part", job.target_path);
    job.integrity_check = Some(IntegrityCheck {
        algorithm: IntegrityAlgorithm::Sha256,
        expected: expected.into(),
        actual: None,
        status: IntegrityStatus::Pending,
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    state
        .complete_job_with_integrity("job_30", 5, &target_path, Some(expected.into()))
        .await
        .unwrap();

    let runtime = state.inner.read().await;
    let job = &runtime.jobs[0];
    assert_eq!(job.state, JobState::Completed);
    assert_eq!(
        job.integrity_check.as_ref().map(|check| check.status),
        Some(IntegrityStatus::Verified)
    );
    assert_eq!(
        job.integrity_check
            .as_ref()
            .and_then(|check| check.actual.as_deref()),
        Some(expected)
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn complete_job_with_mismatched_sha256_marks_integrity_failed() {
    let download_dir = test_runtime_dir("integrity-mismatch");
    let target_path = download_dir.join("hello.txt");
    let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
    let actual = "486ea46224d1bb4fb680f34f7c9ad96a8f24ec88be73ea8e5a6c65260e9cb8a7";
    let mut job = download_job("job_31", JobState::Downloading, ResumeSupport::Supported, 0);
    job.target_path = target_path.display().to_string();
    job.temp_path = format!("{}.part", job.target_path);
    job.integrity_check = Some(IntegrityCheck {
        algorithm: IntegrityAlgorithm::Sha256,
        expected: expected.into(),
        actual: None,
        status: IntegrityStatus::Pending,
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    state
        .complete_job_with_integrity("job_31", 5, &target_path, Some(actual.into()))
        .await
        .unwrap();

    let runtime = state.inner.read().await;
    let job = &runtime.jobs[0];
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.failure_category, Some(FailureCategory::Integrity));
    assert!(job.error.as_deref().unwrap_or_default().contains("SHA-256"));
    assert_eq!(
        job.integrity_check.as_ref().map(|check| check.status),
        Some(IntegrityStatus::Failed)
    );
    assert_eq!(
        job.integrity_check
            .as_ref()
            .and_then(|check| check.actual.as_deref()),
        Some(actual)
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn diagnostics_keep_newest_hundred_events() {
    let download_dir = test_runtime_dir("diagnostic-events");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);

    for index in 0..105 {
        state
            .record_diagnostic_event(
                DiagnosticLevel::Info,
                "test",
                format!("event {index}"),
                None,
            )
            .await
            .unwrap();
    }

    let snapshot = state
        .diagnostics_snapshot(HostRegistrationDiagnostics {
            status: HostRegistrationStatus::Configured,
            entries: Vec::new(),
        })
        .await;

    assert_eq!(snapshot.recent_events.len(), 100);
    assert_eq!(snapshot.recent_events[0].message, "event 5");
    assert_eq!(snapshot.recent_events[99].message, "event 104");

    let _ = std::fs::remove_dir_all(download_dir);
}

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
        created_at: 1,
        progress: 42.0,
        total_bytes: 100,
        downloaded_bytes: 42,
        speed: 2048,
        eta: 12,
        error: Some("server closed the connection".into()),
        failure_category: Some(FailureCategory::Network),
        resume_support: ResumeSupport::Supported,
        retry_attempts: 2,
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
        }),
        state: JobState::Paused,
        created_at: 1,
        progress: 100.0,
        total_bytes: 4096,
        downloaded_bytes: 4096,
        speed: 2048,
        eta: 0,
        error: Some("previous torrent error".into()),
        failure_category: Some(FailureCategory::Torrent),
        resume_support: ResumeSupport::Unsupported,
        retry_attempts: 2,
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
        archive_status: BulkArchiveStatus::Pending,
        output_path: None,
        error: None,
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
        Some("C:/Downloads/bundle.zip".into()),
        None,
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

    state.mark_bulk_archive_status_in_memory(
        "bulk_1",
        BulkArchiveStatus::Failed,
        Some("C:/Downloads/bundle.zip".into()),
        Some("zip failed".into()),
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
        Some("C:/Downloads/bundle.zip".into()),
        None,
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
async fn seeding_jobs_release_download_scheduler_slots() {
    let download_dir = test_runtime_dir("seeding-slots");
    let mut seeding_job = download_job("job_1", JobState::Seeding, ResumeSupport::Unsupported, 100);
    seeding_job.transfer_kind = TransferKind::Torrent;
    seeding_job.progress = 100.0;
    seeding_job.target_path = download_dir.join("seeded").display().to_string();
    seeding_job.temp_path = download_dir
        .join(".torrent-state")
        .join("job_1")
        .display()
        .to_string();
    let mut queued_job = download_job("job_2", JobState::Queued, ResumeSupport::Unknown, 0);
    queued_job.target_path = download_dir.join("queued.zip").display().to_string();
    queued_job.temp_path = download_dir.join("queued.zip.part").display().to_string();
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![seeding_job, queued_job],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.active_workers.insert("job_1".into());
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "job_2");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn queued_torrent_task_preserves_resume_metadata() {
    let download_dir = test_runtime_dir("torrent-task-resume-metadata");
    let mut queued_job = download_job("job_1", JobState::Queued, ResumeSupport::Unknown, 0);
    queued_job.transfer_kind = TransferKind::Torrent;
    queued_job.target_path = download_dir.join("torrent-output").display().to_string();
    queued_job.temp_path = download_dir
        .join(".torrent-state")
        .join("job_1")
        .display()
        .to_string();
    queued_job.torrent = Some(TorrentInfo {
        engine_id: Some(42),
        info_hash: Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8".into()),
        seeding_started_at: Some(123_456),
        ..TorrentInfo::default()
    });
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![queued_job]);

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming torrent job should work");

    assert_eq!(tasks.len(), 1);
    let torrent = tasks[0]
        .torrent
        .as_ref()
        .expect("torrent resume metadata should be preserved");
    assert_eq!(torrent.engine_id, Some(42));
    assert_eq!(
        torrent.info_hash.as_deref(),
        Some("420f3778a160fbe6eb0a67c8470256be13b0ecc8")
    );
    assert_eq!(torrent.seeding_started_at, Some(123_456));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn seed_policy_defaults_to_forever_and_supports_limits() {
    let mut settings = Settings::default();
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

fn download_job(
    id: &str,
    state: JobState,
    resume_support: ResumeSupport,
    downloaded_bytes: u64,
) -> DownloadJob {
    DownloadJob {
        id: id.into(),
        url: format!("https://example.com/{id}.zip"),
        filename: format!("{id}.zip"),
        source: None,
        transfer_kind: TransferKind::Http,
        integrity_check: None,
        torrent: None,
        state,
        created_at: 1,
        progress: 0.0,
        total_bytes: 100,
        downloaded_bytes,
        speed: 0,
        eta: 0,
        error: None,
        failure_category: None,
        resume_support,
        retry_attempts: 0,
        target_path: format!("C:/Downloads/{id}.zip"),
        temp_path: format!("C:/Downloads/{id}.zip.part"),
        artifact_exists: None,
        bulk_archive: None,
    }
}

fn torrent_runtime_update(
    uploaded_bytes: u64,
    downloaded_bytes: u64,
    finished: bool,
) -> TorrentRuntimeSnapshot {
    TorrentRuntimeSnapshot {
        engine_id: 42,
        info_hash: "420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        name: None,
        total_files: Some(1),
        peers: Some(2),
        seeds: Some(3),
        downloaded_bytes,
        total_bytes: downloaded_bytes,
        uploaded_bytes,
        download_speed: 0,
        upload_speed: 0,
        eta: None,
        fetched_bytes: 0,
        finished,
        error: None,
    }
}

fn runtime_state_with_jobs(jobs: Vec<DownloadJob>) -> RuntimeState {
    RuntimeState {
        connection_state: ConnectionState::Connected,
        jobs,
        settings: Settings::default(),
        main_window: None,
        diagnostic_events: Vec::new(),
        next_job_number: 99,
        active_workers: HashSet::new(),
        external_reseed_jobs: HashSet::new(),
        last_host_contact: None,
    }
}

fn shared_state_with_jobs(storage_path: PathBuf, jobs: Vec<DownloadJob>) -> SharedState {
    SharedState {
        inner: Arc::new(RwLock::new(runtime_state_with_jobs(jobs))),
        storage_path: Arc::new(storage_path),
        handoff_auth: Arc::new(RwLock::new(HashMap::new())),
    }
}

fn test_runtime_dir(name: &str) -> PathBuf {
    let dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn target_parent_folder(target_path: &str) -> String {
    PathBuf::from(target_path)
        .parent()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}
