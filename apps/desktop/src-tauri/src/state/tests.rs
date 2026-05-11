use super::*;
use crate::storage::{
    BulkDownloadSettings, HostRegistrationStatus, HosterPreflightInfo, HosterPreflightStatus,
    TorrentPeerConnectionWatchdogMode, TorrentRuntimeDiagnostics,
};

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
async fn claim_schedulable_jobs_preserves_persisted_retry_attempts() {
    let download_dir = test_runtime_dir("claim-preserves-retry-attempts");
    let mut job = download_job(
        "job_retry_budget",
        JobState::Queued,
        ResumeSupport::Supported,
        0,
    );
    job.retry_attempts = 2;
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].retry_attempts, 2);

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

#[tokio::test]
async fn resolve_completed_bulk_archive_path_returns_output_file() {
    let download_dir = test_runtime_dir("resolve-completed-bulk-archive");
    let archive_path = download_dir.join("bulk-download.zip");
    std::fs::write(&archive_path, b"archive").unwrap();
    let archive = BulkArchiveInfo {
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
    };
    let mut job = download_job("job_24", JobState::Completed, ResumeSupport::Supported, 100);
    job.bulk_archive = Some(archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let openable = state
        .resolve_bulk_archive_openable_path("bulk_1")
        .await
        .unwrap();
    let revealable = state
        .resolve_bulk_archive_revealable_path("bulk_1")
        .await
        .unwrap();

    assert_eq!(openable, archive_path);
    assert_eq!(revealable, archive_path);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn resolve_completed_bulk_folder_path_returns_output_directory() {
    let download_dir = test_runtime_dir("resolve-completed-bulk-folder");
    let folder_path = download_dir.join("Game");
    std::fs::create_dir_all(&folder_path).unwrap();
    let archive = BulkArchiveInfo {
        id: "bulk_folder".into(),
        name: "Game".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        archive_status: BulkArchiveStatus::Completed,
        requires_extraction: None,
        output_path: Some(folder_path.display().to_string()),
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let mut job = download_job("job_31", JobState::Completed, ResumeSupport::Supported, 100);
    job.bulk_archive = Some(archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let openable = state
        .resolve_bulk_archive_openable_path("bulk_folder")
        .await
        .unwrap();
    let revealable = state
        .resolve_bulk_archive_revealable_path("bulk_folder")
        .await
        .unwrap();

    assert_eq!(openable, folder_path);
    assert_eq!(revealable, folder_path);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn resolve_bulk_archive_path_rejects_incomplete_failed_missing_and_unknown_archives() {
    let download_dir = test_runtime_dir("resolve-invalid-bulk-archive");
    let completed_missing_path = download_dir.join("missing.zip");
    let mut pending_job =
        download_job("job_25", JobState::Completed, ResumeSupport::Supported, 100);
    pending_job.bulk_archive = Some(BulkArchiveInfo {
        id: "bulk_pending".into(),
        name: "pending.zip".into(),
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
    let mut failed_job = download_job("job_26", JobState::Completed, ResumeSupport::Supported, 100);
    failed_job.bulk_archive = Some(BulkArchiveInfo {
        id: "bulk_failed".into(),
        name: "failed.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Failed,
        requires_extraction: None,
        output_path: Some(download_dir.join("failed.zip").display().to_string()),
        error: Some("zip failed".into()),
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    });
    let mut missing_job =
        download_job("job_30", JobState::Completed, ResumeSupport::Supported, 100);
    missing_job.bulk_archive = Some(BulkArchiveInfo {
        id: "bulk_missing".into(),
        name: "missing.zip".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        archive_status: BulkArchiveStatus::Completed,
        requires_extraction: None,
        output_path: Some(completed_missing_path.display().to_string()),
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    });
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![pending_job, failed_job, missing_job],
    );

    let pending = state
        .resolve_bulk_archive_openable_path("bulk_pending")
        .await
        .unwrap_err();
    let failed = state
        .resolve_bulk_archive_revealable_path("bulk_failed")
        .await
        .unwrap_err();
    let missing = state
        .resolve_bulk_archive_openable_path("bulk_missing")
        .await
        .unwrap_err();
    let unknown = state
        .resolve_bulk_archive_revealable_path("bulk_unknown")
        .await
        .unwrap_err();

    assert!(pending.message.contains("is not ready yet"));
    assert!(failed.message.contains("failed"));
    assert!(missing.message.contains("not available on disk"));
    assert!(unknown.message.contains("Bulk archive not found"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn snapshot_leaves_completed_artifact_existence_unprobed() {
    let jobs = (0..1_000)
        .map(|index| {
            let mut job = download_job(
                &format!("job_{index}"),
                JobState::Completed,
                ResumeSupport::Supported,
                100,
            );
            job.target_path = format!("Z:/definitely-missing/snapshot-scan-{index}.zip");
            job
        })
        .collect::<Vec<_>>();

    let state = runtime_state_with_jobs(jobs);
    let snapshot = state.snapshot();

    assert!(
        snapshot
            .jobs
            .iter()
            .all(|job| job.artifact_exists.is_none()),
        "full snapshots should not issue filesystem existence probes for completed artifacts"
    );
}

#[tokio::test]
async fn update_job_progress_coalesces_persistence() {
    let download_dir = test_runtime_dir("progress-persist-coalesce");
    let storage_path = download_dir.join("state.json");
    let mut first = download_job("job_1", JobState::Downloading, ResumeSupport::Supported, 0);
    first.total_bytes = 100;
    let mut second = download_job("job_2", JobState::Downloading, ResumeSupport::Supported, 0);
    second.total_bytes = 100;
    let state = shared_state_with_jobs(storage_path.clone(), vec![first, second]);

    state
        .update_job_progress("job_1", 10, Some(100), 1, true)
        .await
        .expect("first progress update should persist");
    state
        .update_job_progress("job_2", 25, Some(100), 1, true)
        .await
        .expect("second progress update should update memory");

    let snapshot = state.snapshot().await;
    assert_eq!(snapshot.jobs[1].downloaded_bytes, 25);

    let persisted = load_persisted_state(&storage_path).expect("persisted state should load");
    assert_eq!(persisted.jobs[0].downloaded_bytes, 10);
    assert_eq!(
        persisted.jobs[1].downloaded_bytes, 0,
        "nearby active progress updates should be coalesced instead of writing full state each time"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn mark_job_downloading_preserves_pause_requested_during_startup() {
    let download_dir = test_runtime_dir("mark-downloading-preserves-paused");
    let storage_path = download_dir.join("state.json");
    let mut job = download_job("job_pause", JobState::Paused, ResumeSupport::Unknown, 0);
    job.total_bytes = 0;
    job.speed = 512;
    job.eta = 60;
    let state = shared_state_with_jobs(storage_path, vec![job]);

    let snapshot = state
        .mark_job_downloading(
            "job_pause",
            256,
            Some(1024),
            ResumeSupport::Supported,
            Some("renamed.zip".into()),
        )
        .await
        .expect("late startup metadata should be accepted");
    let paused = &snapshot.jobs[0];

    assert_eq!(paused.state, JobState::Paused);
    assert_eq!(paused.downloaded_bytes, 256);
    assert_eq!(paused.total_bytes, 1024);
    assert_eq!(paused.progress, 25.0);
    assert_eq!(paused.resume_support, ResumeSupport::Supported);
    assert_eq!(paused.filename, "renamed.zip");
    assert_eq!(paused.speed, 0);
    assert_eq!(paused.eta, 0);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn update_job_progress_preserves_paused_state_after_pause() {
    let download_dir = test_runtime_dir("progress-preserves-paused");
    let storage_path = download_dir.join("state.json");
    let mut job = download_job("job_pause", JobState::Paused, ResumeSupport::Supported, 100);
    job.total_bytes = 1000;
    job.speed = 512;
    job.eta = 60;
    let state = shared_state_with_jobs(storage_path, vec![job]);

    let snapshot = state
        .update_job_progress("job_pause", 400, Some(1000), 128, true)
        .await
        .expect("late progress should be accepted");
    let paused = &snapshot.jobs[0];

    assert_eq!(paused.state, JobState::Paused);
    assert_eq!(paused.downloaded_bytes, 400);
    assert_eq!(paused.progress, 40.0);
    assert_eq!(paused.speed, 0);
    assert_eq!(paused.eta, 0);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn update_job_progress_does_not_revive_canceled_jobs() {
    let download_dir = test_runtime_dir("progress-preserves-canceled");
    let storage_path = download_dir.join("state.json");
    let mut job = download_job(
        "job_cancel",
        JobState::Canceled,
        ResumeSupport::Supported,
        100,
    );
    job.total_bytes = 1000;
    job.speed = 512;
    job.eta = 60;
    let state = shared_state_with_jobs(storage_path, vec![job]);

    let snapshot = state
        .update_job_progress("job_cancel", 400, Some(1000), 128, true)
        .await
        .expect("late progress should be accepted");
    let canceled = &snapshot.jobs[0];

    assert_eq!(canceled.state, JobState::Canceled);
    assert_eq!(canceled.downloaded_bytes, 400);
    assert_eq!(canceled.progress, 40.0);
    assert_eq!(canceled.speed, 0);
    assert_eq!(canceled.eta, 0);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn runtime_state_tracks_job_indexes_after_insert_and_remove() {
    let mut state = runtime_state_with_jobs(vec![
        download_job("job_1", JobState::Queued, ResumeSupport::Unknown, 0),
        download_job("job_2", JobState::Queued, ResumeSupport::Unknown, 0),
    ]);

    assert_eq!(state.job_index("job_1"), Some(0));
    assert_eq!(state.job_index("job_2"), Some(1));

    let removed = state.remove_job_at_index(0);
    assert_eq!(removed.id, "job_1");
    assert_eq!(state.job_index("job_1"), None);
    assert_eq!(state.job_index("job_2"), Some(0));
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
async fn duplicate_reduced_bulk_batch_does_not_create_single_member_archive() {
    let download_dir = test_runtime_dir("enqueue-batch-duplicate-reduced");
    let mut existing = download_job("job_existing", JobState::Queued, ResumeSupport::Unknown, 0);
    existing.url = "https://example.com/Game.part01.rar".into();
    existing.filename = "Game.part01.rar".into();
    existing.target_path = download_dir
        .join("Compressed")
        .join("Game.part01.rar")
        .display()
        .to_string();
    existing.temp_path = format!("{}.part", existing.target_path);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![existing]);
    state
        .save_settings(Settings {
            download_directory: download_dir.display().to_string(),
            ..Settings::default()
        })
        .await
        .unwrap();
    std::fs::create_dir_all(download_dir.join("Bulk").join("Game.zip"))
        .expect("pre-existing bulk output path should not matter for one new member");

    let results = state
        .enqueue_download_entries_with_options(
            bulk_test_entries(),
            None,
            Some("Game.zip".into()),
            true,
        )
        .await
        .expect("duplicate-reduced bulk batch should enqueue the new member");

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].status, EnqueueStatus::DuplicateExistingJob);
    assert_eq!(results[1].status, EnqueueStatus::Queued);
    let snapshot = &results[1].snapshot;
    let new_job = snapshot
        .jobs
        .iter()
        .find(|job| job.id == results[1].job_id)
        .expect("queued member should be in the final snapshot");
    assert!(
        new_job.bulk_archive.is_none(),
        "one newly queued member should not reserve a bulk archive"
    );

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
            BulkMemberAutoRestartMode::PreservePartial,
        ),
        (
            FailureCategory::Server,
            "Download request failed with HTTP 503 Service Unavailable.",
            true,
            BulkMemberAutoRestartMode::PreservePartial,
        ),
        (
            FailureCategory::Http,
            "Download request failed with HTTP 403 Forbidden.",
            false,
            BulkMemberAutoRestartMode::PreservePartial,
        ),
        (
            FailureCategory::Resume,
            "The remote server rejected the resume request.",
            false,
            BulkMemberAutoRestartMode::ResetPartial,
        ),
    ] {
        let candidate = state
            .bulk_member_auto_restart_candidate("job_auto", category, message, retryable)
            .await
            .expect("candidate lookup should succeed")
            .expect("transient pending HTTP bulk member should be eligible");
        assert_eq!(candidate.attempt, 1);
        assert_eq!(candidate.max_attempts, 5);
        assert_eq!(candidate.mode, expected_mode);
        assert_eq!(
            candidate.resolved_from_url.as_deref(),
            Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar")
        );
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
async fn bulk_member_auto_restart_candidate_resets_after_preserved_attempt() {
    let download_dir = test_runtime_dir("bulk-auto-restart-reset-after-preserve");
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
    assert_eq!(candidate.mode, BulkMemberAutoRestartMode::ResetPartial);

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
async fn retry_bulk_member_resets_partial_state_and_preserves_bulk_identity() {
    let download_dir = test_runtime_dir("bulk-member-manual-retry-reset");
    let target_path = download_dir.join("Game.part01.rar");
    let temp_path = download_dir.join("Game.part01.rar.part");
    std::fs::write(&temp_path, b"partial").unwrap();

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
    job.resolved_from_url = Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar".into());
    job.bulk_archive = Some(bulk_archive.clone());

    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    let snapshot = state
        .retry_bulk_member(
            "job_manual_retry",
            "https://dl.fuckingfast.co/dl/new-token".into(),
        )
        .await
        .expect("manual bulk member retry should reset and queue the member");

    let retried = snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_manual_retry")
        .expect("job should remain in queue");
    assert_eq!(retried.state, JobState::Queued);
    assert_eq!(retried.url, "https://dl.fuckingfast.co/dl/new-token");
    assert_eq!(retried.filename, "Game.part01.rar");
    assert_eq!(retried.target_path, target_path.display().to_string());
    assert_eq!(retried.progress, 0.0);
    assert_eq!(retried.total_bytes, 0);
    assert_eq!(retried.downloaded_bytes, 0);
    assert_eq!(retried.error, None);
    assert_eq!(retried.failure_category, None);
    assert_eq!(retried.resume_support, ResumeSupport::Unknown);
    assert_eq!(retried.retry_attempts, 0);
    assert_eq!(retried.auto_restart_attempts, 0);
    assert_eq!(
        retried.resolved_from_url.as_deref(),
        Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar")
    );
    assert_eq!(retried.bulk_archive.as_ref(), Some(&bulk_archive));
    assert!(!temp_path.exists());

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
async fn bulk_scheduler_cap_skips_bulk_members_but_keeps_normal_downloads_flowing() {
    let download_dir = test_runtime_dir("bulk-scheduler-cap");
    let archive = BulkArchiveInfo {
        id: "bulk_scheduler".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        archive_status: BulkArchiveStatus::Pending,
        requires_extraction: None,
        output_path: Some(download_dir.join("Bulk").join("Game").display().to_string()),
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let mut active_bulk = download_job(
        "job_bulk_active",
        JobState::Downloading,
        ResumeSupport::Supported,
        25,
    );
    active_bulk.bulk_archive = Some(archive.clone());
    let mut queued_bulk = download_job(
        "job_bulk_queued",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    queued_bulk.bulk_archive = Some(archive);
    let normal_queued = download_job(
        "job_normal_queued",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );

    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active_bulk, queued_bulk, normal_queued],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 2;
        runtime.settings.bulk.max_concurrent_downloads = 1;
        runtime.active_workers.insert("job_bulk_active".into());
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "job_normal_queued");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_workers_do_not_consume_normal_download_slots() {
    let download_dir = test_runtime_dir("bulk-does-not-consume-normal-slots");
    let archive = bulk_archive_info(&download_dir, "bulk_slot_split");
    let mut active_bulk = download_job(
        "job_bulk_active",
        JobState::Downloading,
        ResumeSupport::Supported,
        25,
    );
    active_bulk.bulk_archive = Some(archive);
    let normal_queued = download_job(
        "job_normal_queued",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active_bulk, normal_queued],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.settings.bulk.max_concurrent_downloads = 1;
        runtime.active_workers.insert("job_bulk_active".into());
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "job_normal_queued");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn normal_workers_do_not_consume_bulk_download_slots() {
    let download_dir = test_runtime_dir("normal-does-not-consume-bulk-slots");
    let archive = bulk_archive_info(&download_dir, "bulk_slot_split");
    let active_normal = download_job(
        "job_normal_active",
        JobState::Downloading,
        ResumeSupport::Supported,
        25,
    );
    let mut queued_bulk = download_job(
        "job_bulk_queued",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    queued_bulk.bulk_archive = Some(archive);
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active_normal, queued_bulk],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.settings.bulk.max_concurrent_downloads = 1;
        runtime.active_workers.insert("job_normal_active".into());
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "job_bulk_queued");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn scheduler_claims_normal_and_bulk_jobs_in_same_pass() {
    let download_dir = test_runtime_dir("normal-and-bulk-same-pass");
    let archive = bulk_archive_info(&download_dir, "bulk_slot_split");
    let normal_queued = download_job(
        "job_normal_queued",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    let mut bulk_queued = download_job(
        "job_bulk_queued",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    bulk_queued.bulk_archive = Some(archive);
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![normal_queued, bulk_queued],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.settings.bulk.max_concurrent_downloads = 1;
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");
    let task_ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(task_ids, vec!["job_normal_queued", "job_bulk_queued"]);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn bulk_runtime_tuning_uses_bulk_settings_without_changing_normal_downloads() {
    let download_dir = test_runtime_dir("bulk-runtime-tuning");
    let state = shared_state_with_jobs(download_dir.join("state.json"), Vec::new());
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.speed_limit_kib_per_second = 128;
        runtime.settings.download_performance_mode = DownloadPerformanceMode::Stable;
        runtime.settings.bulk.speed_limit_kib_per_second = 512;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Fast;
    }

    assert_eq!(
        state.speed_limit_bytes_per_second_for_task(false).await,
        Some(128 * 1024)
    );
    assert_eq!(
        state.speed_limit_bytes_per_second_for_task(true).await,
        Some(512 * 1024)
    );
    assert_eq!(
        state.download_performance_mode_for_task(false).await,
        DownloadPerformanceMode::Stable
    );
    assert_eq!(
        state.download_performance_mode_for_task(true).await,
        DownloadPerformanceMode::Fast
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn protected_bulk_hoster_claim_holds_back_next_hoster_but_allows_direct_bulk() {
    let download_dir = test_runtime_dir("bulk-hoster-fairness-startup");
    let archive = BulkArchiveInfo {
        id: "bulk_hoster_fairness".into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        archive_status: BulkArchiveStatus::Pending,
        requires_extraction: None,
        output_path: Some(download_dir.join("Bulk").join("Game").display().to_string()),
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    };
    let mut first_hoster = download_job(
        "job_hoster_first",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    first_hoster.url = "https://fuckingfast.co/first".into();
    first_hoster.resolved_from_url = Some("https://fuckingfast.co/first".into());
    first_hoster.bulk_archive = Some(archive.clone());
    let mut second_hoster = download_job(
        "job_hoster_second",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    second_hoster.url = "https://fuckingfast.co/second".into();
    second_hoster.resolved_from_url = Some("https://fuckingfast.co/second".into());
    second_hoster.bulk_archive = Some(archive.clone());
    let mut direct_bulk = download_job(
        "job_direct_bulk",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    direct_bulk.url = "https://cdn.example.com/direct.part03.rar".into();
    direct_bulk.bulk_archive = Some(archive);

    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![first_hoster, second_hoster, direct_bulk],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 3;
        runtime.settings.bulk.max_concurrent_downloads = 3;
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    let task_ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(task_ids, vec!["job_hoster_first", "job_direct_bulk"]);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn adaptive_hoster_fairness_isolated_by_origin_and_allows_direct_bulk() {
    let download_dir = test_runtime_dir("bulk-hoster-fairness-per-origin");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_per_origin");
    let mut ff_first = protected_hoster_bulk_job("job_ff_first", archive.clone());
    ff_first.url = "https://fuckingfast.co/first".into();
    ff_first.resolved_from_url = Some(ff_first.url.clone());
    let mut ff_second = protected_hoster_bulk_job("job_ff_second", archive.clone());
    ff_second.url = "https://www.fuckingfast.co/second".into();
    ff_second.resolved_from_url = Some(ff_second.url.clone());
    let mut datanodes = protected_hoster_bulk_job("job_datanodes", archive.clone());
    datanodes.url = "https://datanodes.to/61nni6me5p0n/Game.part02.rar".into();
    datanodes.resolved_from_url = Some(datanodes.url.clone());
    let mut direct_bulk = download_job(
        "job_direct_bulk",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    direct_bulk.url = "https://cdn.example.com/Game.part03.rar".into();
    direct_bulk.bulk_archive = Some(archive);

    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![ff_first, ff_second, datanodes, direct_bulk],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 4;
        runtime.settings.bulk.max_concurrent_downloads = 4;
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");
    let task_ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        task_ids,
        vec!["job_ff_first", "job_datanodes", "job_direct_bulk"]
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn protected_hoster_segment_budget_counts_current_effective_origin() {
    let download_dir = test_runtime_dir("bulk-hoster-segment-budget-current-origin");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_segment_budget");
    let mut active = protected_hoster_bulk_job("job_hoster_active", archive);
    active.state = JobState::Starting;
    active.url = "https://node41.datanodes.to/d/old-token/Game.bin".into();
    active.resolved_from_url = Some("https://datanodes.to/abc123456789/Game.bin".into());
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_hoster_active".into());
    }

    assert_eq!(
        state
            .active_protected_hoster_bulk_worker_counts(
                "job_hoster_active",
                "https://node42.datanodes.to/d/new-token/Game.bin",
            )
            .await,
        (1, 1)
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn safe_hoster_fairness_keeps_one_active_protected_hoster() {
    let download_dir = test_runtime_dir("bulk-hoster-safe-mode");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_safe");
    let mut active_hoster = protected_hoster_bulk_job("job_hoster_active", archive.clone());
    active_hoster.state = JobState::Downloading;
    active_hoster.speed = 128 * 1024;
    active_hoster.downloaded_bytes = 128 * 1024;
    let queued_hoster = protected_hoster_bulk_job("job_hoster_queued", archive);
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active_hoster, queued_hoster],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.settings.bulk.max_concurrent_downloads = 2;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Safe;
        runtime.active_workers.insert("job_hoster_active".into());
        seed_healthy_bulk_hoster_health(&mut runtime, "job_hoster_active", 128 * 1024);
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert!(tasks.is_empty());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn disabled_hoster_fairness_uses_bulk_pool_limit() {
    let download_dir = test_runtime_dir("bulk-hoster-fairness-off");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_off");
    let mut active_hoster = protected_hoster_bulk_job("job_hoster_active", archive.clone());
    active_hoster.state = JobState::Starting;
    let queued_hoster = protected_hoster_bulk_job("job_hoster_queued", archive);
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active_hoster, queued_hoster],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.settings.bulk.max_concurrent_downloads = 2;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Off;
        runtime.active_workers.insert("job_hoster_active".into());
        let now = Instant::now();
        let health = {
            let active_job = runtime.job("job_hoster_active").unwrap();
            BulkHosterWorkerHealth::from_job(active_job, now)
        };
        runtime
            .bulk_hoster_worker_health
            .insert("job_hoster_active".into(), health);
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "job_hoster_queued");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn resolver_wait_does_not_age_transfer_startup_grace() {
    let download_dir = test_runtime_dir("bulk-hoster-resolver-transfer-grace");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_resolver_grace");
    let mut active_hoster = protected_hoster_bulk_job("job_hoster_active", archive.clone());
    active_hoster.state = JobState::Downloading;
    active_hoster.downloaded_bytes = 50;
    let queued_hoster = protected_hoster_bulk_job("job_hoster_queued", archive);
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active_hoster, queued_hoster],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.settings.bulk.max_concurrent_downloads = 2;
        runtime.active_workers.insert("job_hoster_active".into());
        let now = Instant::now();
        let active_job = runtime.job("job_hoster_active").unwrap();
        let mut health =
            BulkHosterWorkerHealth::from_job(active_job, now - Duration::from_secs(90));
        health.mark_resolving(now - Duration::from_secs(85));
        health.mark_transferring(50, now);
        runtime
            .bulk_hoster_worker_health
            .insert("job_hoster_active".into(), health);
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert!(
        tasks.is_empty(),
        "fresh transfer startup should block another protected hoster even after a long resolver wait"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn adaptive_bulk_hoster_fairness_claims_one_hoster_initially() {
    let download_dir = test_runtime_dir("bulk-hoster-adaptive-initial");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_adaptive_initial");
    let jobs = (1..=4)
        .map(|index| protected_hoster_bulk_job(&format!("job_hoster_{index}"), archive.clone()))
        .collect::<Vec<_>>();
    let state = shared_state_with_jobs(download_dir.join("state.json"), jobs);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 4;
        runtime.settings.bulk.max_concurrent_downloads = 4;
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "job_hoster_1");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn adaptive_bulk_hoster_fairness_ramps_to_four_after_healthy_progress() {
    let download_dir = test_runtime_dir("bulk-hoster-adaptive-ramp");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_adaptive_ramp");
    let mut active = protected_hoster_bulk_job("job_hoster_1", archive.clone());
    active.state = JobState::Downloading;
    active.speed = 128 * 1024;
    active.downloaded_bytes = 128 * 1024;
    let queued = (2..=4)
        .map(|index| protected_hoster_bulk_job(&format!("job_hoster_{index}"), archive.clone()))
        .collect::<Vec<_>>();
    let mut jobs = vec![active];
    jobs.extend(queued);
    let state = shared_state_with_jobs(download_dir.join("state.json"), jobs);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 4;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.active_workers.insert("job_hoster_1".into());
        seed_healthy_bulk_hoster_health(&mut runtime, "job_hoster_1", 128 * 1024);
    }

    for next_id in ["job_hoster_2", "job_hoster_3", "job_hoster_4"] {
        let (_, tasks) = state
            .claim_schedulable_jobs()
            .await
            .expect("claiming jobs should work");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, next_id);

        let mut runtime = state.inner.write().await;
        let job = runtime
            .job_mut(next_id)
            .expect("claimed hoster job should exist");
        job.state = JobState::Downloading;
        job.speed = 128 * 1024;
        job.downloaded_bytes = 128 * 1024;
        seed_healthy_bulk_hoster_health(&mut runtime, next_id, 128 * 1024);
    }

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn protected_bulk_hoster_health_allows_next_hoster_after_recovery() {
    let download_dir = test_runtime_dir("bulk-hoster-fairness-recovered");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_recovered");
    let mut active_hoster = download_job(
        "job_hoster_active",
        JobState::Downloading,
        ResumeSupport::Supported,
        50,
    );
    active_hoster.resolved_from_url = Some("https://fuckingfast.co/active".into());
    active_hoster.bulk_archive = Some(archive.clone());
    active_hoster.speed = 96 * 1024;
    let mut queued_hoster = download_job(
        "job_hoster_queued",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    queued_hoster.resolved_from_url = Some("https://fuckingfast.co/queued".into());
    queued_hoster.bulk_archive = Some(archive);
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active_hoster, queued_hoster],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 2;
        runtime.settings.bulk.max_concurrent_downloads = 2;
        runtime.active_workers.insert("job_hoster_active".into());
        seed_healthy_bulk_hoster_health(&mut runtime, "job_hoster_active", 96 * 1024);
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "job_hoster_queued");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn adaptive_bulk_hoster_fairness_downshifts_after_aggregate_speed_collapse() {
    let download_dir = test_runtime_dir("bulk-hoster-adaptive-downshift");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_adaptive_downshift");
    let mut jobs = (1..=3)
        .map(|index| {
            let mut job =
                protected_hoster_bulk_job(&format!("job_hoster_{index}"), archive.clone());
            job.state = JobState::Downloading;
            job.speed = 96 * 1024;
            job.downloaded_bytes = 96 * 1024;
            job
        })
        .collect::<Vec<_>>();
    jobs.push(protected_hoster_bulk_job("job_hoster_4", archive));
    let state = shared_state_with_jobs(download_dir.join("state.json"), jobs);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 4;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        let now = Instant::now();
        let fairness_key =
            protected_bulk_hoster_fairness_key(runtime.job("job_hoster_1").unwrap()).unwrap();
        runtime.bulk_hoster_fairness.insert(
            fairness_key,
            BulkHosterFairnessController {
                target_active: 4,
                aggregate_baseline_speed: Some(512 * 1024),
                degraded_since: Some(
                    now - BULK_HOSTER_AGGREGATE_DEGRADATION_WINDOW - Duration::from_secs(1),
                ),
                cooldown_until: None,
                last_freeze_reported_at: None,
            },
        );
        for id in ["job_hoster_1", "job_hoster_2", "job_hoster_3"] {
            runtime.active_workers.insert(id.into());
            seed_healthy_bulk_hoster_health(&mut runtime, id, 96 * 1024);
        }
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert!(tasks.is_empty());
    let runtime = state.inner.read().await;
    let fairness_key =
        protected_bulk_hoster_fairness_key(runtime.job("job_hoster_1").unwrap()).unwrap();
    assert_eq!(
        runtime
            .bulk_hoster_fairness
            .get(&fairness_key)
            .unwrap()
            .target_active,
        3
    );
    for id in ["job_hoster_1", "job_hoster_2", "job_hoster_3"] {
        assert!(runtime.active_workers.contains(id));
    }
    assert!(runtime.diagnostic_events.iter().any(|event| event
        .message
        .contains("downshifted protected bulk hoster concurrency")));
    drop(runtime);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn protected_bulk_hoster_sustained_low_speed_holds_back_next_hoster() {
    let download_dir = test_runtime_dir("bulk-hoster-fairness-slow");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_slow");
    let mut active_hoster = download_job(
        "job_hoster_active",
        JobState::Downloading,
        ResumeSupport::Supported,
        50,
    );
    active_hoster.resolved_from_url = Some("https://fuckingfast.co/active".into());
    active_hoster.bulk_archive = Some(archive.clone());
    active_hoster.speed = 32 * 1024;
    let mut queued_hoster = download_job(
        "job_hoster_queued",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    queued_hoster.resolved_from_url = Some("https://fuckingfast.co/queued".into());
    queued_hoster.bulk_archive = Some(archive);
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active_hoster, queued_hoster],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 2;
        runtime.settings.bulk.max_concurrent_downloads = 2;
        runtime.active_workers.insert("job_hoster_active".into());
        let now = Instant::now();
        let active_job = runtime.job("job_hoster_active").unwrap();
        let mut health = BulkHosterWorkerHealth::from_job(
            active_job,
            now - BULK_HOSTER_STARTUP_GRACE_WINDOW - Duration::from_secs(1),
        );
        health.update(
            50,
            32 * 1024,
            now - BULK_HOSTER_LOW_SPEED_WINDOW - Duration::from_secs(1),
        );
        runtime
            .bulk_hoster_worker_health
            .insert("job_hoster_active".into(), health);
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert!(tasks.is_empty());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn protected_bulk_hoster_health_is_cleared_after_pause_and_cancel() {
    let download_dir = test_runtime_dir("bulk-hoster-fairness-pause-cancel");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_pause_cancel");
    let mut paused_hoster = protected_hoster_bulk_job("job_hoster_pause", archive.clone());
    paused_hoster.state = JobState::Downloading;
    let mut canceled_hoster = protected_hoster_bulk_job("job_hoster_cancel", archive);
    canceled_hoster.state = JobState::Downloading;
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![paused_hoster, canceled_hoster],
    );
    {
        let mut runtime = state.inner.write().await;
        for id in ["job_hoster_pause", "job_hoster_cancel"] {
            runtime.active_workers.insert(id.into());
            seed_healthy_bulk_hoster_health(&mut runtime, id, 96 * 1024);
        }
    }

    state
        .pause_job("job_hoster_pause")
        .await
        .expect("pause should work");
    state
        .cancel_job("job_hoster_cancel")
        .await
        .expect("cancel should work");

    let runtime = state.inner.read().await;
    assert!(!runtime
        .bulk_hoster_worker_health
        .contains_key("job_hoster_pause"));
    assert!(!runtime
        .bulk_hoster_worker_health
        .contains_key("job_hoster_cancel"));
    drop(runtime);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn protected_bulk_hoster_health_is_cleared_after_failure() {
    let download_dir = test_runtime_dir("bulk-hoster-fairness-cleanup");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_cleanup");
    let mut active_hoster = download_job(
        "job_hoster_active",
        JobState::Downloading,
        ResumeSupport::Supported,
        50,
    );
    active_hoster.resolved_from_url = Some("https://fuckingfast.co/active".into());
    active_hoster.bulk_archive = Some(archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active_hoster]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_hoster_active".into());
        let now = Instant::now();
        let health = {
            let active_job = runtime.job("job_hoster_active").unwrap();
            BulkHosterWorkerHealth::from_job(active_job, now)
        };
        runtime
            .bulk_hoster_worker_health
            .insert("job_hoster_active".into(), health);
    }

    state
        .fail_job(
            "job_hoster_active",
            "hoster timed out",
            FailureCategory::Network,
        )
        .await
        .expect("job should fail");

    let runtime = state.inner.read().await;
    assert!(!runtime.active_workers.contains("job_hoster_active"));
    assert!(!runtime
        .bulk_hoster_worker_health
        .contains_key("job_hoster_active"));
    drop(runtime);

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
        TorrentPeerConnectionWatchdogMode::Diagnose,
        "peer connection watchdog should default to diagnostic-only mode"
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
        auto_restart_attempts: 0,
        resolved_from_url: None,
        hoster_preflight: None,
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
        phase: TorrentRuntimePhase::Live,
        finished,
        error: None,
        diagnostics: None,
    }
}

fn runtime_state_with_jobs(jobs: Vec<DownloadJob>) -> RuntimeState {
    let job_indexes = job_indexes_for(&jobs);
    RuntimeState {
        connection_state: ConnectionState::Connected,
        jobs,
        settings: Settings::default(),
        main_window: None,
        diagnostic_events: Vec::new(),
        next_job_number: 99,
        job_indexes,
        active_workers: HashSet::new(),
        bulk_hoster_worker_health: HashMap::new(),
        bulk_hoster_fairness: HashMap::new(),
        external_reseed_jobs: HashSet::new(),
        last_host_contact: None,
        last_progress_persist_at: None,
    }
}

fn bulk_archive_info(download_dir: &Path, id: &str) -> BulkArchiveInfo {
    BulkArchiveInfo {
        id: id.into(),
        name: "Game.zip".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        archive_status: BulkArchiveStatus::Pending,
        requires_extraction: None,
        output_path: Some(download_dir.join("Bulk").join("Game").display().to_string()),
        error: None,
        warning: None,
        finalize_total_bytes: None,
        finalize_processed_bytes: None,
        finalize_mode: None,
    }
}

fn protected_hoster_bulk_job(id: &str, archive: BulkArchiveInfo) -> DownloadJob {
    let mut job = download_job(id, JobState::Queued, ResumeSupport::Unknown, 0);
    job.url = format!("https://fuckingfast.co/{id}");
    job.resolved_from_url = Some(job.url.clone());
    job.bulk_archive = Some(archive);
    job
}

fn seed_healthy_bulk_hoster_health(runtime: &mut RuntimeState, id: &str, speed: u64) {
    let now = Instant::now();
    let job = runtime.job(id).expect("hoster job should exist");
    let mut health = BulkHosterWorkerHealth::from_job(
        job,
        now - BULK_HOSTER_STARTUP_GRACE_WINDOW - Duration::from_secs(1),
    );
    health.mark_transferring(
        job.downloaded_bytes,
        now - BULK_HOSTER_STARTUP_GRACE_WINDOW - Duration::from_secs(1),
    );
    let first_bytes = job.downloaded_bytes.saturating_add(speed.max(1));
    health.update(
        first_bytes,
        speed,
        now - BULK_HOSTER_HEALTH_SAMPLE_WINDOW - Duration::from_secs(1),
    );
    health.update(first_bytes.saturating_add(speed.max(1)), speed, now);
    runtime
        .bulk_hoster_worker_health
        .insert(id.to_string(), health);
}

fn shared_state_with_jobs(storage_path: PathBuf, jobs: Vec<DownloadJob>) -> SharedState {
    SharedState {
        inner: Arc::new(RwLock::new(runtime_state_with_jobs(jobs))),
        storage_path: Arc::new(storage_path),
        handoff_auth: Arc::new(RwLock::new(HashMap::new())),
    }
}

fn bulk_test_entries() -> Vec<BatchDownloadEntry> {
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
    ]
}

async fn complete_bulk_members_for_ready(state: &SharedState) {
    let mut runtime = state.inner.write().await;
    for (index, job) in runtime.jobs.iter_mut().enumerate() {
        let target_path = PathBuf::from(&job.target_path);
        std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
        std::fs::write(&target_path, format!("part-{index}")).unwrap();
        job.state = JobState::Completed;
        job.progress = 100.0;
        job.downloaded_bytes = job.total_bytes;
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
