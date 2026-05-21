use super::*;

#[test]
fn queue_summary_counts_attention_jobs() {
    let state = runtime_state_with_jobs(vec![
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
    ]);

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

#[test]
fn normalize_extension_settings_migrates_legacy_protected_downloads() {
    let mut settings = ExtensionIntegrationSettings {
        authenticated_handoff_enabled: true,
        protected_download_auth_scope: ProtectedDownloadAuthScope::Off,
        authenticated_handoff_hosts: vec![],
        ..ExtensionIntegrationSettings::default()
    };

    normalize_extension_settings(&mut settings);

    assert!(settings.authenticated_handoff_enabled);
    assert_eq!(
        settings.protected_download_auth_scope,
        ProtectedDownloadAuthScope::LegacyGlobal
    );
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

#[tokio::test]
async fn authenticated_handoff_auth_requires_allowed_host() {
    let download_dir = test_runtime_dir("auth-handoff-allowlist");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);
    let mut settings = state.settings().await;
    settings.download_directory = download_dir.display().to_string();
    settings.extension_integration.authenticated_handoff_enabled = true;
    settings.extension_integration.protected_download_auth_scope =
        ProtectedDownloadAuthScope::Allowlist;
    settings.extension_integration.authenticated_handoff_hosts = vec!["chatgpt.com".into()];
    state.save_settings(settings).await.unwrap();

    let auth = HandoffAuth {
        headers: vec![HandoffAuthHeader {
            name: "Cookie".into(),
            value: "session=abc".into(),
        }],
    };

    let error = state
        .enqueue_download_with_options(
            "https://example.com/file.zip".into(),
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
        .expect_err("unlisted hosts should not receive browser session headers");

    assert_eq!(error.code, "PERMISSION_DENIED");

    state
        .enqueue_download_with_options(
            "https://files.chatgpt.com/file.zip".into(),
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
                handoff_auth: Some(auth),
                ..Default::default()
            },
        )
        .await
        .expect("allowlisted host should receive browser session headers");

    let raw_state = std::fs::read_to_string(download_dir.join("state.json")).unwrap();
    assert!(!raw_state.contains("session=abc"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn duplicate_enqueue_result_includes_existing_job_details() {
    let mut existing_job = download_job("job_9", JobState::Paused, ResumeSupport::Supported, 50);
    existing_job.url = "https://example.com/file.zip".into();
    existing_job.filename = "file.zip".into();

    let state = runtime_state_with_jobs(vec![existing_job]);

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
fn validate_settings_still_rejects_unavailable_download_directory() {
    let runtime_dir = test_runtime_dir("category-settings-blocked");
    let blocking_file = runtime_dir.join("blocked-parent");
    std::fs::write(&blocking_file, "not a directory").unwrap();
    let mut settings = Settings {
        download_directory: blocking_file.join("Downloads").display().to_string(),
        ..Settings::default()
    };

    let error =
        validate_settings(&mut settings).expect_err("settings save should reject invalid paths");

    assert!(error.contains("Could not create download directory"));
    let _ = std::fs::remove_dir_all(runtime_dir);
}

#[test]
fn validate_settings_creates_default_bulk_output_directory_and_preserves_custom_bulk_path() {
    let download_dir = test_runtime_dir("bulk-settings-default-output");
    let mut settings = Settings {
        download_directory: download_dir.display().to_string(),
        ..Settings::default()
    };
    settings.bulk.output_directory.clear();

    validate_settings(&mut settings).expect("settings should validate");

    assert_eq!(
        PathBuf::from(&settings.bulk.output_directory),
        download_dir.join("Bulk")
    );
    assert!(
        download_dir.join("Bulk").is_dir(),
        "default bulk output directory should be created during validation"
    );

    let custom_dir = test_runtime_dir("bulk-settings-custom-output").join("My Bulk");
    settings.bulk.output_directory = custom_dir.display().to_string();
    validate_settings(&mut settings).expect("settings with custom bulk output should validate");
    assert_eq!(PathBuf::from(&settings.bulk.output_directory), custom_dir);
    assert!(
        custom_dir.is_dir(),
        "custom bulk output directory should be created"
    );

    let _ = std::fs::remove_dir_all(download_dir);
    let _ = std::fs::remove_dir_all(custom_dir.parent().unwrap());
}

#[test]
fn validate_settings_defaults_and_creates_torrent_download_directory() {
    let download_dir = test_runtime_dir("torrent-directory-settings");
    let mut settings = Settings {
        download_directory: download_dir.display().to_string(),
        torrent: TorrentSettings {
            download_directory: String::new(),
            ..TorrentSettings::default()
        },
        ..Settings::default()
    };

    validate_settings(&mut settings).expect("settings should validate");

    let expected_torrent_dir = download_dir.join("Torrent");
    assert_eq!(
        settings.torrent.download_directory,
        expected_torrent_dir.display().to_string()
    );
    assert!(expected_torrent_dir.is_dir());

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
fn enqueue_torrent_uses_torrent_directory_without_category_folder() {
    let download_dir = test_runtime_dir("torrent-directory-routing");
    let torrent_dir = download_dir.join("Torrent");
    let mut state = runtime_state_with_jobs(Vec::new());
    state.settings.download_directory = download_dir.display().to_string();
    state.settings.torrent.download_directory = torrent_dir.display().to_string();

    let result = state
        .enqueue_download_in_memory(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Fedora",
            EnqueueOptions::default(),
        )
        .expect("torrent should enqueue into the torrent directory");
    let job = result
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == result.job_id)
        .expect("queued torrent job");

    assert_eq!(
        PathBuf::from(&job.target_path)
            .parent()
            .map(Path::to_path_buf),
        Some(torrent_dir.clone())
    );
    assert!(PathBuf::from(&job.temp_path).starts_with(torrent_dir.join(".torrent-state")));
    assert_ne!(target_parent_folder(&job.target_path), "Other");

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
            BrowserFallback::Replay,
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
fn prepare_torrent_download_prompt_uses_torrent_directory_without_category_folder() {
    let download_dir = test_runtime_dir("prompt-torrent-directory");
    let torrent_dir = download_dir.join("Torrent");
    let mut state = runtime_state_with_jobs(Vec::new());
    state.settings.download_directory = download_dir.display().to_string();
    state.settings.torrent.download_directory = torrent_dir.display().to_string();

    let prompt = state
        .prepare_download_prompt(
            "prompt_torrent",
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Fedora",
            None,
            None,
            None,
            BrowserFallback::Replay,
        )
        .expect("torrent prompt should use the torrent directory");

    assert_eq!(prompt.default_directory, torrent_dir.display().to_string());
    assert_eq!(
        PathBuf::from(&prompt.target_path)
            .parent()
            .map(Path::to_path_buf),
        Some(torrent_dir.clone())
    );
    assert_ne!(target_parent_folder(&prompt.target_path), "Other");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn prepare_download_prompt_marks_duplicate_target_job_for_same_filename() {
    let download_dir = test_runtime_dir("prompt-target-job-duplicate");
    let mut existing_job = download_job("job_13", JobState::Paused, ResumeSupport::Supported, 0);
    existing_job.url = "https://example.com/first-file.pdf".into();
    existing_job.filename = "guide.pdf".into();
    existing_job.target_path = download_dir
        .join("Document")
        .join("guide.pdf")
        .display()
        .to_string();
    existing_job.temp_path = format!("{}.part", existing_job.target_path);

    let mut state = runtime_state_with_jobs(vec![existing_job]);
    state.settings.download_directory = download_dir.display().to_string();

    let prompt = state
        .prepare_download_prompt(
            "prompt_target",
            "https://cdn.example.com/second-file",
            None,
            Some("guide.pdf".into()),
            None,
            BrowserFallback::Replay,
        )
        .expect("prompt should be prepared");

    assert_eq!(
        prompt.duplicate_job.as_ref().map(|job| job.id.as_str()),
        Some("job_13")
    );
    assert_eq!(
        prompt.duplicate_path.as_deref(),
        Some(
            download_dir
                .join("Document")
                .join("guide.pdf")
                .display()
                .to_string()
                .as_str()
        )
    );
    assert_eq!(prompt.duplicate_reason.as_deref(), Some("path"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn prepare_download_prompt_marks_duplicate_existing_target_file() {
    let download_dir = test_runtime_dir("prompt-target-file-duplicate");
    let target_path = download_dir.join("Document").join("guide.pdf");
    std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
    std::fs::write(&target_path, b"existing").unwrap();

    let mut state = runtime_state_with_jobs(Vec::new());
    state.settings.download_directory = download_dir.display().to_string();

    let prompt = state
        .prepare_download_prompt(
            "prompt_file",
            "https://cdn.example.com/generated",
            None,
            Some("guide.pdf".into()),
            None,
            BrowserFallback::Replay,
        )
        .expect("prompt should be prepared");

    assert!(prompt.duplicate_job.is_none());
    assert_eq!(
        prompt.duplicate_path.as_deref(),
        Some(target_path.display().to_string().as_str())
    );
    assert_eq!(prompt.duplicate_reason.as_deref(), Some("file"));

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

#[test]
fn normalize_job_populates_missing_created_at() {
    let mut job = download_job("job_26", JobState::Queued, ResumeSupport::Unknown, 0);
    job.created_at = 0;

    let normalized = normalize_job(job, &Settings::default());

    assert!(normalized.created_at > 0);
}

#[test]
fn normalize_job_marks_stale_bulk_finalization_failed() {
    let mut job = download_job(
        "job_stale_bulk",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    job.bulk_archive = Some(BulkArchiveInfo {
        id: "bulk_stale".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Extracting,
        requires_extraction: Some(true),
        output_path: Some("C:/Downloads/Game.zip".into()),
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    });

    let normalized = normalize_job(job, &Settings::default());
    let archive = normalized
        .bulk_archive
        .expect("bulk archive metadata should be preserved");

    assert_eq!(archive.id, "bulk_stale");
    assert_eq!(archive.archive_status, BulkArchiveStatus::Failed);
    assert_eq!(
        archive.output_path.as_deref(),
        Some("C:/Downloads/Game.zip")
    );
    assert!(archive
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("interrupted"));
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
fn enqueue_download_in_memory_accepts_torrent_filename_hint_for_browser_handoff() {
    let download_dir = test_runtime_dir("enqueue-torrent-filename-hint");
    let torrent_dir = download_dir.join("Torrent");
    let mut state = runtime_state_with_jobs(Vec::new());
    state.settings.download_directory = download_dir.display().to_string();
    state.settings.torrent.download_directory = torrent_dir.display().to_string();

    let result = state
        .enqueue_download_in_memory(
            "https://example.com/download?id=opaque",
            EnqueueOptions {
                source: Some(DownloadSource {
                    entry_point: "browser_download".into(),
                    browser: "firefox".into(),
                    extension_version: "0.3.52".into(),
                    page_url: None,
                    page_title: None,
                    referrer: None,
                    incognito: Some(false),
                }),
                filename_hint: Some("linux.iso.torrent".into()),
                transfer_kind: Some(TransferKind::Torrent),
                ..Default::default()
            },
        )
        .expect("browser handoff should accept explicit torrent metadata from filename");

    let job = result
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == result.job_id)
        .expect("queued torrent job");
    assert_eq!(job.transfer_kind, TransferKind::Torrent);
    assert_eq!(job.filename, "linux.iso.torrent");
    assert_eq!(
        PathBuf::from(&job.target_path)
            .parent()
            .map(Path::to_path_buf),
        Some(torrent_dir.clone())
    );
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

#[tokio::test]
async fn enqueue_download_entries_uses_filename_hints_for_resolved_hoster_links() {
    let download_dir = test_runtime_dir("enqueue-batch-filename-hint");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();

    let results = state
        .enqueue_download_entries(
            vec![BatchDownloadEntry {
                url: "https://dl.fuckingfast.co/dl/direct-token_123".into(),
                filename_hint: Some("archive.part01.rar".into()),
                resolved_from_url: Some(
                    "https://fuckingfast.co/ecw0lw398okf#archive.part01.rar".into(),
                ),
                hoster_preflight: None,
            }],
            None,
            None,
        )
        .await
        .expect("resolved hoster link should enqueue");

    assert_eq!(results[0].filename, "archive.part01.rar");
    assert_eq!(
        results[0].snapshot.jobs[0].url,
        "https://dl.fuckingfast.co/dl/direct-token_123"
    );
    assert_eq!(
        results[0].snapshot.jobs[0].resolved_from_url.as_deref(),
        Some("https://fuckingfast.co/ecw0lw398okf#archive.part01.rar")
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn enqueue_hoster_bulk_entry_keeps_source_url_and_marks_preflight_checking() {
    let download_dir = test_runtime_dir("enqueue-hoster-source-preflight");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();

    let source_url = "https://fuckingfast.co/ecw0lw398okf#archive.part01.rar";
    let results = state
        .enqueue_download_entries(
            vec![BatchDownloadEntry {
                url: source_url.into(),
                filename_hint: Some("archive.part01.rar".into()),
                resolved_from_url: Some(source_url.into()),
                hoster_preflight: Some(HosterPreflightInfo {
                    status: HosterPreflightStatus::Checking,
                    message: None,
                }),
            }],
            None,
            Some("Game".into()),
        )
        .await
        .expect("hoster source row should enqueue without a direct token");

    let job = &results[0].snapshot.jobs[0];
    assert_eq!(job.url, source_url);
    assert_eq!(job.resolved_from_url.as_deref(), Some(source_url));
    assert_eq!(
        job.hoster_preflight
            .as_ref()
            .map(|preflight| preflight.status),
        Some(HosterPreflightStatus::Checking)
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn enqueue_download_entries_can_start_bulk_batches_paused() {
    let download_dir = test_runtime_dir("enqueue-batch-start-paused");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();

    let results = state
        .enqueue_download_entries_with_options(
            vec![
                BatchDownloadEntry {
                    url: "https://example.com/Game.part01.rar".into(),
                    filename_hint: None,
                    resolved_from_url: None,
                    hoster_preflight: None,
                },
                BatchDownloadEntry {
                    url: "https://example.com/Game.part02.rar".into(),
                    filename_hint: None,
                    resolved_from_url: None,
                    hoster_preflight: None,
                },
            ],
            None,
            Some("Game.zip".into()),
            true,
        )
        .await
        .expect("bulk batch should enqueue paused");

    assert_eq!(results.len(), 2);
    assert!(results
        .last()
        .unwrap()
        .snapshot
        .jobs
        .iter()
        .all(|job| job.state == JobState::Paused));
    assert!(results.last().unwrap().snapshot.jobs.iter().all(|job| job
        .bulk_archive
        .as_ref()
        .map(|archive| archive.name.as_str())
        == Some("Game.zip")));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_enqueue_results_share_final_batch_snapshot() {
    let download_dir = test_runtime_dir("enqueue-batch-final-snapshot");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();

    let results = state
        .enqueue_download_entries_with_options(
            bulk_test_entries(),
            None,
            Some("Game.zip".into()),
            true,
        )
        .await
        .expect("bulk batch should enqueue paused");

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|result| result.snapshot.jobs.len() == 2));
    assert!(results
        .iter()
        .all(|result| result.snapshot.jobs.iter().all(|job| job
            .bulk_archive
            .as_ref()
            .is_some_and(|archive| archive.name == "Game.zip"))));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn enqueue_download_entries_defaults_omitted_bulk_output_kind_to_folder() {
    let download_dir = test_runtime_dir("enqueue-batch-default-folder-output-kind");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();

    let results = state
        .enqueue_download_entries_with_options(
            vec![
                BatchDownloadEntry {
                    url: "https://example.com/Game.part01.rar".into(),
                    filename_hint: None,
                    resolved_from_url: None,
                    hoster_preflight: None,
                },
                BatchDownloadEntry {
                    url: "https://example.com/Game.part02.rar".into(),
                    filename_hint: None,
                    resolved_from_url: None,
                    hoster_preflight: None,
                },
            ],
            None,
            Some("Game".into()),
            true,
        )
        .await
        .expect("bulk batch should enqueue with folder output by default");

    let snapshot = &results.last().unwrap().snapshot;
    assert!(snapshot.jobs.iter().all(|job| {
        job.bulk_archive
            .as_ref()
            .is_some_and(|archive| archive.output_kind == BulkArchiveOutputKind::Folder)
    }));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn enqueue_download_entries_stores_folder_bulk_output_kind() {
    let download_dir = test_runtime_dir("enqueue-batch-folder-output-kind");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();

    let results = state
        .enqueue_download_entries_with_bulk_options(
            vec![
                BatchDownloadEntry {
                    url: "https://example.com/Game.part01.rar".into(),
                    filename_hint: None,
                    resolved_from_url: None,
                    hoster_preflight: None,
                },
                BatchDownloadEntry {
                    url: "https://example.com/Game.part02.rar".into(),
                    filename_hint: None,
                    resolved_from_url: None,
                    hoster_preflight: None,
                },
            ],
            None,
            Some("Game".into()),
            true,
            BulkArchiveOutputKind::Folder,
        )
        .await
        .expect("folder bulk batch should enqueue");

    let snapshot = &results.last().unwrap().snapshot;
    assert!(snapshot.jobs.iter().all(|job| {
        job.bulk_archive.as_ref().is_some_and(|archive| {
            archive.output_kind == BulkArchiveOutputKind::Folder && archive.name == "Game"
        })
    }));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_archive_ready_uses_bulk_directory_for_default_folder_output() {
    let download_dir = test_runtime_dir("bulk-archive-ready-default-output");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();

    let results = state
        .enqueue_download_entries_with_options(
            bulk_test_entries(),
            None,
            Some("Game.zip".into()),
            true,
        )
        .await
        .expect("bulk folder batch should enqueue");
    let job_id = results[0].job_id.clone();
    complete_bulk_members_for_ready(&state).await;

    let ready = state
        .bulk_archive_ready_for_job(&job_id)
        .await
        .expect("bulk ready lookup should succeed")
        .expect("completed members should be ready");

    assert_eq!(ready.output_kind, BulkArchiveOutputKind::Folder);
    assert_eq!(
        ready.output_path,
        download_dir.join("Bulk").join("Game.zip")
    );
    assert_ne!(ready.output_path, download_dir.join("Game.zip"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_archive_ready_uses_bulk_directory_for_folder_output() {
    let download_dir = test_runtime_dir("bulk-folder-ready-output");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();

    let results = state
        .enqueue_download_entries_with_bulk_options(
            bulk_test_entries(),
            None,
            Some("Game".into()),
            true,
            BulkArchiveOutputKind::Folder,
        )
        .await
        .expect("bulk folder batch should enqueue");
    let job_id = results[0].job_id.clone();
    complete_bulk_members_for_ready(&state).await;

    let ready = state
        .bulk_archive_ready_for_job(&job_id)
        .await
        .expect("bulk ready lookup should succeed")
        .expect("completed members should be ready");

    assert_eq!(ready.output_path, download_dir.join("Bulk").join("Game"));
    assert_ne!(ready.output_path, download_dir.join("Game"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn enqueue_bulk_archive_freezes_output_path_and_rejects_reserved_duplicate() {
    let download_dir = test_runtime_dir("bulk-output-reservation");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);
    let mut settings = state.settings().await;
    settings.download_directory = download_dir.display().to_string();
    settings.bulk.output_directory = download_dir.join("Bulk").display().to_string();
    state.save_settings(settings).await.unwrap();

    let results = state
        .enqueue_download_entries_with_bulk_options(
            bulk_test_entries(),
            None,
            Some("Bundle".into()),
            true,
            BulkArchiveOutputKind::Folder,
        )
        .await
        .expect("initial bulk enqueue should reserve output");
    let reserved_output = download_dir.join("Bulk").join("Bundle");
    let archive = results
        .last()
        .unwrap()
        .snapshot
        .jobs
        .iter()
        .find_map(|job| job.bulk_archive.as_ref())
        .expect("bulk archive should be attached to members");

    assert_eq!(
        archive.output_path,
        Some(reserved_output.display().to_string())
    );
    assert_eq!(archive.archive_status, BulkArchiveStatus::Pending);

    let error = state
        .enqueue_download_entries_with_bulk_options(
            vec![
                BatchDownloadEntry {
                    url: "https://example.com/Other.part01.rar".into(),
                    filename_hint: None,
                    resolved_from_url: None,
                    hoster_preflight: None,
                },
                BatchDownloadEntry {
                    url: "https://example.com/Other.part02.rar".into(),
                    filename_hint: None,
                    resolved_from_url: None,
                    hoster_preflight: None,
                },
            ],
            None,
            Some("Bundle".into()),
            true,
            BulkArchiveOutputKind::Folder,
        )
        .await
        .expect_err("pending bulk output path should be reserved");

    assert_eq!(error.code, "DESTINATION_EXISTS");
    assert!(error.message.contains("already reserved"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn enqueue_download_in_memory_rechecks_bulk_output_reservation_under_write_lock() {
    let download_dir = test_runtime_dir("bulk-output-write-lock-reservation");
    let mut state = runtime_state_with_jobs(vec![]);
    state.settings.download_directory = download_dir.display().to_string();
    let output_path = download_dir.join("Bulk").join("Bundle");
    std::fs::create_dir_all(output_path.parent().unwrap()).unwrap();

    let archive = |id: &str| BulkArchiveInfo {
        id: id.into(),
        name: "Bundle".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        archive_status: BulkArchiveStatus::Pending,
        requires_extraction: None,
        output_path: Some(output_path.display().to_string()),
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };

    let shared_archive = archive("bulk_a");
    state
        .enqueue_download_in_memory(
            "https://example.com/a.bin",
            EnqueueOptions {
                transfer_kind: Some(TransferKind::Http),
                start_paused: true,
                bulk_archive: Some(shared_archive.clone()),
                ..Default::default()
            },
        )
        .expect("first archive member should enqueue");
    state
        .enqueue_download_in_memory(
            "https://example.com/b.bin",
            EnqueueOptions {
                transfer_kind: Some(TransferKind::Http),
                start_paused: true,
                bulk_archive: Some(shared_archive),
                ..Default::default()
            },
        )
        .expect("same archive id should share the reservation");

    let error = state
        .enqueue_download_in_memory(
            "https://example.com/c.bin",
            EnqueueOptions {
                transfer_kind: Some(TransferKind::Http),
                start_paused: true,
                bulk_archive: Some(archive("bulk_b")),
                ..Default::default()
            },
        )
        .expect_err("different archive id should not reuse reserved output");

    assert_eq!(error.code, "DESTINATION_EXISTS");
    assert!(error.message.contains("already reserved"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_archive_ready_uses_reserved_output_path_after_settings_change() {
    let download_dir = test_runtime_dir("bulk-ready-reserved-output");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);
    let mut settings = state.settings().await;
    settings.download_directory = download_dir.display().to_string();
    settings.bulk.output_directory = download_dir.join("Bulk").display().to_string();
    state.save_settings(settings).await.unwrap();

    let results = state
        .enqueue_download_entries_with_bulk_options(
            bulk_test_entries(),
            None,
            Some("Bundle".into()),
            true,
            BulkArchiveOutputKind::Folder,
        )
        .await
        .expect("bulk enqueue should succeed");
    let first_job_id = results[0].job_id.clone();
    let reserved_output = download_dir.join("Bulk").join("Bundle");

    let mut settings = state.settings().await;
    settings.bulk.output_directory = download_dir.join("OtherBulk").display().to_string();
    state.save_settings(settings).await.unwrap();
    complete_bulk_members_for_ready(&state).await;

    let ready = state
        .bulk_archive_ready_for_job(&first_job_id)
        .await
        .expect("ready check should succeed")
        .expect("completed members should claim archive");

    assert_eq!(ready.output_path, reserved_output);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn unique_archive_entry_name_treats_case_only_collisions_as_duplicates() {
    let mut used_names = HashSet::new();

    assert_eq!(
        unique_archive_entry_name("File.txt", &mut used_names),
        "File.txt"
    );
    assert_eq!(
        unique_archive_entry_name("file.txt", &mut used_names),
        "file (1).txt"
    );
}

#[tokio::test]
async fn bulk_folder_output_named_like_zip_stays_in_bulk_directory() {
    let download_dir = test_runtime_dir("bulk-folder-zip-name-output");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();

    let results = state
        .enqueue_download_entries_with_bulk_options(
            bulk_test_entries(),
            None,
            Some("Game.zip".into()),
            true,
            BulkArchiveOutputKind::Folder,
        )
        .await
        .expect("bulk folder batch should enqueue");
    let job_id = results[0].job_id.clone();
    complete_bulk_members_for_ready(&state).await;

    let ready = state
        .bulk_archive_ready_for_job(&job_id)
        .await
        .expect("bulk ready lookup should succeed")
        .expect("completed members should be ready");

    assert_eq!(
        ready.output_path,
        download_dir.join("Bulk").join("Game.zip")
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_enqueue_rejects_existing_categorized_output_path() {
    let download_dir = test_runtime_dir("bulk-enqueue-categorized-collision");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();
    std::fs::write(download_dir.join("Bulk").join("Game.zip"), b"existing").unwrap();

    let error = state
        .enqueue_download_entries_with_options(
            bulk_test_entries(),
            None,
            Some("Game.zip".into()),
            true,
        )
        .await
        .unwrap_err();

    assert_eq!(error.code, "DESTINATION_EXISTS");
    assert!(error.message.contains("Bulk"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_enqueue_ignores_legacy_root_output_collision() {
    let download_dir = test_runtime_dir("bulk-enqueue-root-collision");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();
    std::fs::write(download_dir.join("Game.zip"), b"legacy").unwrap();

    let results = state
        .enqueue_download_entries_with_options(
            bulk_test_entries(),
            None,
            Some("Game.zip".into()),
            true,
        )
        .await
        .expect("root legacy output should not block categorized output");

    assert_eq!(results.len(), 2);

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
        custom_trackers: vec![
            " https://tracker.example/announce ".into(),
            "HTTPS://tracker.example/announce".into(),
            "udp://tracker.example:1337/announce".into(),
            "ftp://tracker.example/announce".into(),
        ],
        ..TorrentSettings::default()
    };

    normalize_torrent_settings(&mut settings);

    assert_eq!(settings.seed_ratio_limit, 0.1);
    assert_eq!(settings.seed_time_limit_minutes, 1);
    assert_eq!(settings.upload_limit_kib_per_second, 1_048_576);
    assert!(settings.port_forwarding_enabled);
    assert_eq!(settings.port_forwarding_port, 42000);
    assert_eq!(
        settings.custom_trackers,
        vec![
            "https://tracker.example/announce".to_string(),
            "udp://tracker.example:1337/announce".to_string()
        ]
    );
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
async fn bulk_archive_ready_for_retry_reuses_failed_completed_members() {
    let download_dir = test_runtime_dir("retry-failed-bulk-archive-ready");
    let part_1 = download_dir.join("Game.part01.rar");
    let part_2 = download_dir.join("Game.part02.rar");
    std::fs::write(&part_1, b"first").unwrap();
    std::fs::write(&part_2, b"second").unwrap();

    let bulk_archive = BulkArchiveInfo {
        id: "bulk_retry".into(),
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
    job_1.filename = "Game.part01.rar".into();
    job_1.target_path = part_1.display().to_string();
    job_1.temp_path = part_1.with_extension("rar.part").display().to_string();
    job_1.bulk_archive = Some(bulk_archive.clone());
    let mut job_2 = download_job("job_2", JobState::Completed, ResumeSupport::Supported, 100);
    job_2.filename = "Game.part02.rar".into();
    job_2.target_path = part_2.display().to_string();
    job_2.temp_path = part_2.with_extension("rar.part").display().to_string();
    job_2.bulk_archive = Some(bulk_archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job_1, job_2]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.download_directory = download_dir.display().to_string();
    }

    let ready = state
        .bulk_archive_ready_for_retry("bulk_retry")
        .await
        .expect("failed archive should be retryable");

    assert_eq!(ready.archive_id, "bulk_retry");
    assert_eq!(ready.output_kind, BulkArchiveOutputKind::Folder);
    assert_eq!(
        ready.output_path,
        download_dir.join("Bulk").join("Game.zip")
    );
    assert_eq!(ready.entries.len(), 2);
    assert_eq!(ready.entries[0].source_path, part_1);
    assert_eq!(ready.entries[1].source_path, part_2);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_archive_ready_for_job_claims_pending_archive_once() {
    let download_dir = test_runtime_dir("bulk-archive-ready-claim-once");
    let part_1 = download_dir.join("Game.part01.rar");
    let part_2 = download_dir.join("Game.part02.rar");
    std::fs::write(&part_1, b"first").unwrap();
    std::fs::write(&part_2, b"second").unwrap();

    let bulk_archive = BulkArchiveInfo {
        id: "bulk_claim".into(),
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
    let mut job_1 = download_job("job_1", JobState::Completed, ResumeSupport::Supported, 100);
    job_1.filename = "Game.part01.rar".into();
    job_1.target_path = part_1.display().to_string();
    job_1.bulk_archive = Some(bulk_archive.clone());
    let mut job_2 = download_job("job_2", JobState::Completed, ResumeSupport::Supported, 100);
    job_2.filename = "Game.part02.rar".into();
    job_2.target_path = part_2.display().to_string();
    job_2.bulk_archive = Some(bulk_archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job_1, job_2]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.download_directory = download_dir.display().to_string();
    }

    let ready = state
        .bulk_archive_ready_for_job("job_1")
        .await
        .expect("pending archive readiness should be checked")
        .expect("completed pending members should be claimed for finalization");
    let second = state
        .bulk_archive_ready_for_job("job_2")
        .await
        .expect("second readiness check should not fail");
    let claimed_status = {
        let runtime = state.inner.read().await;
        runtime.jobs[0]
            .bulk_archive
            .as_ref()
            .expect("job should keep bulk archive metadata")
            .archive_status
    };

    assert_eq!(ready.archive_id, "bulk_claim");
    assert!(
        second.is_none(),
        "a claimed archive must not be claimed twice"
    );
    assert_eq!(claimed_status, BulkArchiveStatus::CreatingFolder);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_archive_ready_for_retry_claims_failed_archive_once() {
    let download_dir = test_runtime_dir("bulk-archive-retry-claim-once");
    let part_1 = download_dir.join("Game.part01.rar");
    let part_2 = download_dir.join("Game.part02.rar");
    std::fs::write(&part_1, b"first").unwrap();
    std::fs::write(&part_2, b"second").unwrap();

    let bulk_archive = BulkArchiveInfo {
        id: "bulk_retry_claim".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Failed,
        requires_extraction: None,
        output_path: Some(download_dir.join("Game.zip").display().to_string()),
        error: Some("missing".into()),
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let mut job_1 = download_job("job_1", JobState::Completed, ResumeSupport::Supported, 100);
    job_1.filename = "Game.part01.rar".into();
    job_1.target_path = part_1.display().to_string();
    job_1.bulk_archive = Some(bulk_archive.clone());
    let mut job_2 = download_job("job_2", JobState::Completed, ResumeSupport::Supported, 100);
    job_2.filename = "Game.part02.rar".into();
    job_2.target_path = part_2.display().to_string();
    job_2.bulk_archive = Some(bulk_archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job_1, job_2]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.download_directory = download_dir.display().to_string();
    }

    let ready = state
        .bulk_archive_ready_for_retry("bulk_retry_claim")
        .await
        .expect("failed archive should be claimed for retry");
    let error = state
        .bulk_archive_ready_for_retry("bulk_retry_claim")
        .await
        .expect_err("second retry claim should reject a running finalization");
    let claimed_status = {
        let runtime = state.inner.read().await;
        runtime.jobs[0]
            .bulk_archive
            .as_ref()
            .expect("job should keep bulk archive metadata")
            .archive_status
    };

    assert_eq!(ready.archive_id, "bulk_retry_claim");
    assert!(error.contains("already running"));
    assert_eq!(claimed_status, BulkArchiveStatus::CreatingFolder);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_archive_ready_for_retry_without_output_path_uses_bulk_directory() {
    let download_dir = test_runtime_dir("retry-failed-bulk-archive-category-fallback");
    let part_1 = download_dir.join("Game.part01.rar");
    let part_2 = download_dir.join("Game.part02.rar");
    std::fs::write(&part_1, b"first").unwrap();
    std::fs::write(&part_2, b"second").unwrap();

    let bulk_archive = BulkArchiveInfo {
        id: "bulk_retry_fallback".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Failed,
        requires_extraction: None,
        output_path: None,
        error: Some("locked".into()),
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let mut job_1 = download_job("job_1", JobState::Completed, ResumeSupport::Supported, 100);
    job_1.filename = "Game.part01.rar".into();
    job_1.target_path = part_1.display().to_string();
    job_1.temp_path = part_1.with_extension("rar.part").display().to_string();
    job_1.bulk_archive = Some(bulk_archive.clone());
    let mut job_2 = download_job("job_2", JobState::Completed, ResumeSupport::Supported, 100);
    job_2.filename = "Game.part02.rar".into();
    job_2.target_path = part_2.display().to_string();
    job_2.temp_path = part_2.with_extension("rar.part").display().to_string();
    job_2.bulk_archive = Some(bulk_archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job_1, job_2]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.download_directory = download_dir.display().to_string();
    }

    let ready = state
        .bulk_archive_ready_for_retry("bulk_retry_fallback")
        .await
        .expect("failed archive should be retryable");

    assert_eq!(ready.output_kind, BulkArchiveOutputKind::Folder);
    assert_eq!(
        ready.output_path,
        download_dir.join("Bulk").join("Game.zip")
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_archive_ready_for_retry_preserves_nonlegacy_stored_output_path() {
    let download_dir = test_runtime_dir("retry-failed-bulk-archive-custom-output");
    let part_1 = download_dir.join("Game.part01.rar");
    let part_2 = download_dir.join("Game.part02.rar");
    let custom_output = download_dir.join("Custom").join("Game.zip");
    std::fs::write(&part_1, b"first").unwrap();
    std::fs::write(&part_2, b"second").unwrap();

    let bulk_archive = BulkArchiveInfo {
        id: "bulk_retry_custom".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Failed,
        requires_extraction: None,
        output_path: Some(custom_output.display().to_string()),
        error: Some("locked".into()),
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let mut job_1 = download_job("job_1", JobState::Completed, ResumeSupport::Supported, 100);
    job_1.filename = "Game.part01.rar".into();
    job_1.target_path = part_1.display().to_string();
    job_1.temp_path = part_1.with_extension("rar.part").display().to_string();
    job_1.bulk_archive = Some(bulk_archive.clone());
    let mut job_2 = download_job("job_2", JobState::Completed, ResumeSupport::Supported, 100);
    job_2.filename = "Game.part02.rar".into();
    job_2.target_path = part_2.display().to_string();
    job_2.temp_path = part_2.with_extension("rar.part").display().to_string();
    job_2.bulk_archive = Some(bulk_archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job_1, job_2]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.download_directory = download_dir.display().to_string();
    }

    let ready = state
        .bulk_archive_ready_for_retry("bulk_retry_custom")
        .await
        .expect("failed archive should be retryable");

    assert_eq!(ready.output_path, custom_output);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_archive_ready_for_retry_rejects_missing_parts() {
    let download_dir = test_runtime_dir("retry-failed-bulk-archive-missing-part");
    let part_1 = download_dir.join("Game.part01.rar");
    let part_2 = download_dir.join("Game.part02.rar");
    std::fs::write(&part_1, b"first").unwrap();

    let bulk_archive = BulkArchiveInfo {
        id: "bulk_retry_missing".into(),
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
    job_1.filename = "Game.part01.rar".into();
    job_1.target_path = part_1.display().to_string();
    job_1.temp_path = part_1.with_extension("rar.part").display().to_string();
    job_1.bulk_archive = Some(bulk_archive.clone());
    let mut job_2 = download_job("job_2", JobState::Completed, ResumeSupport::Supported, 100);
    job_2.filename = "Game.part02.rar".into();
    job_2.target_path = part_2.display().to_string();
    job_2.temp_path = part_2.with_extension("rar.part").display().to_string();
    job_2.bulk_archive = Some(bulk_archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job_1, job_2]);

    let error = state
        .bulk_archive_ready_for_retry("bulk_retry_missing")
        .await
        .expect_err("missing downloaded part should block archive retry");

    assert!(error.contains("Game.part02.rar"));
    assert!(error.contains("missing"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_archive_ready_for_retry_rejects_unknown_incomplete_running_and_completed_archives() {
    let download_dir = test_runtime_dir("retry-failed-bulk-archive-invalid-states");
    let part_1 = download_dir.join("Game.part01.rar");
    let part_2 = download_dir.join("Game.part02.rar");
    std::fs::write(&part_1, b"first").unwrap();
    std::fs::write(&part_2, b"second").unwrap();

    let archive = |id: &str, status: BulkArchiveStatus| BulkArchiveInfo {
        id: id.into(),
        name: format!("{id}.zip"),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: status,
        requires_extraction: None,
        output_path: Some(download_dir.join(format!("{id}.zip")).display().to_string()),
        error: if status == BulkArchiveStatus::Failed {
            Some("locked".into())
        } else {
            None
        },
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };

    let mut pending_job = download_job(
        "job_pending",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    pending_job.target_path = part_1.display().to_string();
    pending_job.bulk_archive = Some(archive("bulk_pending", BulkArchiveStatus::Pending));

    let mut running_job = download_job(
        "job_running",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    running_job.target_path = part_1.display().to_string();
    running_job.bulk_archive = Some(archive("bulk_running", BulkArchiveStatus::Extracting));

    let mut completed_job = download_job(
        "job_completed",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    completed_job.target_path = part_1.display().to_string();
    completed_job.bulk_archive = Some(archive("bulk_completed", BulkArchiveStatus::Completed));

    let failed_archive = archive("bulk_incomplete", BulkArchiveStatus::Failed);
    let mut incomplete_job_1 = download_job(
        "job_incomplete_1",
        JobState::Completed,
        ResumeSupport::Supported,
        100,
    );
    incomplete_job_1.target_path = part_1.display().to_string();
    incomplete_job_1.bulk_archive = Some(failed_archive.clone());
    let mut incomplete_job_2 = download_job(
        "job_incomplete_2",
        JobState::Paused,
        ResumeSupport::Supported,
        0,
    );
    incomplete_job_2.target_path = part_2.display().to_string();
    incomplete_job_2.bulk_archive = Some(failed_archive);

    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![
            pending_job,
            running_job,
            completed_job,
            incomplete_job_1,
            incomplete_job_2,
        ],
    );

    assert!(state
        .bulk_archive_ready_for_retry("bulk_unknown")
        .await
        .unwrap_err()
        .contains("not found"));
    assert!(state
        .bulk_archive_ready_for_retry("bulk_pending")
        .await
        .unwrap_err()
        .contains("not ready"));
    assert!(state
        .bulk_archive_ready_for_retry("bulk_running")
        .await
        .unwrap_err()
        .contains("already running"));
    assert!(state
        .bulk_archive_ready_for_retry("bulk_completed")
        .await
        .unwrap_err()
        .contains("already completed"));
    assert!(state
        .bulk_archive_ready_for_retry("bulk_incomplete")
        .await
        .unwrap_err()
        .contains("every member"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn normalize_job_converts_stale_removing_to_cleanup_failed() {
    let mut job = download_job(
        "job_removing",
        JobState::Canceled,
        ResumeSupport::Unknown,
        0,
    );
    job.removal_state = Some(RemovalState::Removing);

    let normalized = normalize_job(job, &Settings::default());

    assert_eq!(normalized.removal_state, Some(RemovalState::CleanupFailed));
    assert_eq!(normalized.failure_category, Some(FailureCategory::Disk));
    assert!(normalized
        .error
        .as_deref()
        .is_some_and(|message| message.contains("cleanup was interrupted")));
}
