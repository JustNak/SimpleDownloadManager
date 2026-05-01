use simple_download_manager_desktop_core::contracts::{
    AddJobResult, AddJobStatus, AddJobsResult, ProgressBatchContext, ProgressBatchKind,
};
use simple_download_manager_desktop_core::prompts::PromptDuplicateAction;
use simple_download_manager_desktop_core::storage::{
    BulkArchiveInfo, BulkArchiveStatus, ConnectionState, DesktopSnapshot, DiagnosticEvent,
    DiagnosticLevel, DiagnosticsSnapshot, DownloadJob, DownloadPrompt, DownloadSource,
    FailureCategory, HostRegistrationDiagnostics, HostRegistrationEntry, HostRegistrationStatus,
    JobState, QueueSummary, ResumeSupport, Settings, StartupLaunchMode, Theme, TorrentInfo,
    TorrentJobDiagnostics, TorrentPeerConnectionWatchdogMode, TorrentPeerDiagnostics,
    TorrentRuntimeDiagnostics, TorrentSeedMode, TorrentSettings, TransferKind,
};
use simple_download_manager_desktop_slint::controller::{
    active_download_urls, add_download_outcome_for_result, batch_details_from_context,
    build_filename, category_folder_for_filename, default_delete_from_disk_for_jobs,
    default_torrent_download_directory, delete_action_label_for_job, delete_context_menu_label,
    delete_prompt_content, delete_prompt_from_jobs, diagnostics_view_model_from_snapshot,
    download_progress_metrics, download_ready_detail, download_ready_label, download_submit_label,
    ensure_trailing_editable_line, external_use_auto_reseed_message, filter_excluded_hosts,
    format_diagnostics_report, infer_transfer_kind_for_url, job_row_from_job,
    normalize_accent_color, normalize_archive_name, normalize_extension_input,
    normalize_torrent_settings, parse_download_url_lines, parse_excluded_host_input,
    progress_details_from_job, progress_details_from_job_with_state, prompt_confirm_request,
    prompt_details_from_prompt, prompt_details_from_prompt_with_state,
    queue_view_model_from_snapshot, record_progress_sample, registration_status_label,
    registration_status_message, registration_status_tone, remove_excluded_host,
    reset_prompt_interaction_state, select_job_range, settings_equal,
    settings_view_model_from_state, should_adopt_incoming_settings_draft, should_stop_seeding,
    split_filename, status_text_from_snapshot, toast_for_shell_error, toast_message,
    torrent_info_hash, torrent_peer_health_dots, torrent_progress_strip_text,
    torrent_remaining_text, torrent_source_summary, validate_optional_sha256, AddDownloadFormState,
    AddDownloadProgressIntent, AddDownloadResult, DownloadCategory, DownloadMode,
    ProgressPopupInteractionState, ProgressSample, PromptConfirmAction,
    PromptWindowInteractionState, QueueUiState, SelectionState, SettingsDraftState,
    SettingsSection, SortColumn, SortDirection, SortMode, ToastMessage, ToastType, ViewFilter,
    TOAST_AUTO_CLOSE_MS,
};

#[test]
fn toast_helpers_preserve_react_lifecycle_and_copy() {
    assert_eq!(TOAST_AUTO_CLOSE_MS, 3_000);
    assert_eq!(ToastType::Info.id(), "info");
    assert_eq!(ToastType::Success.tone(), "success");
    assert_eq!(ToastType::Warning.id(), "warning");
    assert_eq!(ToastType::Error.tone(), "error");

    let toast = toast_message(
        ToastType::Success,
        "Settings Saved",
        "Preferences updated successfully.",
    );
    assert_eq!(
        toast,
        ToastMessage {
            id: String::new(),
            toast_type: ToastType::Success,
            title: "Settings Saved".into(),
            message: "Preferences updated successfully.".into(),
            auto_close: true,
        }
    );

    let update = ToastMessage::persistent(
        ToastType::Info,
        "Update Available",
        "Simple Download Manager 0.3.53-alpha is ready to install.",
    );
    assert!(!update.auto_close);

    assert_eq!(
        external_use_auto_reseed_message("file", 60),
        "Windows can use the file now. Simple Download Manager will try to resume seeding every 60s."
    );
    assert_eq!(
        toast_for_shell_error("reveal path", "Access is denied.").title,
        "Shell Error"
    );
    assert_eq!(
        toast_for_shell_error("reveal path", "Access is denied.").message,
        "reveal path failed: Access is denied."
    );
}

#[test]
fn add_download_input_helpers_match_react_modal_behavior() {
    let long_signed_url = "https://store-044.wnam.tb-cdn.io/zip/067d34b2-b6b8-4324-b795-3b45544d9dfb?token=ea24bba1-eba0-4a5d-92cd-bbe07d59b864";

    assert_eq!(
        parse_download_url_lines(long_signed_url),
        vec![long_signed_url]
    );
    assert_eq!(
        parse_download_url_lines(&format!(
            "{long_signed_url}\nhttps://example.com/second.zip"
        )),
        vec![long_signed_url, "https://example.com/second.zip"]
    );
    assert_eq!(ensure_trailing_editable_line(""), "");
    assert_eq!(
        ensure_trailing_editable_line(long_signed_url),
        format!("{long_signed_url}\n")
    );

    assert_eq!(
        download_submit_label(DownloadMode::Single, 1, true),
        "Start Download"
    );
    assert_eq!(
        download_submit_label(DownloadMode::Torrent, 1, true),
        "Add Torrent"
    );
    assert_eq!(
        download_submit_label(DownloadMode::Multi, 2, true),
        "Queue 2 Downloads"
    );
    assert_eq!(
        download_submit_label(DownloadMode::Bulk, 2, true),
        "Queue 2 Downloads and Combine"
    );
    assert_eq!(
        download_ready_label(DownloadMode::Torrent, 1),
        "1 torrent ready"
    );
    assert_eq!(
        download_ready_label(DownloadMode::Multi, 2),
        "2 links ready"
    );
    assert_eq!(
        download_ready_detail(DownloadMode::Bulk, true, "bulk-download.zip"),
        "bulk-download.zip"
    );
    assert_eq!(
        download_ready_detail(DownloadMode::Multi, false, ""),
        "Queue only"
    );

    assert_eq!(validate_optional_sha256(""), Ok(None));
    assert_eq!(
        validate_optional_sha256(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        ),
        Ok(Some(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()
        ))
    );
    assert_eq!(
        validate_optional_sha256("abc123"),
        Err("SHA-256 checksum must be 64 hexadecimal characters.".into())
    );

    assert_eq!(normalize_archive_name("bundle"), "bundle.zip");
    assert_eq!(normalize_archive_name("Bundle.ZIP"), "Bundle.ZIP");
    assert_eq!(normalize_archive_name("<bad>|name"), "badname.zip");
    assert_eq!(normalize_archive_name("   "), "");
    assert_eq!(
        infer_transfer_kind_for_url("magnet:?xt=urn:btih:abc"),
        TransferKind::Torrent
    );
    assert_eq!(
        infer_transfer_kind_for_url("https://example.com/file.torrent"),
        TransferKind::Torrent
    );
    assert_eq!(
        infer_transfer_kind_for_url("C:/Downloads/file.torrent"),
        TransferKind::Http
    );
}

