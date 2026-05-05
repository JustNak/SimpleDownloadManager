use super::*;
use crate::storage::{
    BulkArchiveOutputKind, DownloadJob, HandoffAuth, HandoffAuthHeader, JobState, TorrentInfo,
};
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
        output_kind: BulkArchiveOutputKind::Archive,
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
        vec![
            "downloads/file.txt".to_string(),
            "downloads/file (1).txt".to_string()
        ]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_bulk_archive_sync_rejects_missing_source() {
    let root = test_download_runtime_dir("bulk-archive-missing-source");
    let output_path = root.join("downloads.zip");
    let archive = BulkArchiveReady {
        archive_id: "bulk_2".into(),
        output_kind: BulkArchiveOutputKind::Archive,
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

#[test]
fn bulk_archive_source_plan_detects_multipart_rar_set() {
    let root = test_download_runtime_dir("bulk-archive-detect-rar-set");
    let entries = vec![
        crate::state::BulkArchiveEntry {
            source_path: root.join("Game.part01.rar"),
            archive_name: "Game.part01.rar".into(),
        },
        crate::state::BulkArchiveEntry {
            source_path: root.join("Game.part02.rar"),
            archive_name: "Game.part02.rar".into(),
        },
        crate::state::BulkArchiveEntry {
            source_path: root.join("Game.part03.rar"),
            archive_name: "Game.part03.rar".into(),
        },
    ];

    let plan = build_bulk_archive_source_plan(&entries).expect("rar parts should be planned");

    assert!(bulk_archive_needs_extraction(&entries));
    assert!(plan.raw_entries.is_empty());
    assert_eq!(plan.archive_sets.len(), 1);
    assert_eq!(
        plan.archive_sets[0].first_part.archive_name,
        "Game.part01.rar"
    );
    assert_eq!(
        plan.archive_sets[0]
            .members
            .iter()
            .map(|entry| entry.archive_name.as_str())
            .collect::<Vec<_>>(),
        vec!["Game.part01.rar", "Game.part02.rar", "Game.part03.rar"]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn bulk_archive_source_plan_detects_dot_001_set_and_missing_number() {
    let root = test_download_runtime_dir("bulk-archive-detect-001-set");
    let entries = vec![
        crate::state::BulkArchiveEntry {
            source_path: root.join("payload.001"),
            archive_name: "payload.001".into(),
        },
        crate::state::BulkArchiveEntry {
            source_path: root.join("payload.003"),
            archive_name: "payload.003".into(),
        },
    ];

    let error = build_bulk_archive_source_plan(&entries)
        .expect_err("missing payload.002 should fail the archive plan");

    assert!(error.contains("payload.002"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn bulk_archive_source_plan_keeps_non_archive_files_raw() {
    let root = test_download_runtime_dir("bulk-archive-detect-raw-files");
    let entries = vec![
        crate::state::BulkArchiveEntry {
            source_path: root.join("readme.txt"),
            archive_name: "readme.txt".into(),
        },
        crate::state::BulkArchiveEntry {
            source_path: root.join("cover.jpg"),
            archive_name: "cover.jpg".into(),
        },
    ];

    let plan = build_bulk_archive_source_plan(&entries).expect("raw files should be accepted");

    assert!(!bulk_archive_needs_extraction(&entries));
    assert!(plan.archive_sets.is_empty());
    assert_eq!(plan.raw_entries.len(), 2);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn prepare_bulk_archive_sources_extracts_multiple_sets_into_staging() {
    let root = test_download_runtime_dir("bulk-archive-prepare-extract");
    let archive = BulkArchiveReady {
        archive_id: "bulk_extract".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        output_path: root.join("bulk-download.zip"),
        entries: vec![
            archive_test_entry(&root, "Game.part01.rar", b"first"),
            archive_test_entry(&root, "Game.part02.rar", b"second"),
            archive_test_entry(&root, "Patch.001", b"patch-one"),
            archive_test_entry(&root, "Patch.002", b"patch-two"),
        ],
    };
    let extractor = RecordingArchiveExtractor::default();

    let prepared = prepare_bulk_archive_sources_with_extractor(archive, &extractor)
        .expect("archive sets should be extracted into staging");

    assert_eq!(
        extractor.calls.borrow().clone(),
        vec![root.join("Game.part01.rar"), root.join("Patch.001")]
    );
    assert_eq!(
        prepared
            .entries
            .iter()
            .map(|entry| entry.archive_name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "bulk-download/Game/content.bin",
            "bulk-download/Patch/content.bin"
        ]
    );
    assert_eq!(prepared.cleanup_paths.len(), 4);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn finish_bulk_archive_sources_zips_extracted_files_and_deletes_original_parts() {
    let root = test_download_runtime_dir("bulk-archive-finish-extracted");
    let archive = BulkArchiveReady {
        archive_id: "bulk_finish".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        output_path: root.join("bulk-download.zip"),
        entries: vec![
            archive_test_entry(&root, "Game.part01.rar", b"first"),
            archive_test_entry(&root, "Game.part02.rar", b"second"),
        ],
    };
    let source_part_1 = archive.entries[0].source_path.clone();
    let source_part_2 = archive.entries[1].source_path.clone();
    let extractor = RecordingArchiveExtractor::default();
    let prepared = prepare_bulk_archive_sources_with_extractor(archive, &extractor)
        .expect("archive set should be staged");

    let outcome = finish_prepared_bulk_archive_sync(prepared).expect("zip should be finalized");

    assert_eq!(
        zip_central_directory_names(&outcome.output_path),
        vec!["bulk-download/Game/content.bin"]
    );
    assert!(!source_part_1.exists());
    assert!(!source_part_2.exists());
    assert!(outcome.cleanup_warnings.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn finish_bulk_folder_sources_writes_extracted_files_and_deletes_original_parts() {
    let root = test_download_runtime_dir("bulk-folder-finish-extracted");
    let archive = BulkArchiveReady {
        archive_id: "bulk_folder_finish".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        output_path: root.join("Game"),
        entries: vec![
            archive_test_entry(&root, "Game.part01.rar", b"first"),
            archive_test_entry(&root, "Game.part02.rar", b"second"),
        ],
    };
    let source_part_1 = archive.entries[0].source_path.clone();
    let source_part_2 = archive.entries[1].source_path.clone();
    let extractor = RecordingArchiveExtractor::default();
    let prepared = prepare_bulk_archive_sources_with_extractor(archive, &extractor)
        .expect("archive set should be staged for folder output");

    let outcome =
        finish_prepared_bulk_archive_sync(prepared).expect("folder output should be finalized");

    assert!(outcome.output_path.is_dir());
    assert_eq!(
        std::fs::read(outcome.output_path.join("Game").join("content.bin")).unwrap(),
        b"Game"
    );
    assert!(!source_part_1.exists());
    assert!(!source_part_2.exists());
    assert!(outcome.cleanup_warnings.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn finish_bulk_folder_sources_copies_raw_files_and_deletes_originals() {
    let root = test_download_runtime_dir("bulk-folder-finish-raw");
    let readme = root.join("readme.txt");
    let cover = root.join("cover.jpg");
    std::fs::write(&readme, b"readme").unwrap();
    std::fs::write(&cover, b"cover").unwrap();
    let archive = BulkArchiveReady {
        archive_id: "bulk_folder_raw".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        output_path: root.join("Bundle"),
        entries: vec![
            crate::state::BulkArchiveEntry {
                source_path: readme.clone(),
                archive_name: "readme.txt".into(),
            },
            crate::state::BulkArchiveEntry {
                source_path: cover.clone(),
                archive_name: "cover.jpg".into(),
            },
        ],
    };
    let prepared = prepare_bulk_archive_sources_without_extraction(archive)
        .expect("raw files should be prepared for folder output");

    let outcome =
        finish_prepared_bulk_archive_sync(prepared).expect("folder output should copy raw files");

    assert_eq!(
        std::fs::read(outcome.output_path.join("readme.txt")).unwrap(),
        b"readme"
    );
    assert_eq!(
        std::fs::read(outcome.output_path.join("cover.jpg")).unwrap(),
        b"cover"
    );
    assert!(!readme.exists());
    assert!(!cover.exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cleanup_failures_warn_without_failing_completed_zip() {
    let root = test_download_runtime_dir("bulk-archive-cleanup-warning");
    let source = root.join("source.txt");
    std::fs::write(&source, b"payload").unwrap();
    let prepared = PreparedBulkArchive {
        output_kind: BulkArchiveOutputKind::Archive,
        output_path: root.join("bulk-download.zip"),
        entries: vec![crate::state::BulkArchiveEntry {
            source_path: source,
            archive_name: "source.txt".into(),
        }],
        cleanup_paths: vec![root.join("missing.part01.rar")],
        staging_root: None,
    };

    let outcome = finish_prepared_bulk_archive_sync(prepared)
        .expect("cleanup warnings should not fail a completed zip");

    assert_eq!(
        zip_central_directory_names(&outcome.output_path),
        vec!["source.txt"]
    );
    assert_eq!(outcome.cleanup_warnings.len(), 1);
    assert!(outcome.cleanup_warnings[0].contains("missing.part01.rar"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn single_stream_http_drops_writer_before_finalizing_download() {
    let source = include_str!("http.rs");
    let drop_index = source
        .find("drop(file);")
        .expect("single-stream HTTP path should explicitly drop the completed writer");
    let finalize_index = source
        .find("move_to_final_path(&task.temp_path, &target_path)")
        .expect("single-stream HTTP path should finalize the downloaded file");

    assert!(
        drop_index < finalize_index,
        "download file handle should be released before finalizing and triggering bulk extraction"
    );
}

#[test]
fn seven_zip_failure_messages_are_user_readable() {
    let source = Path::new("Game.part01.rar");

    assert_eq!(
        seven_zip_failure_message(source, Some(2), "ERROR: Wrong password"),
        "Archive extraction failed for Game.part01.rar: password is required or incorrect."
    );
    assert_eq!(
        seven_zip_failure_message(source, Some(2), "ERROR: CRC Failed"),
        "Archive extraction failed for Game.part01.rar: archive data failed CRC validation."
    );
    assert_eq!(
        seven_zip_failure_message(source, Some(2), "ERROR: Missing volume"),
        "Archive extraction failed for Game.part01.rar: one or more archive parts are missing."
    );
    assert_eq!(
        seven_zip_failure_message(
            source,
            Some(2),
            "ERROR: The process cannot access the file because it is being used by another process."
        ),
        "Archive extraction failed for Game.part01.rar: downloaded archive part is still locked by another process. Retry archive creation in a moment."
    );
    assert!(
        seven_zip_failure_message(source, Some(7), "unexpected failure")
            .contains("7-Zip exited with code 7")
    );
}

#[test]
fn archive_extraction_retries_transient_file_locks() {
    let root = test_download_runtime_dir("bulk-archive-lock-retry");
    let archive = BulkArchiveReady {
        archive_id: "bulk_lock_retry".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        output_path: root.join("bulk-download.zip"),
        entries: vec![
            archive_test_entry(&root, "Game.part01.rar", b"first"),
            archive_test_entry(&root, "Game.part02.rar", b"second"),
        ],
    };
    let extractor = LockOnceArchiveExtractor::default();

    let prepared = prepare_bulk_archive_sources_with_extractor(archive, &extractor)
        .expect("transient lock should be retried before failing extraction");

    assert_eq!(*extractor.calls.borrow(), 2);
    assert_eq!(prepared.entries.len(), 1);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn zip_central_directory_entry_uses_zip64_extra_for_large_sizes_and_offsets() {
    let entry = ZipCentralDirectoryEntry {
        name: "large.bin".into(),
        crc32: 0x1234_5678,
        compressed_size: u64::from(u32::MAX) + 1,
        uncompressed_size: u64::from(u32::MAX) + 1,
        local_header_offset: u64::from(u32::MAX) + 24,
    };
    let mut zip = Vec::new();

    write_zip_central_directory_entry(&mut zip, &entry).unwrap();

    assert_eq!(&zip[0..4], &[0x50, 0x4b, 0x01, 0x02]);
    assert_eq!(u16::from_le_bytes(zip[6..8].try_into().unwrap()), 45);
    assert_eq!(
        u32::from_le_bytes(zip[20..24].try_into().unwrap()),
        u32::MAX
    );
    assert_eq!(
        u32::from_le_bytes(zip[24..28].try_into().unwrap()),
        u32::MAX
    );
    assert_eq!(
        u32::from_le_bytes(zip[42..46].try_into().unwrap()),
        u32::MAX
    );

    let name_len = u16::from_le_bytes(zip[28..30].try_into().unwrap()) as usize;
    let extra_len = u16::from_le_bytes(zip[30..32].try_into().unwrap()) as usize;
    let extra_start = 46 + name_len;
    let extra = &zip[extra_start..extra_start + extra_len];
    assert_eq!(u16::from_le_bytes(extra[0..2].try_into().unwrap()), 0x0001);
    assert_eq!(u16::from_le_bytes(extra[2..4].try_into().unwrap()), 24);
    assert_eq!(
        u64::from_le_bytes(extra[4..12].try_into().unwrap()),
        u64::from(u32::MAX) + 1
    );
    assert_eq!(
        u64::from_le_bytes(extra[12..20].try_into().unwrap()),
        u64::from(u32::MAX) + 1
    );
    assert_eq!(
        u64::from_le_bytes(extra[20..28].try_into().unwrap()),
        u64::from(u32::MAX) + 24
    );
}

#[test]
fn zip_end_of_central_directory_emits_zip64_locator_when_zip32_limits_are_exceeded() {
    let mut zip = std::io::Cursor::new(Vec::new());

    write_zip_end_of_central_directory(
        &mut zip,
        usize::from(u16::MAX) + 1,
        u64::from(u32::MAX) + 128,
        u64::from(u32::MAX) + 256,
    )
    .unwrap();

    let bytes = zip.into_inner();
    assert!(
        bytes
            .windows(4)
            .any(|window| window == [0x50, 0x4b, 0x06, 0x06]),
        "ZIP64 end of central directory record should be present"
    );
    assert!(
        bytes
            .windows(4)
            .any(|window| window == [0x50, 0x4b, 0x06, 0x07]),
        "ZIP64 end of central directory locator should be present"
    );
    let eocd_index = bytes
        .windows(4)
        .rposition(|window| window == [0x50, 0x4b, 0x05, 0x06])
        .unwrap();
    assert_eq!(
        u16::from_le_bytes(bytes[eocd_index + 10..eocd_index + 12].try_into().unwrap()),
        u16::MAX
    );
    assert_eq!(
        u32::from_le_bytes(bytes[eocd_index + 12..eocd_index + 16].try_into().unwrap()),
        u32::MAX
    );
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
        target_path: PathBuf::from(job.target_path),
        temp_path: PathBuf::from(job.temp_path),
    };

    let prepared = prepare_torrent_source_for_task(&task, app_data_dir);

    assert_eq!(prepared.source_kind, TorrentSourceKind::Magnet);
    assert!(prepared.source.starts_with(&magnet));

    let _ = std::fs::remove_dir_all(app_data_dir);
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

#[test]
fn preflight_metadata_uses_head_headers() {
    let metadata = derive_preflight_metadata_from_parts(
        Some(4_096),
        Some("bytes"),
        Some("attachment; filename=\"server-report.pdf\""),
        "https://example.com/download",
        EntityValidators::default(),
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
            validators: EntityValidators {
                etag: None,
                last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
            },
        }),
        PreflightMetadata {
            total_bytes: Some(128),
            resume_support: ResumeSupport::Supported,
            filename: Some("probe.bin".into()),
            validators: EntityValidators {
                etag: Some("\"probe\"".into()),
                last_modified: None,
            },
        },
    );

    assert_eq!(merged.total_bytes, Some(128));
    assert_eq!(merged.filename.as_deref(), Some("head.bin"));
    assert_eq!(merged.validators.etag.as_deref(), Some("\"probe\""));
    assert_eq!(
        merged.validators.last_modified.as_deref(),
        Some("Wed, 21 Oct 2015 07:28:00 GMT")
    );
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

    let _response = send_request(&client, &url, 0, None, None).await.unwrap();
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

    let response = send_request(&client, &url, 0, Some(&auth), None)
        .await
        .unwrap();
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

#[tokio::test]
async fn segment_state_preserves_progress_when_validators_match() {
    let root = test_download_runtime_dir("segment-validator-match");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let validators = EntityValidators {
        etag: Some("\"abc123\"".into()),
        last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
    };
    let mut state = new_segment_state_for_test(&plan, validators.clone());
    state.segments[0].downloaded_bytes = 4;
    state.segments[0].completed = true;
    tokio::fs::write(&temp_path, b"abcdefghijkl").await.unwrap();
    persist_segment_state(&temp_path, &state).await.unwrap();

    let reloaded = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .unwrap();

    assert_eq!(reloaded.validators, validators);
    assert_eq!(reloaded.segments[0].downloaded_bytes, 4);
    assert!(reloaded.segments[0].completed);
    assert!(temp_path.exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segment_state_resets_progress_when_validators_change() {
    let root = test_download_runtime_dir("segment-validator-changed");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let mut state = new_segment_state_for_test(
        &plan,
        EntityValidators {
            etag: Some("\"old\"".into()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
        },
    );
    state.segments[0].downloaded_bytes = 4;
    state.segments[0].completed = true;
    tokio::fs::write(&temp_path, b"abcdefghijkl").await.unwrap();
    persist_segment_state(&temp_path, &state).await.unwrap();

    let next_validators = EntityValidators {
        etag: Some("\"new\"".into()),
        last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
    };
    let reloaded = load_or_create_segment_state(&temp_path, &plan, &next_validators)
        .await
        .unwrap();

    assert_eq!(reloaded.validators, next_validators);
    assert!(reloaded
        .segments
        .iter()
        .all(|segment| { segment.downloaded_bytes == 0 && !segment.completed }));
    assert!(!temp_path.exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segment_state_keeps_old_progress_when_stored_validators_are_missing() {
    let root = test_download_runtime_dir("segment-validator-missing");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let mut state = new_segment_state_for_test(&plan, EntityValidators::default());
    state.segments[1].downloaded_bytes = 2;
    tokio::fs::write(&temp_path, b"abcdefghijkl").await.unwrap();
    persist_segment_state(&temp_path, &state).await.unwrap();

    let next_validators = EntityValidators {
        etag: Some("\"new\"".into()),
        last_modified: None,
    };
    let reloaded = load_or_create_segment_state(&temp_path, &plan, &next_validators)
        .await
        .unwrap();

    assert_eq!(reloaded.validators, next_validators);
    assert_eq!(reloaded.segments[1].downloaded_bytes, 2);
    assert!(temp_path.exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn range_request_sends_if_range_when_resume_validator_is_available() {
    let response = concat!(
        "HTTP/1.1 206 Partial Content\r\n",
        "Content-Range: bytes 4-7/12\r\n",
        "Content-Length: 4\r\n",
        "\r\n",
        "efgh"
    );
    let (url, request_handle) = spawn_one_response_server(response).await;
    let client = download_client().unwrap();
    let validators = EntityValidators {
        etag: Some("\"abc123\"".into()),
        last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
    };

    let _response = send_range_request(
        &client,
        &url,
        ByteRange { start: 4, end: 7 },
        None,
        Some(&validators),
    )
    .await
    .unwrap();
    let request = request_handle.await.unwrap();
    let request_lower = request.to_ascii_lowercase();

    assert!(request_lower.contains("range: bytes=4-7"));
    assert!(request_lower.contains("if-range: \"abc123\""));
}

#[tokio::test]
async fn segment_worker_resumes_partial_range_into_existing_file() {
    let root = test_download_runtime_dir("segment-worker-resume");
    let temp_path = root.join("download.bin.part");
    let response = concat!(
        "HTTP/1.1 206 Partial Content\r\n",
        "Content-Range: bytes 4-11/12\r\n",
        "Content-Length: 8\r\n",
        "\r\n",
        "efghijkl"
    );
    let (url, request_handle) = spawn_one_response_server(response).await;
    let validators = EntityValidators {
        etag: Some("\"segment-source\"".into()),
        last_modified: None,
    };
    let segment = SegmentProgress {
        index: 0,
        range: ByteRange { start: 0, end: 11 },
        downloaded_bytes: 4,
        completed: false,
    };
    let mut stored = SegmentedDownloadState {
        total_bytes: 12,
        validators: validators.clone(),
        segments: vec![segment.clone()],
    };
    prepare_direct_segment_file(&temp_path, 12).await.unwrap();
    let mut file = open_direct_segment_file(&temp_path).await.unwrap();
    write_segment_chunk_to(&mut file, 0, b"abcd").await.unwrap();
    drop(file);

    let mut job = torrent_job("job_segment_resume", JobState::Downloading);
    job.transfer_kind = TransferKind::Http;
    job.torrent = None;
    job.temp_path = temp_path.display().to_string();
    job.target_path = root.join("download.bin").display().to_string();
    let state = SharedState::for_tests(test_storage_path("segment-worker-resume-state"), vec![job]);
    stored.segments[0].downloaded_bytes = 4;
    let context = SegmentWorkerContext {
        state,
        client: download_client().unwrap(),
        job_id: "job_segment_resume".into(),
        url,
        handoff_auth: None,
        temp_path: temp_path.clone(),
        total_bytes: 12,
        profile: performance_profile(DownloadPerformanceMode::Balanced),
        validators,
        progress: Arc::new(SegmentedProgressCounters::new(vec![4])),
        metadata: Arc::new(Mutex::new(stored)),
    };

    let outcome = download_segment_worker(context, segment).await.unwrap();
    let request = request_handle.await.unwrap();
    let request_lower = request.to_ascii_lowercase();
    let bytes = tokio::fs::read(&temp_path).await.unwrap();

    assert_eq!(outcome, DownloadOutcome::Completed);
    assert!(request_lower.contains("range: bytes=4-11"));
    assert!(request_lower.contains("if-range: \"segment-source\""));
    assert_eq!(bytes, b"abcdefghijkl");

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segment_worker_collector_returns_on_first_error() {
    let mut workers = tokio::task::JoinSet::new();
    workers.spawn(async {
        Err(download_error(
            FailureCategory::Network,
            "segment failed quickly".into(),
            true,
        ))
    });
    workers.spawn(async {
        tokio::time::sleep(Duration::from_secs(5)).await;
        Ok(DownloadOutcome::Completed)
    });

    let started = Instant::now();
    let (_outcome, error) = await_segment_workers(workers).await;

    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(
        error
            .expect("first worker error should be returned")
            .message,
        "segment failed quickly"
    );
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
        total_bytes: plan.total_bytes,
        validators,
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
    calls: std::cell::RefCell<Vec<PathBuf>>,
}

impl ArchiveExtractor for RecordingArchiveExtractor {
    fn extract(&self, first_part: &Path, output_dir: &Path) -> Result<(), String> {
        self.calls.borrow_mut().push(first_part.to_path_buf());
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

#[derive(Default)]
struct LockOnceArchiveExtractor {
    calls: std::cell::RefCell<usize>,
}

impl ArchiveExtractor for LockOnceArchiveExtractor {
    fn extract(&self, first_part: &Path, output_dir: &Path) -> Result<(), String> {
        let mut calls = self.calls.borrow_mut();
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

    let validators = EntityValidators::default();
    let mut state = load_or_create_segment_state(&temp_path, &plan, &validators)
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
    let reloaded = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .unwrap();
    assert_eq!(reloaded.segments[0].downloaded_bytes, 0);
    assert!(!reloaded.segments[0].completed);
    assert_eq!(reloaded.segments[1].downloaded_bytes, 0);
    assert!(!reloaded.segments[1].completed);

    let _ = tokio::fs::remove_dir_all(root).await;
}
