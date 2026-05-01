use simple_download_manager_desktop_core::contracts::{
    AddJobResult, AddJobStatus, AddJobsResult, ProgressBatchContext, ProgressBatchKind,
};
use simple_download_manager_desktop_core::storage::{
    ConnectionState, DesktopSnapshot, DownloadJob, DownloadPrompt, DownloadSource, FailureCategory,
    JobState, ResumeSupport, Settings, TorrentInfo, TransferKind,
};
use simple_download_manager_desktop_slint::controller::{
    active_download_urls, add_download_outcome_for_result, batch_details_from_context,
    build_filename, default_delete_from_disk_for_jobs, delete_action_label_for_job,
    delete_context_menu_label, delete_prompt_content, delete_prompt_from_jobs,
    download_ready_detail, download_ready_label, download_submit_label,
    ensure_trailing_editable_line, infer_transfer_kind_for_url, job_row_from_job,
    normalize_archive_name, normalize_extension_input, parse_download_url_lines,
    progress_details_from_job, prompt_details_from_prompt, queue_view_model_from_snapshot,
    select_job_range, split_filename, status_text_from_snapshot, validate_optional_sha256,
    AddDownloadFormState, AddDownloadProgressIntent, AddDownloadResult, DownloadCategory,
    DownloadMode, QueueUiState, SelectionState, SortColumn, SortDirection, SortMode, ViewFilter,
};

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