#[test]
fn add_download_form_state_tracks_active_urls_by_mode() {
    let state = AddDownloadFormState {
        mode: DownloadMode::Single,
        single_url: " https://example.com/file.zip ".into(),
        torrent_url: "magnet:?xt=urn:btih:abc".into(),
        multi_urls: "https://example.com/a.bin\n\n https://example.com/b.bin\n".into(),
        bulk_urls: "https://example.com/bulk-a.bin\nhttps://example.com/bulk-b.bin".into(),
        ..Default::default()
    };
    assert_eq!(
        active_download_urls(&state),
        vec!["https://example.com/file.zip"]
    );

    let torrent_state = AddDownloadFormState {
        mode: DownloadMode::Torrent,
        ..state.clone()
    };
    assert_eq!(
        active_download_urls(&torrent_state),
        vec!["magnet:?xt=urn:btih:abc"]
    );

    let multi_state = AddDownloadFormState {
        mode: DownloadMode::Multi,
        ..state.clone()
    };
    assert_eq!(
        active_download_urls(&multi_state),
        vec!["https://example.com/a.bin", "https://example.com/b.bin"]
    );

    let bulk_state = AddDownloadFormState {
        mode: DownloadMode::Bulk,
        ..state
    };
    assert_eq!(
        active_download_urls(&bulk_state),
        vec![
            "https://example.com/bulk-a.bin",
            "https://example.com/bulk-b.bin"
        ]
    );
}

#[test]
fn add_download_progress_intents_match_react_submission_behavior() {
    let queued = AddJobResult {
        job_id: "job_queued".into(),
        filename: "queued.zip".into(),
        status: AddJobStatus::Queued,
    };
    let duplicate = AddJobResult {
        job_id: "job_duplicate".into(),
        filename: "duplicate.zip".into(),
        status: AddJobStatus::DuplicateExistingJob,
    };

    let single = add_download_outcome_for_result(
        DownloadMode::Single,
        AddDownloadResult::Single(queued.clone()),
        None,
    );
    assert_eq!(single.primary_job_id.as_deref(), Some("job_queued"));
    assert_eq!(
        single.progress_intent,
        Some(AddDownloadProgressIntent::Single {
            job_id: "job_queued".into()
        })
    );
    assert_eq!(single.view_id, "all");

    let torrent_duplicate = add_download_outcome_for_result(
        DownloadMode::Torrent,
        AddDownloadResult::Single(duplicate.clone()),
        None,
    );
    assert_eq!(
        torrent_duplicate.primary_job_id.as_deref(),
        Some("job_duplicate")
    );
    assert_eq!(torrent_duplicate.progress_intent, None);
    assert_eq!(torrent_duplicate.view_id, "torrents");

    let batch_result = AddJobsResult {
        results: vec![duplicate.clone(), queued.clone()],
        queued_count: 1,
        duplicate_count: 1,
    };
    let multi = add_download_outcome_for_result(
        DownloadMode::Multi,
        AddDownloadResult::Batch(batch_result),
        None,
    );
    assert_eq!(multi.primary_job_id.as_deref(), Some("job_queued"));
    match multi.progress_intent {
        Some(AddDownloadProgressIntent::Batch { context }) => {
            assert_eq!(context.kind, ProgressBatchKind::Multi);
            assert_eq!(context.title, "Multi-download progress");
            assert_eq!(context.job_ids, vec!["job_queued"]);
            assert_eq!(context.archive_name, None);
        }
        other => panic!("expected multi batch progress intent, got {other:?}"),
    }

    let duplicate_only = AddJobsResult {
        results: vec![duplicate],
        queued_count: 0,
        duplicate_count: 1,
    };
    let bulk_duplicate = add_download_outcome_for_result(
        DownloadMode::Bulk,
        AddDownloadResult::Batch(duplicate_only),
        Some("bundle.zip"),
    );
    assert_eq!(bulk_duplicate.progress_intent, None);

    let bulk_result = AddJobsResult {
        results: vec![queued],
        queued_count: 1,
        duplicate_count: 0,
    };
    let bulk = add_download_outcome_for_result(
        DownloadMode::Bulk,
        AddDownloadResult::Batch(bulk_result),
        Some("bundle.zip"),
    );
    match bulk.progress_intent {
        Some(AddDownloadProgressIntent::Batch { context }) => {
            assert_eq!(context.kind, ProgressBatchKind::Bulk);
            assert_eq!(context.title, "Bulk download progress");
            assert_eq!(context.archive_name.as_deref(), Some("bundle.zip"));
        }
        other => panic!("expected bulk batch progress intent, got {other:?}"),
    }
}

#[test]
fn settings_draft_adoption_and_view_sections_match_react_settings_page() {
    let saved = Settings {
        download_directory: "C:/Downloads".into(),
        theme: Theme::System,
        accent_color: "#3b82f6".into(),
        startup_launch_mode: StartupLaunchMode::Open,
        ..Settings::default()
    };
    let mut dirty_draft = saved.clone();
    dirty_draft.theme = Theme::Dark;
    let mut next = saved.clone();
    next.download_directory = "D:/Incoming".into();

    assert!(settings_equal(&saved, &saved));
    assert!(!settings_equal(&dirty_draft, &saved));
    assert!(should_adopt_incoming_settings_draft(&saved, &saved, &next));
    assert!(should_adopt_incoming_settings_draft(&next, &saved, &next));
    assert!(!should_adopt_incoming_settings_draft(
        &dirty_draft,
        &saved,
        &next
    ));

    let state = SettingsDraftState::new(saved.clone());
    let model = settings_view_model_from_state(&state);
    assert_eq!(
        model
            .sections
            .iter()
            .map(|section| (section.id.as_str(), section.label.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("general", "General"),
            ("updates", "App Updates"),
            ("torrenting", "Torrenting"),
            ("appearance", "Appearance/Behavior"),
            ("extension", "Web Extension"),
            ("native_host", "Native Host"),
        ]
    );
    assert_eq!(
        SettingsSection::from_id("torrenting"),
        Some(SettingsSection::Torrenting)
    );
    assert_eq!(
        SettingsSection::from_id("native_host"),
        Some(SettingsSection::NativeHost)
    );
    assert_eq!(model.active_section_id, "general");
    assert_eq!(model.theme_id, "system");
    assert_eq!(model.startup_launch_mode_id, "open");
    assert!(!model.dirty);
}

#[test]
fn diagnostics_status_helpers_match_react_native_host_copy() {
    assert_eq!(
        registration_status_label(Some(HostRegistrationStatus::Configured)),
        "Ready"
    );
    assert_eq!(
        registration_status_message(Some(HostRegistrationStatus::Configured)),
        "At least one browser has a valid native host registration and host binary path."
    );
    assert_eq!(
        registration_status_tone(Some(HostRegistrationStatus::Configured)),
        "success"
    );

    assert_eq!(
        registration_status_label(Some(HostRegistrationStatus::Broken)),
        "Repair"
    );
    assert_eq!(
        registration_status_message(Some(HostRegistrationStatus::Broken)),
        "A browser registration exists, but the manifest or native host binary path is broken."
    );
    assert_eq!(
        registration_status_tone(Some(HostRegistrationStatus::Broken)),
        "warning"
    );

    assert_eq!(
        registration_status_label(Some(HostRegistrationStatus::Missing)),
        "Missing"
    );
    assert_eq!(
        registration_status_message(Some(HostRegistrationStatus::Missing)),
        "No browser registration was detected for the native messaging host."
    );
    assert_eq!(
        registration_status_tone(Some(HostRegistrationStatus::Missing)),
        "error"
    );

    assert_eq!(registration_status_label(None), "Checking");
    assert_eq!(
        registration_status_message(None),
        "Diagnostics are still loading."
    );
    assert_eq!(registration_status_tone(None), "neutral");
}

#[test]
fn diagnostics_report_formatting_matches_react_report() {
    let diagnostics = diagnostics_snapshot();
    let report = format_diagnostics_report(&diagnostics);

    assert!(report.contains("Simple Download Manager Diagnostics"));
    assert!(report.contains("Connection State: connected"));
    assert!(report.contains("Last Host Contact: 12 seconds ago"));
    assert!(report.contains("Queue Total: 3"));
    assert!(report.contains("Host Registration Status: configured"));
    assert!(report.contains("- Chrome"));
    assert!(report.contains("Registry: HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts"));
    assert!(report.contains("Manifest: C:/Users/Me/AppData/host.json"));
    assert!(report.contains("Host Binary Exists: true"));
    assert!(report.contains("Torrent Diagnostics:"));
    assert!(report.contains("- torrent_1 ubuntu.iso"));
    assert!(report.contains("Info Hash: abc123"));
    assert!(report.contains("Listen Port: 42000 (fallback active)"));
    assert!(report.contains("Peer Samples:"));
    assert!(report.contains("- connecting fetched 512 bytes, errors 1, pieces 2, attempts 3"));
    assert!(report.contains("Recent Events:"));
    assert!(report.contains("- unknown-time info download job_1 Completed file.zip"));
}

#[test]
fn diagnostics_view_model_reverses_events_and_preserves_host_entries() {
    let diagnostics = diagnostics_snapshot();
    let model = diagnostics_view_model_from_snapshot(
        Some(&diagnostics),
        false,
        "Diagnostics refreshed.",
        "",
    );

    assert!(model.has_snapshot);
    assert!(!model.loading);
    assert_eq!(model.status_label, "Ready");
    assert_eq!(model.status_tone, "success");
    assert_eq!(model.last_host_contact_text, "12 seconds ago");
    assert_eq!(
        model.queue_summary_text,
        "3 total | 1 active | 1 needs attention"
    );
    assert_eq!(model.action_status_text, "Diagnostics refreshed.");
    assert_eq!(model.error_text, "");
    assert_eq!(model.host_entries.len(), 2);
    assert_eq!(model.host_entries[0].browser, "Chrome");
    assert_eq!(model.host_entries[0].status_label, "Ready");
    assert_eq!(model.host_entries[0].status_tone, "success");
    assert_eq!(model.host_entries[1].browser, "Firefox");
    assert_eq!(model.host_entries[1].status_label, "Missing");
    assert_eq!(model.host_entries[1].status_tone, "neutral");
    assert_eq!(
        model
            .recent_events
            .iter()
            .map(|event| event.message.as_str())
            .collect::<Vec<_>>(),
        vec!["Queued retry", "Completed file.zip"]
    );
    assert_eq!(model.recent_events[1].timestamp_text, "Unknown time");

    let loading = diagnostics_view_model_from_snapshot(None, true, "", "");
    assert!(!loading.has_snapshot);
    assert!(loading.loading);
    assert_eq!(loading.status_label, "Checking");
    assert_eq!(loading.status_message, "Diagnostics are still loading.");
}

#[test]
fn torrent_settings_helpers_match_react_normalization_and_seed_policy() {
    assert_eq!(
        default_torrent_download_directory(" C:\\Users\\Me\\Downloads\\\\ "),
        "C:\\Users\\Me\\Downloads\\Torrent"
    );
    assert_eq!(
        default_torrent_download_directory(" /home/me/Downloads// "),
        "/home/me/Downloads/Torrent"
    );
    assert_eq!(default_torrent_download_directory("  "), "");

    let normalized = normalize_torrent_settings(
        TorrentSettings {
            download_directory: "  ".into(),
            seed_ratio_limit: -5.0,
            seed_time_limit_minutes: 900_000,
            upload_limit_kib_per_second: 2_000_000,
            port_forwarding_enabled: true,
            port_forwarding_port: 999,
            peer_connection_watchdog_mode: TorrentPeerConnectionWatchdogMode::Experimental,
            ..TorrentSettings::default()
        },
        "D:/Downloads",
    );
    assert_eq!(normalized.download_directory, "D:/Downloads/Torrent");
    assert_eq!(normalized.seed_ratio_limit, 0.1);
    assert_eq!(normalized.seed_time_limit_minutes, 525_600);
    assert_eq!(normalized.upload_limit_kib_per_second, 1_048_576);
    assert_eq!(normalized.port_forwarding_port, 42_000);
    assert_eq!(
        normalized.peer_connection_watchdog_mode,
        TorrentPeerConnectionWatchdogMode::Experimental
    );

    let ratio_limited = TorrentSettings {
        seed_mode: TorrentSeedMode::Ratio,
        seed_ratio_limit: 1.5,
        ..TorrentSettings::default()
    };
    assert!(should_stop_seeding(&ratio_limited, 1.5, 1));
    assert!(!should_stop_seeding(&ratio_limited, 1.49, 10_000));

    let time_limited = TorrentSettings {
        seed_mode: TorrentSeedMode::Time,
        seed_time_limit_minutes: 2,
        ..TorrentSettings::default()
    };
    assert!(!should_stop_seeding(&time_limited, 99.0, 119));
    assert!(should_stop_seeding(&time_limited, 0.0, 120));
}

#[test]
fn settings_excluded_sites_and_accent_helpers_match_react_behavior() {
    assert_eq!(
        parse_excluded_host_input(
            " HTTPS://User@Example.COM:443/path, *.Sub.Example.com \n bad host "
        ),
        vec!["example.com", "*.sub.example.com"]
    );

    let result = simple_download_manager_desktop_slint::controller::add_excluded_hosts(
        vec!["web.telegram.org".into()],
        vec![
            "https://Example.com/download".into(),
            "example.com".into(),
            "web.telegram.org".into(),
            "bad host".into(),
        ],
    );
    assert_eq!(
        result.hosts,
        vec!["web.telegram.org".to_string(), "example.com".to_string()]
    );
    assert_eq!(result.added_hosts, vec!["example.com".to_string()]);
    assert_eq!(
        result.duplicate_hosts,
        vec!["example.com".to_string(), "web.telegram.org".to_string()]
    );
    assert_eq!(
        remove_excluded_host(&result.hosts, "web.telegram.org"),
        vec!["example.com".to_string()]
    );
    assert_eq!(
        filter_excluded_hosts(&result.hosts, "EXAMPLE"),
        vec!["example.com".to_string()]
    );
    assert_eq!(
        simple_download_manager_desktop_slint::controller::format_excluded_sites_summary(
            &result.hosts
        ),
        "2 excluded sites"
    );
    assert_eq!(normalize_accent_color("  ABCDEF "), "#abcdef");
    assert_eq!(normalize_accent_color("not a color"), "#3b82f6");
}

#[test]
fn job_row_conversion_preserves_queue_identity_and_progress_text() {
    let job = DownloadJob {
        id: "job_1".into(),
        url: "https://example.com/archive.zip".into(),
        filename: "archive.zip".into(),
        source: None,
        transfer_kind: TransferKind::Http,
        integrity_check: None,
        torrent: None,
        state: JobState::Downloading,
        created_at: 0,
        progress: 42.5,
        total_bytes: 200,
        downloaded_bytes: 85,
        speed: 0,
        eta: 0,
        error: None,
        failure_category: None,
        resume_support: Default::default(),
        retry_attempts: 0,
        target_path: "C:/Downloads/archive.zip".into(),
        temp_path: "C:/Downloads/archive.zip.part".into(),
        artifact_exists: None,
        bulk_archive: None,
    };

    let row = job_row_from_job(&job);

    assert_eq!(row.id, "job_1");
    assert_eq!(row.filename, "archive.zip");
    assert_eq!(row.state, "Downloading");
    assert_eq!(row.progress, 42.5);
    assert_eq!(row.bytes_text, "85 B / 200 B");
}

#[test]
fn snapshot_status_text_summarizes_connection_and_queue_counts() {
    let snapshot = DesktopSnapshot {
        connection_state: ConnectionState::Connected,
        jobs: vec![DownloadJob {
            id: "job_1".into(),
            url: "https://example.com/archive.zip".into(),
            filename: "archive.zip".into(),
            source: None,
            transfer_kind: TransferKind::Http,
            integrity_check: None,
            torrent: None,
            state: JobState::Queued,
            created_at: 0,
            progress: 0.0,
            total_bytes: 0,
            downloaded_bytes: 0,
            speed: 0,
            eta: 0,
            error: None,
            failure_category: None,
            resume_support: Default::default(),
            retry_attempts: 0,
            target_path: "C:/Downloads/archive.zip".into(),
            temp_path: "C:/Downloads/archive.zip.part".into(),
            artifact_exists: None,
            bulk_archive: None,
        }],
        settings: Settings::default(),
    };

    assert_eq!(
        status_text_from_snapshot(&snapshot),
        "Connected to browser handoff | 1 download"
    );
}

#[test]
fn prompt_details_preserve_download_prompt_payload() {
    let prompt = DownloadPrompt {
        id: "prompt_1".into(),
        url: "https://example.com/archive.zip".into(),
        filename: "archive.zip".into(),
        source: None,
        total_bytes: Some(4096),
        default_directory: "C:/Downloads".into(),
        target_path: "C:/Downloads/archive.zip".into(),
        duplicate_job: None,
        duplicate_path: None,
        duplicate_filename: None,
        duplicate_reason: None,
    };

    let details = prompt_details_from_prompt(&prompt);

    assert_eq!(details.id, "prompt_1");
    assert_eq!(details.title, "New download detected");
    assert_eq!(details.filename, "archive.zip");
    assert_eq!(details.url, "https://example.com/archive.zip");
    assert_eq!(details.destination, "C:/Downloads/archive.zip");
    assert_eq!(details.size_text, "4.0 KiB");
    assert_eq!(details.duplicate_text, "");
}

#[test]
fn prompt_details_surface_duplicate_context() {
    let duplicate = DownloadJob {
        id: "job_existing".into(),
        url: "https://example.com/archive.zip".into(),
        filename: "archive.zip".into(),
        source: None,
        transfer_kind: TransferKind::Http,
        integrity_check: None,
        torrent: None,
        state: JobState::Completed,
        created_at: 0,
        progress: 100.0,
        total_bytes: 4096,
        downloaded_bytes: 4096,
        speed: 0,
        eta: 0,
        error: None,
        failure_category: None,
        resume_support: Default::default(),
        retry_attempts: 0,
        target_path: "C:/Downloads/archive.zip".into(),
        temp_path: "C:/Downloads/archive.zip.part".into(),
        artifact_exists: None,
        bulk_archive: None,
    };
    let prompt = DownloadPrompt {
        id: "prompt_2".into(),
        url: "https://example.com/archive.zip".into(),
        filename: "archive.zip".into(),
        source: None,
        total_bytes: Some(4096),
        default_directory: "C:/Downloads".into(),
        target_path: "C:/Downloads/archive.zip".into(),
        duplicate_job: Some(duplicate),
        duplicate_path: Some("C:/Downloads/archive.zip".into()),
        duplicate_filename: Some("archive.zip".into()),
        duplicate_reason: Some("target_exists".into()),
    };

    let details = prompt_details_from_prompt(&prompt);

    assert_eq!(details.title, "Duplicate download detected");
    assert_eq!(
        details.duplicate_text,
        "Duplicate target_exists: archive.zip"
    );
}

#[test]
fn prompt_duplicate_details_match_react_compact_prompt_behavior() {
    let duplicate = DownloadJob {
        id: "job_existing".into(),
        url: "https://example.com/archive.zip".into(),
        filename: "existing.zip".into(),
        source: None,
        transfer_kind: TransferKind::Http,
        integrity_check: None,
        torrent: None,
        state: JobState::Completed,
        created_at: 0,
        progress: 100.0,
        total_bytes: 4096,
        downloaded_bytes: 4096,
        speed: 0,
        eta: 0,
        error: None,
        failure_category: None,
        resume_support: Default::default(),
        retry_attempts: 0,
        target_path: "C:/Downloads/Compressed/existing.zip".into(),
        temp_path: "C:/Downloads/Compressed/existing.zip.part".into(),
        artifact_exists: None,
        bulk_archive: None,
    };
    let prompt = DownloadPrompt {
        id: "prompt_duplicate_job".into(),
        url: "https://example.com/archive.zip".into(),
        filename: "archive.zip".into(),
        source: Some(DownloadSource {
            entry_point: "browser_download".into(),
            browser: "chrome".into(),
            extension_version: "0.3.52".into(),
            page_url: None,
            page_title: None,
            referrer: None,
            incognito: None,
        }),
        total_bytes: Some(4096),
        default_directory: "C:/Downloads".into(),
        target_path: "C:/Downloads/Compressed/archive.zip".into(),
        duplicate_job: Some(duplicate),
        duplicate_path: None,
        duplicate_filename: Some("archive.zip".into()),
        duplicate_reason: Some("url".into()),
    };

    let details =
        prompt_details_from_prompt_with_state(&prompt, &PromptWindowInteractionState::default());

    assert_eq!(details.title, "Duplicate download detected");
    assert!(details.is_duplicate);
    assert_eq!(details.duplicate_label, "existing.zip");
    assert_eq!(details.duplicate_message, "Already in queue: ");
    assert_eq!(details.overwrite_label, "replace queue");
    assert_eq!(details.source_label, "chrome browser download");
    assert!(!details.can_swap_to_browser);
}

#[test]
fn prompt_destination_duplicate_and_swap_state_match_react_behavior() {
    let mut prompt = DownloadPrompt {
        id: "prompt_destination".into(),
        url: "https://example.com/archive.zip".into(),
        filename: "archive.zip".into(),
        source: Some(DownloadSource {
            entry_point: "browser_download".into(),
            browser: "edge".into(),
            extension_version: "0.3.52".into(),
            page_url: None,
            page_title: None,
            referrer: None,
            incognito: None,
        }),
        total_bytes: Some(4096),
        default_directory: "C:/Downloads".into(),
        target_path: "C:/Downloads/Compressed/archive.zip".into(),
        duplicate_job: None,
        duplicate_path: None,
        duplicate_filename: None,
        duplicate_reason: None,
    };
    let state = PromptWindowInteractionState {
        directory_override: Some("D:/Incoming".into()),
        ..Default::default()
    };

    let details = prompt_details_from_prompt_with_state(&prompt, &state);

    assert_eq!(category_folder_for_filename("archive.zip"), "Compressed");
    assert_eq!(details.destination, "D:/Incoming/Compressed/archive.zip");
    assert_eq!(details.source_label, "edge browser download");
    assert!(details.can_swap_to_browser);

    prompt.duplicate_path = Some("D:/Incoming/Compressed/archive.zip".into());
    prompt.duplicate_filename = Some("archive.zip".into());
    prompt.duplicate_reason = Some("target_exists".into());
    let duplicate_details = prompt_details_from_prompt_with_state(&prompt, &state);

    assert!(duplicate_details.is_duplicate);
    assert_eq!(duplicate_details.duplicate_label, "archive.zip");
    assert_eq!(duplicate_details.duplicate_message, "Destination exists: ");
    assert_eq!(duplicate_details.overwrite_label, "replace file");
    assert!(!duplicate_details.can_swap_to_browser);
}

#[test]
fn prompt_confirm_requests_map_duplicate_actions_and_rename_validation() {
    let prompt = DownloadPrompt {
        id: "prompt_confirm".into(),
        url: "https://example.com/archive.zip".into(),
        filename: "archive.zip".into(),
        source: None,
        total_bytes: Some(4096),
        default_directory: "C:/Downloads".into(),
        target_path: "C:/Downloads/archive.zip".into(),
        duplicate_job: None,
        duplicate_path: Some("C:/Downloads/archive.zip".into()),
        duplicate_filename: Some("archive.zip".into()),
        duplicate_reason: Some("target_exists".into()),
    };
    let mut state = PromptWindowInteractionState {
        directory_override: Some("D:/Incoming".into()),
        renamed_filename: "  renamed.zip  ".into(),
        ..Default::default()
    };

    let renamed = prompt_confirm_request(&prompt, &state, PromptConfirmAction::Rename).unwrap();
    assert_eq!(renamed.id, "prompt_confirm");
    assert_eq!(renamed.directory_override.as_deref(), Some("D:/Incoming"));
    assert_eq!(renamed.duplicate_action, PromptDuplicateAction::Rename);
    assert_eq!(renamed.renamed_filename.as_deref(), Some("renamed.zip"));

    let overwrite =
        prompt_confirm_request(&prompt, &state, PromptConfirmAction::Overwrite).unwrap();
    assert_eq!(overwrite.duplicate_action, PromptDuplicateAction::Overwrite);
    assert_eq!(overwrite.renamed_filename, None);

    let download_anyway =
        prompt_confirm_request(&prompt, &state, PromptConfirmAction::DownloadAnyway).unwrap();
    assert_eq!(
        download_anyway.duplicate_action,
        PromptDuplicateAction::DownloadAnyway
    );

    let default_download =
        prompt_confirm_request(&prompt, &state, PromptConfirmAction::DefaultDownload).unwrap();
    assert_eq!(
        default_download.duplicate_action,
        PromptDuplicateAction::ReturnExisting
    );

    state.renamed_filename = "   ".into();
    assert_eq!(
        prompt_confirm_request(&prompt, &state, PromptConfirmAction::Rename),
        Err("Enter a file name before renaming the duplicate download.".into())
    );

    reset_prompt_interaction_state(&mut state, Some(&prompt));
    assert_eq!(state.renamed_filename, "archive.zip");
    assert!(state.directory_override.is_none());
    assert!(!state.duplicate_menu_open);
    assert!(!state.renaming_duplicate);
}

#[test]
fn progress_details_preserve_job_state_and_error() {
    let job = DownloadJob {
        id: "job_2".into(),
        url: "https://example.com/video.mp4".into(),
        filename: "video.mp4".into(),
        source: None,
        transfer_kind: TransferKind::Torrent,
        integrity_check: None,
        torrent: None,
        state: JobState::Failed,
        created_at: 0,
        progress: 45.0,
        total_bytes: 2000,
        downloaded_bytes: 900,
        speed: 0,
        eta: 0,
        error: Some("network failed".into()),
        failure_category: None,
        resume_support: Default::default(),
        retry_attempts: 0,
        target_path: "C:/Downloads/video.mp4".into(),
        temp_path: "C:/Downloads/video.mp4.part".into(),
        artifact_exists: None,
        bulk_archive: None,
    };

    let details = progress_details_from_job(&job, "Torrent session");

    assert_eq!(details.id, "job_2");
    assert_eq!(details.title, "Torrent session");
    assert_eq!(details.filename, "video.mp4");
    assert_eq!(details.state, "Failed");
    assert_eq!(details.progress, 45.0);
    assert_eq!(details.bytes_text, "900 B / 2.0 KiB");
    assert_eq!(details.error_text, "network failed");
}

#[test]
fn http_progress_metrics_and_actions_match_compact_react_popup() {
    let mut job = queue_job(
        "http_active",
        "archive.zip",
        TransferKind::Http,
        JobState::Downloading,
    );
    job.url = "https://downloads.example.com/files/archive.zip".into();
    job.total_bytes = 1_000;
    job.downloaded_bytes = 600;
    job.speed = 10;
    job.progress = 60.0;
    job.target_path = "C:/Downloads/Compressed/archive.zip".into();

    let samples = vec![
        ProgressSample {
            job_id: "http_active".into(),
            timestamp: 1_000,
            downloaded_bytes: 200,
        },
        ProgressSample {
            job_id: "http_active".into(),
            timestamp: 3_000,
            downloaded_bytes: 600,
        },
    ];
    let metrics = download_progress_metrics(&job, &samples, 3_000);
    let details = progress_details_from_job_with_state(
        &job,
        "Download progress",
        &metrics,
        &ProgressPopupInteractionState::default(),
    );

    assert_eq!(metrics.average_speed, 200);
    assert_eq!(metrics.time_remaining, 2);
    assert_eq!(details.subtitle, "downloads.example.com");
    assert_eq!(details.destination, "C:/Downloads/Compressed/archive.zip");
    assert_eq!(details.progress_label, "60%");
    assert_eq!(details.speed_text, "200 B/s");
    assert_eq!(details.eta_text, "2s");
    assert_eq!(details.size_text, "1000 B");
    assert_eq!(details.status_tone, "active");
    assert!(details.can_pause);
    assert!(details.can_cancel);
    assert!(!details.can_open);
    assert!(!details.can_swap_to_browser);

    let updated_samples = record_progress_sample(Vec::new(), &job, 4_000);
    assert_eq!(updated_samples.len(), 1);
    assert_eq!(updated_samples[0].downloaded_bytes, 600);
}

#[test]
fn progress_details_expose_failed_browser_swap_and_completed_file_actions() {
    let failed = {
        let mut job = queue_job("failed", "failed.zip", TransferKind::Http, JobState::Failed);
        job.source = Some(DownloadSource {
            entry_point: "browser_download".into(),
            browser: "chrome".into(),
            extension_version: "0.3.52".into(),
            page_url: None,
            page_title: None,
            referrer: None,
            incognito: None,
        });
        job.error = Some("network failed".into());
        job
    };
    let failed_details = progress_details_from_job(&failed, "Download progress");

    assert_eq!(failed_details.status_tone, "error");
    assert!(failed_details.can_retry);
    assert!(failed_details.can_swap_to_browser);
    assert!(failed_details.can_close);
    assert_eq!(failed_details.error_text, "network failed");

    let completed = queue_job(
        "completed",
        "completed.zip",
        TransferKind::Http,
        JobState::Completed,
    );
    let completed_details = progress_details_from_job(&completed, "Download progress");

    assert_eq!(completed_details.status_tone, "success");
    assert!(completed_details.can_open);
    assert!(completed_details.can_reveal);
    assert!(completed_details.can_close);
    assert!(!completed_details.can_cancel);
}

#[test]
fn torrent_progress_helpers_match_react_torrent_popup_copy() {
    let mut torrent = queue_job(
        "torrent_1",
        "Example",
        TransferKind::Torrent,
        JobState::Downloading,
    );
    torrent.url = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=udp%3A%2F%2Ftracker.example%2Fannounce&tr=https%3A%2F%2Ftracker.example%2Fannounce".into();
    torrent.progress = 74.0;
    torrent.total_bytes = 4_400;
    torrent.downloaded_bytes = 3_200;
    torrent.speed = 64;
    torrent.eta = 10;
    torrent.torrent = Some(TorrentInfo {
        info_hash: Some("0123456789abcdef0123456789abcdef01234567".into()),
        name: Some("Example".into()),
        total_files: Some(3),
        peers: Some(28),
        seeds: Some(112),
        uploaded_bytes: 512,
        fetched_bytes: 3_200,
        ratio: 0.18,
        diagnostics: Some(TorrentRuntimeDiagnostics {
            session_upload_speed: 128,
            ..Default::default()
        }),
        ..Default::default()
    });

    assert_eq!(torrent_source_summary(&torrent), "DHT, 2 trackers");
    assert_eq!(
        torrent_info_hash(&torrent),
        "0123456789abcdef0123456789abcdef01234567"
    );
    assert_eq!(torrent_remaining_text(&torrent), "1.2 KiB remaining");
    assert_eq!(
        torrent_peer_health_dots(&torrent),
        vec![
            "success", "success", "success", "success", "success", "success", "warning", "warning",
            "muted", "muted", "muted", "muted"
        ]
    );

    let details = progress_details_from_job(&torrent, "Torrent session");
    assert_eq!(details.filename, "Example");
    assert_eq!(details.source_summary, "DHT, 2 trackers");
    assert_eq!(
        details.info_hash,
        "0123456789abcdef0123456789abcdef01234567"
    );
    assert_eq!(details.progress_label, "74%");
    assert_eq!(details.remaining_text, "1.2 KiB remaining");
    assert_eq!(details.upload_speed_text, "128 B/s");
    assert_eq!(details.peers_text, "28");
    assert_eq!(details.seeds_text, "112");
    assert_eq!(details.ratio_text, "0.18");
    assert_eq!(details.files_text, "3 files (4.3 KiB)");
}

#[test]
fn torrent_progress_labels_verified_file_checks_separately_from_peer_fetches() {
    let mut checking = queue_job(
        "torrent_checking",
        "ubuntu.iso",
        TransferKind::Torrent,
        JobState::Downloading,
    );
    checking.progress = 25.0;
    checking.downloaded_bytes = 256;
    checking.total_bytes = 1024;
    checking.torrent = Some(TorrentInfo {
        fetched_bytes: 0,
        ..Default::default()
    });

    assert_eq!(
        torrent_progress_strip_text(&checking),
        ("Verified 25%".into(), "Verified 256 B / 1.0 KiB".into())
    );
    let checking_details = progress_details_from_job(&checking, "Torrent session");
    assert_eq!(checking_details.state, "Checking files");
    assert_eq!(checking_details.progress_label, "Verified 25%");

    checking.torrent = Some(TorrentInfo {
        fetched_bytes: 736,
        ..Default::default()
    });
    assert_eq!(
        torrent_progress_strip_text(&checking),
        ("25%".into(), "256 B / 1.0 KiB".into())
    );
}

#[test]
fn batch_details_summarize_context_jobs_from_snapshot() {
    let snapshot = DesktopSnapshot {
        connection_state: ConnectionState::Connected,
        jobs: vec![
            DownloadJob {
                id: "job_a".into(),
                url: "https://example.com/a.bin".into(),
                filename: "a.bin".into(),
                source: None,
                transfer_kind: TransferKind::Http,
                integrity_check: None,
                torrent: None,
                state: JobState::Completed,
                created_at: 0,
                progress: 100.0,
                total_bytes: 100,
                downloaded_bytes: 100,
                speed: 0,
                eta: 0,
                error: None,
                failure_category: None,
                resume_support: Default::default(),
                retry_attempts: 0,
                target_path: "C:/Downloads/a.bin".into(),
                temp_path: "C:/Downloads/a.bin.part".into(),
                artifact_exists: None,
                bulk_archive: None,
            },
            DownloadJob {
                id: "job_b".into(),
                url: "https://example.com/b.bin".into(),
                filename: "b.bin".into(),
                source: None,
                transfer_kind: TransferKind::Http,
                integrity_check: None,
                torrent: None,
                state: JobState::Downloading,
                created_at: 0,
                progress: 50.0,
                total_bytes: 300,
                downloaded_bytes: 150,
                speed: 0,
                eta: 0,
                error: None,
                failure_category: None,
                resume_support: Default::default(),
                retry_attempts: 0,
                target_path: "C:/Downloads/b.bin".into(),
                temp_path: "C:/Downloads/b.bin.part".into(),
                artifact_exists: None,
                bulk_archive: None,
            },
        ],
        settings: Settings::default(),
    };
    let context = ProgressBatchContext {
        batch_id: "batch_1".into(),
        kind: ProgressBatchKind::Bulk,
        job_ids: vec!["job_a".into(), "job_b".into()],
        title: "Archive batch".into(),
        archive_name: Some("archive.zip".into()),
    };

    let details = batch_details_from_context(&context, &snapshot);

    assert_eq!(details.batch_id, "batch_1");
    assert_eq!(details.title, "Archive batch");
    assert_eq!(details.summary, "1 of 2 completed");
    assert_eq!(details.progress, 62.5);
    assert_eq!(details.bytes_text, "250 B / 400 B");
}

#[test]
fn batch_details_include_rows_actions_and_bulk_phase() {
    let archive = BulkArchiveInfo {
        id: "bulk_1".into(),
        name: "bundle.zip".into(),
        archive_status: BulkArchiveStatus::Compressing,
        output_path: None,
        error: None,
    };
    let snapshot = queue_snapshot(vec![
        {
            let mut job = queue_job("job_a", "a.bin", TransferKind::Http, JobState::Completed);
            job.total_bytes = 100;
            job.downloaded_bytes = 100;
            job.bulk_archive = Some(archive.clone());
            job
        },
        {
            let mut job = queue_job("job_b", "b.bin", TransferKind::Http, JobState::Completed);
            job.total_bytes = 300;
            job.downloaded_bytes = 300;
            job.bulk_archive = Some(archive);
            job
        },
    ]);
    let context = ProgressBatchContext {
        batch_id: "batch_bulk".into(),
        kind: ProgressBatchKind::Bulk,
        job_ids: vec!["job_a".into(), "job_b".into()],
        title: "Bulk download progress".into(),
        archive_name: Some("bundle.zip".into()),
    };

    let details = batch_details_from_context(&context, &snapshot);

    assert_eq!(details.display_title, "bundle.zip");
    assert_eq!(details.completed_count, 2);
    assert_eq!(details.failed_count, 0);
    assert_eq!(details.active_count, 0);
    assert_eq!(details.total_count, 2);
    assert_eq!(details.phase_id, "compressing");
    assert_eq!(details.phase_label, "Compressing archive");
    assert_eq!(details.phase_tone, "warning");
    assert!(!details.can_pause);
    assert!(!details.can_cancel);
    assert!(details.can_close);
    assert_eq!(details.rows.len(), 2);
    assert_eq!(details.rows[0].filename, "a.bin");
    assert_eq!(details.rows[0].status_text, "Completed");
    assert_eq!(details.rows[0].progress_label, "100%");
}

#[test]
fn queue_view_counts_and_filters_match_react_queue_views() {
    let snapshot = queue_snapshot(vec![
        queue_job(
            "http_active",
            "app.exe",
            TransferKind::Http,
            JobState::Downloading,
        ),
        queue_job(
            "http_done",
            "guide.pdf",
            TransferKind::Http,
            JobState::Completed,
        ),
        {
            let mut job = queue_job(
                "http_failed",
                "broken.zip",
                TransferKind::Http,
                JobState::Failed,
            );
            job.failure_category = Some(FailureCategory::Network);
            job
        },
        queue_job(
            "torrent_active",
            "linux.iso",
            TransferKind::Torrent,
            JobState::Downloading,
        ),
        {
            let mut job = queue_job(
                "torrent_seed",
                "movie.torrent",
                TransferKind::Torrent,
                JobState::Seeding,
            );
            job.torrent = Some(TorrentInfo {
                uploaded_bytes: 1024,
                fetched_bytes: 2048,
                ratio: 1.4,
                ..Default::default()
            });
            job
        },
        {
            let mut job = queue_job(
                "torrent_failed",
                "failed.torrent",
                TransferKind::Torrent,
                JobState::Failed,
            );
            job.failure_category = Some(FailureCategory::Torrent);
            job
        },
    ]);

    let model = queue_view_model_from_snapshot(&snapshot, &QueueUiState::default());

    assert_eq!(model.counts.all, 3);
    assert_eq!(model.counts.active, 1);
    assert_eq!(model.counts.attention, 1);
    assert_eq!(model.counts.completed, 1);
    assert_eq!(model.counts.categories.get(DownloadCategory::Document), 1);
    assert_eq!(model.counts.categories.get(DownloadCategory::Program), 1);
    assert_eq!(model.counts.categories.get(DownloadCategory::Compressed), 1);
    assert_eq!(model.counts.torrents.all, 3);
    assert_eq!(model.counts.torrents.active, 1);
    assert_eq!(model.counts.torrents.seeding, 1);
    assert_eq!(model.counts.torrents.attention, 1);
    assert_eq!(
        model
            .rows
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        vec!["http_active", "http_failed", "http_done"]
    );

    let torrent_state = QueueUiState {
        view: ViewFilter::TorrentSeeding,
        ..Default::default()
    };
    let torrent_model = queue_view_model_from_snapshot(&snapshot, &torrent_state);
    assert_eq!(
        torrent_model
            .rows
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        vec!["torrent_seed"]
    );
    assert!(torrent_model.footer_text.contains("1 seeding"));

    let category_state = QueueUiState {
        view: ViewFilter::Category(DownloadCategory::Document),
        ..Default::default()
    };
    let category_model = queue_view_model_from_snapshot(&snapshot, &category_state);
    assert_eq!(
        category_model
            .rows
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        vec!["http_done"]
    );
}

#[test]
fn queue_view_search_sort_and_row_action_flags_match_queue_contracts() {
    let snapshot = queue_snapshot(vec![
        {
            let mut job = queue_job("oldest", "alpha.zip", TransferKind::Http, JobState::Queued);
            job.created_at = 1_700_000_000_000;
            job.total_bytes = 10;
            job
        },
        {
            let mut job = queue_job(
                "newest",
                "beta.zip",
                TransferKind::Http,
                JobState::Downloading,
            );
            job.created_at = 1_800_000_000_000;
            job.total_bytes = 1_000;
            job.downloaded_bytes = 500;
            job.speed = 2048;
            job
        },
        {
            let mut job = queue_job("undated", "gamma.zip", TransferKind::Http, JobState::Failed);
            job.created_at = 0;
            job.total_bytes = 0;
            job.error = Some("network failed".into());
            job
        },
    ]);

    let model = queue_view_model_from_snapshot(&snapshot, &QueueUiState::default());
    assert_eq!(
        model.rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        vec!["oldest", "newest", "undated"],
        "default Slint queue sort should preserve React's date ascending order with undated jobs last"
    );
    assert!(model.rows[1].can_pause);
    assert!(!model.rows[1].can_remove);
    assert_eq!(model.rows[1].speed_text, "2.0 KiB/s");
    assert!(model.rows[2].can_retry);
    assert!(model.rows[2].status_detail.contains("network failed"));

    let size_state = QueueUiState {
        sort_mode: SortMode::new(SortColumn::Size, SortDirection::Desc),
        ..Default::default()
    };
    let size_model = queue_view_model_from_snapshot(&snapshot, &size_state);
    assert_eq!(
        size_model
            .rows
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        vec!["newest", "oldest", "undated"]
    );

    let search_state = QueueUiState {
        search_query: "beta".into(),
        ..Default::default()
    };
    let search_model = queue_view_model_from_snapshot(&snapshot, &search_state);
    assert_eq!(search_model.rows.len(), 1);
    assert_eq!(search_model.rows[0].id, "newest");
}

#[test]
fn queue_selection_prunes_hidden_jobs_and_supports_range_helpers() {
    let mut selection = SelectionState::default();
    selection.select_single("job_2");
    selection.toggle("job_4", true);
    selection.prune_to_visible(&["job_1".into(), "job_2".into(), "job_3".into()]);

    assert_eq!(selection.selected_ids(), vec!["job_2".to_string()]);
    assert_eq!(
        select_job_range(
            &[
                "job_1".into(),
                "job_2".into(),
                "job_3".into(),
                "job_4".into()
            ],
            "job_2",
            "job_4",
        ),
        vec![
            "job_2".to_string(),
            "job_3".to_string(),
            "job_4".to_string()
        ]
    );
    assert!(select_job_range(&["job_1".into()], "job_1", "missing").is_empty());
}

#[test]
fn delete_prompt_content_matches_react_copy_and_selection_rules() {
    let single = delete_prompt_content(1);
    assert_eq!(single.title, "Delete Download");
    assert_eq!(
        single.description,
        "Remove this download from the list. Disk deletion requires explicit confirmation below."
    );
    assert_eq!(single.checkbox_label, "Delete file from disk");
    assert_eq!(single.confirm_label, "Delete");
    assert_eq!(single.context_menu_label, "Delete");
    assert_eq!(single.selected_summary, "1 download selected");
    assert_eq!(
        single.missing_path_label,
        "No file path is recorded for this download."
    );

    let multi = delete_prompt_content(3);
    assert_eq!(multi.title, "Delete 3 Downloads");
    assert_eq!(multi.checkbox_label, "Delete selected files from disk");
    assert_eq!(multi.confirm_label, "Delete All");
    assert_eq!(multi.context_menu_label, "Delete All");
    assert_eq!(multi.selected_summary, "3 downloads selected");
    assert_eq!(delete_context_menu_label(0), "Delete");
    assert_eq!(delete_context_menu_label(2), "Delete All");
}

#[test]
fn delete_prompt_filters_active_jobs_and_defaults_paused_seeding_disk_delete() {
    let queued = queue_job("queued", "queued.zip", TransferKind::Http, JobState::Queued);
    let active = queue_job(
        "active",
        "active.zip",
        TransferKind::Http,
        JobState::Downloading,
    );
    let mut paused_seed = queue_job(
        "seed",
        "Seeded Torrent",
        TransferKind::Torrent,
        JobState::Paused,
    );
    paused_seed.target_path = "E:/Download/Other/Seeded Torrent".into();
    paused_seed.torrent = Some(TorrentInfo {
        uploaded_bytes: 2048,
        fetched_bytes: 4096,
        ratio: 0.5,
        seeding_started_at: Some(123_456),
        ..Default::default()
    });

    assert_eq!(
        delete_action_label_for_job(&paused_seed),
        "Delete from disk..."
    );
    assert!(default_delete_from_disk_for_jobs(&[paused_seed.clone()]));
    assert!(!default_delete_from_disk_for_jobs(std::slice::from_ref(
        &queued
    )));

    let prompt = delete_prompt_from_jobs(&[queued.clone(), active, paused_seed.clone()])
        .expect("delete prompt should include removable jobs");
    assert_eq!(
        prompt
            .jobs
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        vec!["queued", "seed"]
    );
    assert!(!prompt.delete_from_disk);
    assert_eq!(prompt.content.title, "Delete 2 Downloads");
}

#[test]
fn rename_filename_helpers_match_react_prompt_behavior() {
    let split = split_filename("archive.tar.gz");
    assert_eq!(split.base_name, "archive.tar");
    assert_eq!(split.extension, "gz");
    let no_extension = split_filename(".gitignore");
    assert_eq!(no_extension.base_name, ".gitignore");
    assert_eq!(no_extension.extension, "");

    assert_eq!(normalize_extension_input(" .z ip* "), ".zip");
    assert_eq!(normalize_extension_input("..tar"), "tar");
    assert_eq!(
        build_filename("  renamed  ", ".zip"),
        Some("renamed.zip".into())
    );
    assert_eq!(build_filename("  renamed  ", ""), Some("renamed".into()));
    assert_eq!(build_filename("   ", "zip"), None);
}

#[test]
fn queue_row_action_flags_match_react_queue_actions() {
    let paused = queue_job("paused", "paused.zip", TransferKind::Http, JobState::Paused);
    let canceled = queue_job(
        "canceled",
        "canceled.zip",
        TransferKind::Http,
        JobState::Canceled,
    );
    let failed_browser = {
        let mut job = queue_job("failed", "failed.zip", TransferKind::Http, JobState::Failed);
        job.source = Some(DownloadSource {
            entry_point: "browser_download".into(),
            browser: "chrome".into(),
            extension_version: "0.3.51".into(),
            page_url: None,
            page_title: None,
            referrer: None,
            incognito: None,
        });
        job
    };
    let downloading = queue_job(
        "downloading",
        "downloading.zip",
        TransferKind::Http,
        JobState::Downloading,
    );

    let paused_row = job_row_from_job(&paused);
    assert!(paused_row.can_resume);
    assert!(paused_row.can_rename);
    assert!(paused_row.can_remove);
    assert!(paused_row.can_open);
    assert!(paused_row.can_reveal);

    let canceled_row = job_row_from_job(&canceled);
    assert!(!canceled_row.can_resume);
    assert!(canceled_row.can_retry);
    assert!(!canceled_row.can_cancel);

    let failed_row = job_row_from_job(&failed_browser);
    assert!(!failed_row.can_resume);
    assert!(failed_row.can_retry);
    assert!(!failed_row.can_cancel);
    assert!(failed_row.can_swap_to_browser);

    let active_row = job_row_from_job(&downloading);
    assert!(active_row.can_pause);
    assert!(!active_row.can_remove);
    assert!(!active_row.can_rename);
    assert_eq!(active_row.delete_label, "Delete");
    assert_eq!(active_row.target_path, "C:/Downloads/downloading.zip");
}

fn queue_snapshot(jobs: Vec<DownloadJob>) -> DesktopSnapshot {
    DesktopSnapshot {
        connection_state: ConnectionState::Connected,
        jobs,
        settings: Settings::default(),
    }
}

fn queue_job(
    id: &str,
    filename: &str,
    transfer_kind: TransferKind,
    state: JobState,
) -> DownloadJob {
    DownloadJob {
        id: id.into(),
        url: format!("https://example.com/{filename}"),
        filename: filename.into(),
        source: None,
        transfer_kind,
        integrity_check: None,
        torrent: if transfer_kind == TransferKind::Torrent {
            Some(TorrentInfo::default())
        } else {
            None
        },
        state,
        created_at: 1,
        progress: if state == JobState::Completed {
            100.0
        } else {
            0.0
        },
        total_bytes: 100,
        downloaded_bytes: 0,
        speed: 0,
        eta: 0,
        error: None,
        failure_category: None,
        resume_support: ResumeSupport::Unknown,
        retry_attempts: 0,
        target_path: format!("C:/Downloads/{filename}"),
        temp_path: format!("C:/Downloads/{filename}.part"),
        artifact_exists: None,
        bulk_archive: None,
    }
}

fn diagnostics_snapshot() -> DiagnosticsSnapshot {
    DiagnosticsSnapshot {
        connection_state: ConnectionState::Connected,
        queue_summary: QueueSummary {
            total: 3,
            active: 1,
            attention: 1,
            queued: 1,
            downloading: 1,
            completed: 1,
            failed: 1,
        },
        last_host_contact_seconds_ago: Some(12),
        host_registration: HostRegistrationDiagnostics {
            status: HostRegistrationStatus::Configured,
            entries: vec![
                HostRegistrationEntry {
                    browser: "Chrome".into(),
                    registry_path: "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts".into(),
                    manifest_path: Some("C:/Users/Me/AppData/host.json".into()),
                    manifest_exists: true,
                    host_binary_path: Some(
                        "C:/Program Files/SimpleDownloadManager/native-host.exe".into(),
                    ),
                    host_binary_exists: true,
                },
                HostRegistrationEntry {
                    browser: "Firefox".into(),
                    registry_path: "HKCU\\Software\\Mozilla\\NativeMessagingHosts".into(),
                    manifest_path: None,
                    manifest_exists: false,
                    host_binary_path: None,
                    host_binary_exists: false,
                },
            ],
        },
        torrent_diagnostics: vec![TorrentJobDiagnostics {
            job_id: "torrent_1".into(),
            filename: "ubuntu.iso".into(),
            info_hash: Some("abc123".into()),
            diagnostics: TorrentRuntimeDiagnostics {
                queued_peers: 4,
                connecting_peers: 3,
                live_peers: 2,
                seen_peers: 8,
                dead_peers: 1,
                not_needed_peers: 0,
                contributing_peers: 1,
                peer_errors: 2,
                peers_with_errors: 1,
                peer_connection_attempts: 9,
                session_download_speed: 2048,
                session_upload_speed: 1024,
                average_piece_download_millis: Some(75),
                listen_port: Some(42_000),
                listener_fallback: true,
                peer_samples: vec![TorrentPeerDiagnostics {
                    state: "connecting".into(),
                    fetched_bytes: 512,
                    errors: 1,
                    downloaded_pieces: 2,
                    connection_attempts: 3,
                }],
            },
        }],
        recent_events: vec![
            DiagnosticEvent {
                timestamp: 0,
                level: DiagnosticLevel::Info,
                category: "download".into(),
                message: "Completed file.zip".into(),
                job_id: Some("job_1".into()),
            },
            DiagnosticEvent {
                timestamp: 1_700_000_000_000,
                level: DiagnosticLevel::Warning,
                category: "queue".into(),
                message: "Queued retry".into(),
                job_id: None,
            },
        ],
    }
}
