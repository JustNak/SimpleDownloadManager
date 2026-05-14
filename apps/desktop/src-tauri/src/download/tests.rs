use super::*;
use crate::storage::{
    BulkArchiveOutputKind, BulkHosterAccelerationMode, DownloadJob, HandoffAuth, HandoffAuthHeader,
    JobState, TorrentInfo, TorrentPeerConnectionWatchdogMode,
};
use std::future::pending;
use std::sync::{Mutex as TestMutex, MutexGuard as TestMutexGuard, OnceLock as TestOnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[path = "tests/recovery.rs"]
mod recovery;
#[path = "tests/scenarios.rs"]
mod scenarios;

static SEGMENT_HOST_SCORE_TEST_LOCK: TestOnceLock<TestMutex<()>> = TestOnceLock::new();

fn segment_host_score_test_guard() -> TestMutexGuard<'static, ()> {
    SEGMENT_HOST_SCORE_TEST_LOCK
        .get_or_init(|| TestMutex::new(()))
        .lock()
        .unwrap()
}

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
    assert_eq!(
        unavailable.http_status,
        Some(StatusCode::SERVICE_UNAVAILABLE)
    );

    let not_found = error_for_http_status(StatusCode::NOT_FOUND, false);
    assert_eq!(not_found.category, FailureCategory::Http);
    assert!(!not_found.retryable);
    assert_eq!(not_found.http_status, Some(StatusCode::NOT_FOUND));
}

#[test]
fn http_response_errors_preserve_retry_after_for_segment_reconnects() {
    let mut headers = HeaderMap::new();
    headers.insert(RETRY_AFTER, HeaderValue::from_static("120"));

    let error = error_for_http_response(StatusCode::TOO_MANY_REQUESTS, &headers, false);

    assert_eq!(error.http_status, Some(StatusCode::TOO_MANY_REQUESTS));
    assert_eq!(error.retry_after, Some(MAX_RETRY_AFTER_DELAY));
    assert_eq!(
        segment_reconnect_delay_for_error(
            &error,
            1,
            "job_rate_limited",
            "https://cdn.example.com/file.bin"
        ),
        MAX_RETRY_AFTER_DELAY
    );
}

#[test]
fn rate_limited_segment_reconnects_use_stable_jitter_without_retry_after() {
    let error = error_for_http_status(StatusCode::TOO_MANY_REQUESTS, false);
    let first =
        segment_reconnect_delay_for_error(&error, 2, "job_a", "https://cdn.example.com/file.bin");
    let second =
        segment_reconnect_delay_for_error(&error, 2, "job_b", "https://cdn.example.com/file.bin");

    assert!(first >= REQUEST_RETRY_DELAYS[1]);
    assert!(first <= REQUEST_RETRY_DELAYS[1] + MAX_RETRY_JITTER);
    assert_ne!(
        first, second,
        "rate-limited segment reconnects should be de-synchronized"
    );
}

#[test]
fn hoster_refresh_retries_expired_links_range_failures_and_early_eof() {
    let forbidden = error_for_http_status(StatusCode::FORBIDDEN, false);
    assert!(hoster_refresh_error_allows_retry(&forbidden));

    let gone = error_for_http_status(StatusCode::GONE, false);
    assert!(hoster_refresh_error_allows_retry(&gone));

    let range_rejected = download_error(
        FailureCategory::Resume,
        "The remote server rejected the resume request.".into(),
        false,
    );
    assert!(hoster_refresh_error_allows_retry(&range_rejected));

    let early_eof = download_error(
        FailureCategory::Network,
        "Download ended early. Received 1024 of 4096 bytes.".into(),
        true,
    );
    assert!(hoster_refresh_error_allows_retry(&early_eof));

    let integrity = download_error(
        FailureCategory::Integrity,
        "Downloaded file checksum did not match.".into(),
        false,
    );
    assert!(!hoster_refresh_error_allows_retry(&integrity));
}

#[tokio::test]
async fn download_client_does_not_decode_mislabelled_file_bodies() {
    let response = "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\n\r\nbad";
    let (url, _request_handle) = spawn_one_response_server(response).await;
    let client = download_client().unwrap();

    let response = send_request(&client, &url, 0, None, None)
        .await
        .expect("mislabelled file response should still start");
    let bytes = response
        .bytes()
        .await
        .expect("download client should stream raw file bytes without decompression");

    assert_eq!(&bytes[..], b"bad");
}

#[tokio::test]
async fn response_body_decode_errors_are_retryable_network_failures() {
    let response = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nbad\r\n0\r\n\r\n";
    let (url, _request_handle) = spawn_one_response_server(response).await;
    let client = Client::builder().redirect(Policy::none()).build().unwrap();
    let response = client.get(&url).send().await.unwrap();
    let error = response
        .bytes()
        .await
        .expect_err("reqwest should reject malformed transfer-encoding bodies");

    let classified = download_stream_error(error);

    assert_eq!(classified.category, FailureCategory::Network);
    assert!(classified.retryable);
}

#[test]
fn hoster_refresh_before_attempt_fails_closed_instead_of_using_source_url() {
    let source = include_str!("http.rs");

    assert!(source.contains("Err(error) => return Err(error),"));
    assert!(!source.contains("Ok(None) | Err(_) => task.url.clone()"));
}

#[test]
fn hoster_refresh_preserves_resolution_retryability() {
    let terminal = crate::hosters::HosterResolutionError {
        code: "HOSTER_RESOLUTION_FAILED",
        message: "DataNodes captcha-protected downloads are not supported.".into(),
        retryable: false,
    };
    let terminal_error = hoster_resolution_download_error(terminal);
    assert_eq!(terminal_error.category, FailureCategory::Http);
    assert!(!terminal_error.retryable);

    let transient = crate::hosters::HosterResolutionError {
        code: "HOSTER_RESOLUTION_FAILED",
        message: "hoster resolver: HTTP 503 Service Unavailable.".into(),
        retryable: true,
    };
    let transient_error = hoster_resolution_download_error(transient);
    assert_eq!(transient_error.category, FailureCategory::Http);
    assert!(transient_error.retryable);
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
fn bulk_archive_source_plan_detects_legacy_rar_volumes() {
    let root = test_download_runtime_dir("bulk-archive-detect-legacy-rar");
    let entries = vec![
        archive_test_entry(&root, "Game.rar", b"first"),
        archive_test_entry(&root, "Game.r00", b"second"),
        archive_test_entry(&root, "Game.r01", b"third"),
    ];

    let plan = build_bulk_archive_source_plan(&entries).expect("legacy rar volumes should group");

    assert_eq!(plan.raw_entries.len(), 0);
    assert_eq!(plan.archive_sets.len(), 1);
    assert_eq!(plan.archive_sets[0].first_part.archive_name, "Game.rar");
    assert_eq!(
        plan.archive_sets[0]
            .members
            .iter()
            .map(|entry| entry.archive_name.as_str())
            .collect::<Vec<_>>(),
        vec!["Game.rar", "Game.r00", "Game.r01"]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn bulk_archive_source_plan_rejects_missing_legacy_rar_volume() {
    let root = test_download_runtime_dir("bulk-archive-missing-legacy-rar");
    let entries = vec![
        archive_test_entry(&root, "Game.rar", b"first"),
        archive_test_entry(&root, "Game.r01", b"third"),
    ];

    let error = build_bulk_archive_source_plan(&entries)
        .expect_err("missing legacy rar volumes should fail before extraction");

    assert!(error.contains("Game.r00"));

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
        vec!["Game/content.bin", "Patch/content.bin"]
    );
    assert_eq!(prepared.cleanup_paths.len(), 4);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn legacy_archive_extracted_sources_finalize_as_folder_and_delete_original_parts() {
    let root = test_download_runtime_dir("bulk-archive-finish-extracted");
    let archive = BulkArchiveReady {
        archive_id: "bulk_finish".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        output_path: root.join("bulk-download"),
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
fn finish_bulk_folder_sources_moves_raw_files_without_cleanup_warnings() {
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
        finish_prepared_bulk_archive_sync(prepared).expect("folder output should move raw files");

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
fn legacy_archive_output_kind_finalizes_as_folder_output() {
    let root = test_download_runtime_dir("bulk-folder-legacy-archive-output");
    let readme = root.join("readme.txt");
    let cover = root.join("cover.jpg");
    std::fs::write(&readme, b"readme").unwrap();
    std::fs::write(&cover, b"cover").unwrap();
    let archive = BulkArchiveReady {
        archive_id: "bulk_legacy_archive".into(),
        output_kind: BulkArchiveOutputKind::Archive,
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
        .expect("legacy archive output should normalize to folder preparation");

    let outcome =
        finish_prepared_bulk_archive_sync(prepared).expect("legacy archive output should finalize");

    assert!(outcome.output_path.is_dir());
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
    assert!(outcome.cleanup_warnings.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn folder_combine_failure_keeps_original_sources_and_removes_incomplete_output() {
    let root = test_download_runtime_dir("bulk-folder-finish-failure-keeps-sources");
    let source = root.join("source.txt");
    let missing = root.join("missing.txt");
    std::fs::write(&source, b"source").unwrap();
    let prepared = PreparedBulkArchive {
        output_path: root.join("Bundle"),
        entries: vec![
            crate::state::BulkArchiveEntry {
                source_path: source.clone(),
                archive_name: "source.txt".into(),
            },
            crate::state::BulkArchiveEntry {
                source_path: missing,
                archive_name: "missing.txt".into(),
            },
        ],
        cleanup_paths: vec![source.clone()],
        staging_root: None,
    };

    let error = finish_prepared_bulk_archive_sync(prepared)
        .expect_err("missing second source should fail folder finalization");

    assert!(error.contains("missing.txt"));
    assert_eq!(std::fs::read(&source).unwrap(), b"source");
    assert!(!root.join("Bundle").exists());
    assert!(
        extracting_staging_dirs(&root).is_empty(),
        "failed folder finalization should remove incomplete temp output folders"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn folder_combine_removes_stale_staging_dirs_for_same_output() {
    let root = test_download_runtime_dir("bulk-folder-removes-stale-staging");
    let source = root.join("source.txt");
    std::fs::write(&source, b"source").unwrap();
    let output_path = root.join("Bundle");
    let stale_staging = root.join(".Bundle.extracting-111-222");
    std::fs::create_dir_all(&stale_staging).unwrap();
    std::fs::write(stale_staging.join("old.tmp"), b"old").unwrap();
    let prepared = PreparedBulkArchive {
        output_path: output_path.clone(),
        entries: vec![crate::state::BulkArchiveEntry {
            source_path: source.clone(),
            archive_name: "source.txt".into(),
        }],
        cleanup_paths: vec![source],
        staging_root: None,
    };

    let outcome = finish_prepared_bulk_archive_sync(prepared)
        .expect("folder finalization should clean stale staging and complete");

    assert_eq!(outcome.output_path, output_path);
    assert!(!stale_staging.exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn bulk_file_operation_retries_transient_failures() {
    let calls = std::cell::Cell::new(0);

    retry_bulk_file_operation("test copy", || {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "The process cannot access the file because it is being used by another process.",
            ));
        }
        Ok(())
    })
    .expect("transient file operation should be retried");

    assert_eq!(calls.get(), 2);
}

#[test]
fn duplicate_raw_output_paths_fail_before_sources_are_deleted() {
    let root = test_download_runtime_dir("bulk-folder-duplicate-raw-paths");
    let source_a = root.join("source-a.txt");
    let source_b = root.join("source-b.txt");
    std::fs::write(&source_a, b"alpha").unwrap();
    std::fs::write(&source_b, b"bravo").unwrap();
    let archive = BulkArchiveReady {
        archive_id: "bulk_duplicate_raw".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        output_path: root.join("Bundle"),
        entries: vec![
            crate::state::BulkArchiveEntry {
                source_path: source_a.clone(),
                archive_name: "same.txt".into(),
            },
            crate::state::BulkArchiveEntry {
                source_path: source_b.clone(),
                archive_name: "same.txt".into(),
            },
        ],
    };

    let error = prepare_bulk_archive_sources_without_extraction(archive)
        .expect_err("duplicate output paths should be rejected before writing");

    assert!(error.contains("Duplicate bulk output path"));
    assert_eq!(std::fs::read(&source_a).unwrap(), b"alpha");
    assert_eq!(std::fs::read(&source_b).unwrap(), b"bravo");
    assert!(!root.join("Bundle").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn bulk_finalization_plan_counts_bytes_and_uses_move_for_folder_output() {
    let root = test_download_runtime_dir("bulk-finalization-plan-folder");
    let readme = root.join("readme.txt");
    let cover = root.join("cover.jpg");
    std::fs::write(&readme, b"readme").unwrap();
    std::fs::write(&cover, b"cover").unwrap();
    let archive = BulkArchiveReady {
        archive_id: "bulk_folder_plan".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        output_path: root.join("Bundle"),
        entries: vec![
            crate::state::BulkArchiveEntry {
                source_path: readme,
                archive_name: "readme.txt".into(),
            },
            crate::state::BulkArchiveEntry {
                source_path: cover,
                archive_name: "cover.jpg".into(),
            },
        ],
    };

    let plan = bulk_finalization_plan(&archive).expect("folder plan should be built");

    assert_eq!(plan.total_completed_bytes, 11);
    assert_eq!(plan.finalize_mode, BulkFinalizeMode::Move);
    assert_eq!(plan.output_kind, BulkArchiveOutputKind::Folder);
    assert!(!plan.requires_extraction);
    assert_eq!(plan.scratch_space_bytes, 0);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn bulk_finalization_plan_normalizes_huge_requests_to_folder_output() {
    let root = test_download_runtime_dir("bulk-finalization-plan-huge-folder");
    let source = root.join("huge.bin");
    let file = std::fs::File::create(&source).unwrap();
    file.set_len(HUGE_BULK_ARCHIVE_THRESHOLD_BYTES).unwrap();
    let archive = BulkArchiveReady {
        archive_id: "bulk_zip_plan".into(),
        output_kind: BulkArchiveOutputKind::Archive,
        output_path: root.join("huge"),
        entries: vec![crate::state::BulkArchiveEntry {
            source_path: source,
            archive_name: "huge.bin".into(),
        }],
    };

    let plan = bulk_finalization_plan(&archive).expect("zip plan should be built");

    assert_eq!(
        plan.total_completed_bytes,
        HUGE_BULK_ARCHIVE_THRESHOLD_BYTES
    );
    assert_eq!(plan.output_kind, BulkArchiveOutputKind::Folder);
    assert_eq!(plan.finalize_mode, BulkFinalizeMode::Move);
    assert!(
        plan.warning
            .as_deref()
            .is_some_and(|warning| warning.contains("100 GiB")),
        "huge bulk folder finalization should carry an early warning"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn async_bulk_finalization_planning_counts_sources_before_prepare() {
    let root = test_download_runtime_dir("bulk-finalization-plan-async");
    let first = archive_test_entry(&root, "first.bin", b"first");
    let second = archive_test_entry(&root, "second.bin", b"second");
    let archive = BulkArchiveReady {
        archive_id: "bulk_async_plan".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        output_path: root.join("Bundle"),
        entries: vec![first, second],
    };

    let plan = plan_bulk_archive_finalization(archive)
        .await
        .expect("async bulk plan should be built before source preparation");

    assert_eq!(plan.total_completed_bytes, 11);
    assert_eq!(plan.output_kind, BulkArchiveOutputKind::Folder);
    assert_eq!(plan.finalize_mode, BulkFinalizeMode::Move);
    assert!(!plan.requires_extraction);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cleanup_failures_warn_without_failing_completed_folder() {
    let root = test_download_runtime_dir("bulk-archive-cleanup-warning");
    let source = root.join("source.txt");
    std::fs::write(&source, b"payload").unwrap();
    let prepared = PreparedBulkArchive {
        output_path: root.join("bulk-download"),
        entries: vec![crate::state::BulkArchiveEntry {
            source_path: source,
            archive_name: "source.txt".into(),
        }],
        cleanup_paths: vec![root.join("missing.part01.rar")],
        staging_root: None,
    };

    let outcome = finish_prepared_bulk_archive_sync(prepared)
        .expect("cleanup warnings should not fail a completed folder");

    assert!(outcome.output_path.is_dir());
    assert_eq!(
        std::fs::read(outcome.output_path.join("source.txt")).unwrap(),
        b"payload"
    );
    assert_eq!(outcome.cleanup_warnings.len(), 1);
    assert!(outcome.cleanup_warnings[0].contains("missing.part01.rar"));

    let _ = std::fs::remove_dir_all(root);
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
fn extraction_uses_isolated_staging_directory_per_archive_set() {
    let root = test_download_runtime_dir("bulk-archive-isolated-extract-sets");
    let archive = BulkArchiveReady {
        archive_id: "bulk_isolated_extract".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        output_path: root.join("bulk-download"),
        entries: vec![
            archive_test_entry(&root, "Game.part01.rar", b"first"),
            archive_test_entry(&root, "Game.part02.rar", b"second"),
            archive_test_entry(&root, "Patch.001", b"patch-one"),
            archive_test_entry(&root, "Patch.002", b"patch-two"),
        ],
    };
    let extractor = RecordingArchiveExtractor::default();

    let prepared = prepare_bulk_archive_sources_with_extractor(archive, &extractor)
        .expect("archive sets should extract into isolated staging directories");

    let output_dirs = extractor.output_dirs.borrow();
    assert_eq!(output_dirs.len(), 2);
    assert_ne!(output_dirs[0], output_dirs[1]);
    assert!(output_dirs[0].ends_with("set-0"));
    assert!(output_dirs[1].ends_with("set-1"));
    assert_eq!(
        prepared
            .entries
            .iter()
            .map(|entry| entry.archive_name.as_str())
            .collect::<Vec<_>>(),
        vec!["Game/content.bin", "Patch/content.bin"]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn duplicate_extracted_output_paths_fail_cleanly() {
    let root = test_download_runtime_dir("bulk-archive-duplicate-extracted-paths");
    let archive = BulkArchiveReady {
        archive_id: "bulk_duplicate_extracted".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        output_path: root.join("bulk-download"),
        entries: vec![
            archive_test_entry(&root, "Game.part01.rar", b"first"),
            archive_test_entry(&root, "Game.part02.rar", b"second"),
            archive_test_entry(&root, "Patch.001", b"patch-one"),
            archive_test_entry(&root, "Patch.002", b"patch-two"),
        ],
    };
    let original_parts = archive
        .entries
        .iter()
        .map(|entry| entry.source_path.clone())
        .collect::<Vec<_>>();
    let extractor = FlatContentArchiveExtractor;

    let error = prepare_bulk_archive_sources_with_extractor(archive, &extractor)
        .expect_err("duplicate extracted output paths should fail");

    assert!(error.contains("Duplicate bulk output path"));
    for path in original_parts {
        assert!(
            path.exists(),
            "source part should remain after failed extraction planning"
        );
    }
    assert!(
        extracting_staging_dirs(&root).is_empty(),
        "failed extraction planning should remove staging directories"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn extracted_symlink_entries_are_rejected() {
    let root = test_download_runtime_dir("bulk-archive-reject-extracted-symlink");
    let archive = BulkArchiveReady {
        archive_id: "bulk_symlink_extract".into(),
        output_kind: BulkArchiveOutputKind::Folder,
        output_path: root.join("bulk-download"),
        entries: vec![archive_test_entry(&root, "Game.rar", b"archive")],
    };
    let extractor = SymlinkArchiveExtractor;

    let error = match prepare_bulk_archive_sources_with_extractor(archive, &extractor) {
        Ok(_) => panic!("extracted symlink should be rejected"),
        Err(error) if error == "symlink creation is not available in this test environment" => {
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        Err(error) => error,
    };

    assert!(error.contains("Unsupported extracted archive entry"));

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

    let cleanup_retry = torrent_metadata_recovery_stage(1);
    assert!(!cleanup_retry.use_tracker_first);
    assert!(!cleanup_retry.reset_engine_before_retry);
    assert_eq!(cleanup_retry.timeout, TORRENT_METADATA_TIMEOUT);

    let reset_retry = torrent_metadata_recovery_stage(2);
    assert!(!reset_retry.use_tracker_first);
    assert!(reset_retry.reset_engine_before_retry);
    assert_eq!(reset_retry.timeout, TORRENT_METADATA_TIMEOUT);

    assert!(torrent_metadata_recovery_stage(3).is_final_failure);
}

#[test]
fn torrent_metadata_recovery_final_message_distinguishes_skipped_engine_reset() {
    let reset_attempted = torrent_metadata_recovery_failure_message(true);
    assert!(reset_attempted.contains("engine-reset recovery attempts"));
    assert!(!reset_attempted.contains("could not reset"));

    let reset_skipped = torrent_metadata_recovery_failure_message(false);
    assert!(reset_skipped.contains("could not reset the torrent engine"));
    assert!(reset_skipped.contains("another torrent was active"));
    assert!(reset_skipped.contains("Pause other torrents"));
}

#[test]
fn torrent_metadata_recovery_final_error_is_terminal_torrent_failure() {
    let error = torrent_metadata_recovery_failure_error(false);

    assert_eq!(error.category, FailureCategory::Torrent);
    assert!(!error.retryable);
    assert!(error.message.contains("could not reset the torrent engine"));
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
    let source = include_str!("torrent.rs");
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
        tracker_first_metadata_diagnostic_message(&TrackerFirstMetadataOutcome::Resolved),
        "Tracker-first torrent metadata resolved"
    );
    assert_eq!(
            tracker_first_metadata_diagnostic_message(&TrackerFirstMetadataOutcome::TimedOut),
            "Tracker-first torrent metadata timed out after 8 seconds; falling back to the main DHT session"
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
fn torrent_peer_watchdog_recover_mode_refreshes_readds_then_resets_once() {
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
        TorrentPeerConnectionWatchdogDecision::ReaddTorrent
    );
    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(180)),
        TorrentPeerConnectionWatchdogDecision::ResetEngine
    );
    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(300)),
        TorrentPeerConnectionWatchdogDecision::Report,
        "recover mode should not keep refreshing, re-adding, or resetting the same job attempt"
    );
}

#[test]
fn torrent_peer_watchdog_rearms_engine_reset_after_skip() {
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
        TorrentPeerConnectionWatchdogDecision::ReaddTorrent
    );
    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(180)),
        TorrentPeerConnectionWatchdogDecision::ResetEngine
    );

    watchdog.rearm_engine_reset();

    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(239)),
        TorrentPeerConnectionWatchdogDecision::Continue
    );
    assert_eq!(
        watchdog.observe(&update, started_at + Duration::from_secs(240)),
        TorrentPeerConnectionWatchdogDecision::ResetEngine,
        "a skipped reset should be attempted again after another stall window"
    );
}

#[test]
fn torrent_peer_watchdog_defaults_to_recover_mode() {
    assert_eq!(
        TorrentPeerConnectionWatchdogMode::default(),
        TorrentPeerConnectionWatchdogMode::Recover
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
        removal_state: None,
        created_at: 1,
        progress: 0.0,
        total_bytes: 0,
        downloaded_bytes: 0,
        speed: 0,
        eta: 0,
        active_segments: None,
        planned_segments: None,
        error: None,
        failure_category: None,
        resume_support: ResumeSupport::Unknown,
        retry_attempts: 0,
        auto_restart_attempts: 0,
        resolved_from_url: None,
        hoster_preflight: None,
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

#[tokio::test]
async fn stream_wait_observes_canceled_control_before_next_chunk_arrives() {
    let job = DownloadJob {
        id: "job_cancel_wait".into(),
        url: "https://example.com/file.bin".into(),
        filename: "file.bin".into(),
        source: None,
        transfer_kind: TransferKind::Http,
        integrity_check: None,
        torrent: None,
        state: JobState::Canceled,
        removal_state: None,
        created_at: 1,
        progress: 0.0,
        total_bytes: 0,
        downloaded_bytes: 0,
        speed: 0,
        eta: 0,
        active_segments: None,
        planned_segments: None,
        error: None,
        failure_category: None,
        resume_support: ResumeSupport::Unknown,
        retry_attempts: 0,
        auto_restart_attempts: 0,
        resolved_from_url: None,
        hoster_preflight: None,
        target_path: "C:/Downloads/file.bin".into(),
        temp_path: test_storage_path("stream-wait-cancel-part")
            .display()
            .to_string(),
        artifact_exists: None,
        bulk_archive: None,
    };
    let state = SharedState::for_tests(test_storage_path("stream-wait-cancel-state"), vec![job]);
    let result = next_stream_item_with_control(
        &state,
        "job_cancel_wait",
        None,
        std::future::pending::<Option<Result<(), ()>>>(),
    )
    .await;

    assert!(matches!(
        result,
        StreamItemWait::Interrupted(DownloadOutcome::Canceled)
    ));
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
fn fast_profile_uses_fast_plus_initial_and_adaptive_segment_caps() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    assert_eq!(profile.initial_segments, 16);
    assert_eq!(profile.soft_max_segments, 32);
    assert_eq!(profile.max_segments, 64);
    assert_eq!(profile.target_segment_size, 8 * 1024 * 1024);
    assert_eq!(profile.adaptive_ramp_step, 4);
    assert_eq!(profile.adaptive_ramp_interval, Duration::from_secs(2));
}

#[test]
fn fast_range_plan_uses_fast_plus_initial_fanout() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let minimum_plan =
        plan_segmented_ranges(16 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("fast mode should segment range-capable files at 16 MiB");
    let ramp_plan =
        plan_segmented_ranges(256 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("large fast downloads should immediately fan out across the fast initial cap");
    let capped_plan =
        plan_segmented_ranges(1024 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("large fast downloads should use capped segmented downloading");

    assert_eq!(minimum_plan.segments.len(), 2);
    assert_eq!(ramp_plan.segments.len(), 16);
    assert_eq!(capped_plan.segments.len(), 16);
}

#[test]
fn fast_dynamic_queue_depth_keeps_ranges_long_lived_past_mid_download() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);

    assert_eq!(dynamic_segment_queue_depth(profile), 80);
}

#[test]
fn fast_tail_lease_size_uses_remaining_byte_buckets() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let mib = 1024 * 1024;
    let gib = 1024 * mib;

    assert_eq!(
        dynamic_segment_tail_lease_size(2 * gib, profile),
        Some(32 * mib)
    );
    assert_eq!(dynamic_segment_tail_lease_size(gib, profile), Some(8 * mib));
    assert_eq!(
        dynamic_segment_tail_lease_size(256 * mib, profile),
        Some(8 * mib)
    );
    assert_eq!(
        dynamic_segment_tail_lease_size(255 * mib, profile),
        Some(mib)
    );
    assert_eq!(
        dynamic_segment_tail_lease_size(
            2 * gib,
            performance_profile(DownloadPerformanceMode::Balanced)
        ),
        None
    );
}

#[test]
fn fast_tail_leasing_splits_clean_ranges_into_fixed_leases() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let mib = 1024 * 1024;
    let gib = 1024 * mib;
    let total_bytes = 2 * gib;
    let mut state = segmented_state_for_test(total_bytes, vec![(0, total_bytes - 1, 0, false)]);
    let mut active = HashSet::new();

    let claimed = claim_largest_dynamic_segment_for_profile_tests(
        &mut state,
        &mut active,
        dynamic_segment_queue_depth(profile),
        profile,
    )
    .expect("tail leasing should claim a clean lease");

    assert_eq!(
        claimed.range,
        ByteRange {
            start: 0,
            end: 32 * mib - 1
        }
    );
    assert_eq!(
        state
            .segments
            .iter()
            .map(|segment| segment.range.len())
            .sum::<u64>(),
        total_bytes
    );
    assert!(state
        .segments
        .iter()
        .all(|segment| segment.range.len() <= 32 * mib));
}

#[test]
fn fast_tail_leasing_does_not_split_partial_pending_ranges() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let mib = 1024 * 1024;
    let mut state = segmented_state_for_test(96 * mib, vec![(0, 96 * mib - 1, 4 * mib, false)]);
    let mut active = HashSet::new();

    let claimed = claim_largest_dynamic_segment_for_profile_tests(
        &mut state,
        &mut active,
        dynamic_segment_queue_depth(profile),
        profile,
    )
    .expect("partial pending range should still be claimed");

    assert_eq!(state.segments.len(), 1);
    assert_eq!(
        claimed.range,
        ByteRange {
            start: 0,
            end: 96 * mib - 1
        }
    );
    assert_eq!(claimed.downloaded_bytes, 4 * mib);
}

#[test]
fn fast_tail_stage_keeps_pending_one_mib_leases_available() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let mib = 1024 * 1024;
    let mut state = segmented_state_for_test(128 * mib, vec![(0, 128 * mib - 1, 0, false)]);
    let active = HashSet::new();

    fill_dynamic_segment_queue_for_profile_tests(
        &mut state,
        &active,
        dynamic_segment_queue_depth(profile),
        profile,
    );

    let pending_one_mib_leases = state
        .segments
        .iter()
        .filter(|segment| segment.range.len() <= mib)
        .count();
    assert!(pending_segment_count(&state, &active) >= profile.max_segments);
    assert!(pending_one_mib_leases >= profile.max_segments);
    assert_eq!(
        state
            .segments
            .iter()
            .map(|segment| segment.range.len())
            .sum::<u64>(),
        128 * mib
    );
}

#[test]
fn fast_tail_leasing_splits_large_clean_ranges_even_when_queue_is_full() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let mib = 1024 * 1024;
    let total_bytes = 80 * 64 * mib;
    let mut state = segmented_state_for_test(
        total_bytes,
        (0_u64..80)
            .map(|index| {
                let start = index * 64 * mib;
                (start, start + 64 * mib - 1, 0, false)
            })
            .collect(),
    );
    let mut active = HashSet::new();

    let claimed = claim_largest_dynamic_segment_for_profile_tests(
        &mut state,
        &mut active,
        dynamic_segment_queue_depth(profile),
        profile,
    )
    .expect("clean resumed ranges should be leased before claim");

    assert_eq!(claimed.range.len(), 32 * mib);
    assert!(
        state
            .segments
            .iter()
            .filter(|segment| segment.range.len() <= 32 * mib)
            .count()
            >= dynamic_segment_queue_depth(profile)
    );
    assert_eq!(
        state
            .segments
            .iter()
            .map(|segment| segment.range.len())
            .sum::<u64>(),
        total_bytes
    );
}

#[test]
fn range_plan_respects_segment_connection_budget() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let capped_plan = plan_segmented_ranges_with_budget(
        1024 * 1024 * 1024,
        ResumeSupport::Supported,
        None,
        profile,
        Some(8),
    )
    .expect("available segment budget should still allow segmented downloading");

    assert_eq!(capped_plan.segments.len(), 8);
    assert!(plan_segmented_ranges_with_budget(
        1024 * 1024 * 1024,
        ResumeSupport::Supported,
        None,
        profile,
        Some(1),
    )
    .is_none());
}

#[test]
fn dynamic_segment_queue_splits_largest_pending_ranges_before_claim() {
    let mut state = SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 64,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: vec![SegmentProgress {
            index: 0,
            range: ByteRange { start: 0, end: 63 },
            downloaded_bytes: 0,
            completed: false,
        }],
    };
    let mut active = HashSet::new();

    let claimed = claim_largest_dynamic_segment_for_tests(&mut state, &mut active, 4, 8)
        .expect("dynamic queue should claim a segment");

    assert_eq!(claimed.range, ByteRange { start: 0, end: 15 });
    assert!(active.contains(&claimed.index));
    assert_eq!(
        state
            .segments
            .iter()
            .map(|segment| segment.range)
            .collect::<Vec<_>>(),
        vec![
            ByteRange { start: 0, end: 15 },
            ByteRange { start: 16, end: 31 },
            ByteRange { start: 32, end: 47 },
            ByteRange { start: 48, end: 63 },
        ]
    );
}

#[test]
fn dynamic_segment_queue_does_not_reassign_completed_spans() {
    let mut state = SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 64,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: vec![
            SegmentProgress {
                index: 0,
                range: ByteRange { start: 0, end: 15 },
                downloaded_bytes: 16,
                completed: true,
            },
            SegmentProgress {
                index: 1,
                range: ByteRange { start: 16, end: 63 },
                downloaded_bytes: 0,
                completed: false,
            },
        ],
    };
    let mut active = HashSet::new();

    let claimed = claim_largest_dynamic_segment_for_tests(&mut state, &mut active, 4, 8)
        .expect("dynamic queue should claim unfinished work");

    assert_ne!(claimed.index, 0);
    assert!(claimed.range.start >= 16);
    assert!(state.segments[0].completed);
    assert_eq!(state.segments[0].downloaded_bytes, 16);
}

fn adaptive_admission_for_test(
    name: &str,
    admitted_workers: usize,
    active_count: usize,
    previous_downloaded: u64,
    current_downloaded: u64,
    previous_bps: u64,
) -> (
    PathBuf,
    Arc<AtomicBool>,
    Arc<AtomicUsize>,
    AdaptiveSegmentAdmission,
) {
    let root = test_download_runtime_dir(name);
    let temp_path = root.join("download.part");
    let mut job = torrent_job(name, JobState::Downloading);
    job.transfer_kind = TransferKind::Http;
    job.torrent = None;
    job.temp_path = temp_path.display().to_string();
    job.target_path = root.join("download.bin").display().to_string();
    let state = SharedState::for_tests(test_storage_path(name), vec![job]);
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let progress = Arc::new(SegmentedProgressCounters::new(vec![0; 96]));
    progress.store_segment_bytes(0, current_downloaded);
    let active_segments = Arc::new(Mutex::new((0_usize..active_count).collect::<HashSet<_>>()));
    let ramp_blocked = Arc::new(AtomicBool::new(false));
    let target_workers = Arc::new(AtomicUsize::new(admitted_workers));
    let metadata = Arc::new(Mutex::new(SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 1024 * 1024 * 1024,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: (0_u64..96)
            .map(|index| SegmentProgress {
                index: index as usize,
                range: ByteRange {
                    start: index * 8 * 1024 * 1024,
                    end: ((index + 1) * 8 * 1024 * 1024).saturating_sub(1),
                },
                downloaded_bytes: 0,
                completed: false,
            })
            .collect(),
    }));
    let context = SegmentWorkerContext {
        state,
        client: download_client().unwrap(),
        job_id: name.into(),
        url: "https://cdn.example.com/file.bin".into(),
        segment_pressure_key: "https://cdn.example.com:443".into(),
        handoff_auth: None,
        temp_path,
        total_bytes: 1024 * 1024 * 1024,
        profile,
        validators: EntityValidators::default(),
        progress: progress.clone(),
        metadata: metadata.clone(),
        metadata_persisted_at: Arc::new(Mutex::new(Instant::now())),
        stop: Arc::new(AtomicBool::new(false)),
        control_signal: WorkerControlSignal::default(),
        ramp_blocked: ramp_blocked.clone(),
        priority_throttle: Arc::new(Mutex::new(DynamicThrottleState::default())),
        priority_throttle_enabled: false,
        stall_timeout: None,
        reconnects: Arc::new(SegmentReconnectTracker::default()),
        target_workers: target_workers.clone(),
        active_workers: Arc::new(AtomicUsize::new(admitted_workers)),
    };
    let admission = AdaptiveSegmentAdmission {
        context,
        active_segments,
        metadata,
        progress,
        admitted_workers: Arc::new(AtomicUsize::new(admitted_workers)),
        target_workers: target_workers.clone(),
        segment_lease: None,
        queue_depth: 128,
        min_split_size: 1024 * 1024,
        last_ramp_total_bytes: AtomicU64::new(previous_downloaded),
        last_ramp_speed_bps: AtomicU64::new(previous_bps),
        regression_windows: AtomicUsize::new(0),
    };

    (root, ramp_blocked, target_workers, admission)
}

#[tokio::test]
async fn adaptive_segment_admission_blocks_after_throughput_regression() {
    let root = test_download_runtime_dir("adaptive-ramp-regression");
    let temp_path = root.join("download.part");
    let mut job = torrent_job("job_adaptive_regression", JobState::Downloading);
    job.transfer_kind = TransferKind::Http;
    job.torrent = None;
    job.temp_path = temp_path.display().to_string();
    job.target_path = root.join("download.bin").display().to_string();
    let state = SharedState::for_tests(test_storage_path("adaptive-ramp-regression"), vec![job]);
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let progress = Arc::new(SegmentedProgressCounters::new(vec![0; 96]));
    progress.store_segment_bytes(0, 128 * 1024 * 1024);
    let active_segments = Arc::new(Mutex::new((0_usize..31).collect::<HashSet<_>>()));
    let ramp_blocked = Arc::new(AtomicBool::new(false));
    let target_workers = Arc::new(AtomicUsize::new(32));
    let metadata = Arc::new(Mutex::new(SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 1024 * 1024 * 1024,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: (0_u64..96)
            .map(|index| SegmentProgress {
                index: index as usize,
                range: ByteRange {
                    start: index * 8 * 1024 * 1024,
                    end: ((index + 1) * 8 * 1024 * 1024).saturating_sub(1),
                },
                downloaded_bytes: 0,
                completed: false,
            })
            .collect(),
    }));
    let context = SegmentWorkerContext {
        state,
        client: download_client().unwrap(),
        job_id: "job_adaptive_regression".into(),
        url: "https://cdn.example.com/file.bin".into(),
        segment_pressure_key: "https://cdn.example.com:443".into(),
        handoff_auth: None,
        temp_path,
        total_bytes: 1024 * 1024 * 1024,
        profile,
        validators: EntityValidators::default(),
        progress: progress.clone(),
        metadata: metadata.clone(),
        metadata_persisted_at: Arc::new(Mutex::new(Instant::now())),
        stop: Arc::new(AtomicBool::new(false)),
        control_signal: WorkerControlSignal::default(),
        ramp_blocked: ramp_blocked.clone(),
        priority_throttle: Arc::new(Mutex::new(DynamicThrottleState::default())),
        priority_throttle_enabled: false,
        stall_timeout: None,
        reconnects: Arc::new(SegmentReconnectTracker::default()),
        target_workers: target_workers.clone(),
        active_workers: Arc::new(AtomicUsize::new(32)),
    };
    let admission = AdaptiveSegmentAdmission {
        context,
        active_segments,
        metadata,
        progress,
        admitted_workers: Arc::new(AtomicUsize::new(32)),
        target_workers,
        segment_lease: None,
        queue_depth: 128,
        min_split_size: 1024 * 1024,
        last_ramp_total_bytes: AtomicU64::new(120 * 1024 * 1024),
        last_ramp_speed_bps: AtomicU64::new(64 * 1024 * 1024),
        regression_windows: AtomicUsize::new(0),
    };

    assert!(!admission.can_admit_more().await);
    assert!(ramp_blocked.load(Ordering::Relaxed));

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn adaptive_segment_admission_blocks_after_moderate_throughput_regression() {
    let root = test_download_runtime_dir("adaptive-ramp-moderate-regression");
    let temp_path = root.join("download.part");
    let mut job = torrent_job("job_adaptive_moderate_regression", JobState::Downloading);
    job.transfer_kind = TransferKind::Http;
    job.torrent = None;
    job.temp_path = temp_path.display().to_string();
    job.target_path = root.join("download.bin").display().to_string();
    let state = SharedState::for_tests(
        test_storage_path("adaptive-ramp-moderate-regression"),
        vec![job],
    );
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let progress = Arc::new(SegmentedProgressCounters::new(vec![0; 96]));
    progress.store_segment_bytes(0, 154 * 1024 * 1024);
    let active_segments = Arc::new(Mutex::new((0_usize..31).collect::<HashSet<_>>()));
    let ramp_blocked = Arc::new(AtomicBool::new(false));
    let target_workers = Arc::new(AtomicUsize::new(32));
    let metadata = Arc::new(Mutex::new(SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 1024 * 1024 * 1024,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: (0_u64..96)
            .map(|index| SegmentProgress {
                index: index as usize,
                range: ByteRange {
                    start: index * 8 * 1024 * 1024,
                    end: ((index + 1) * 8 * 1024 * 1024).saturating_sub(1),
                },
                downloaded_bytes: 0,
                completed: false,
            })
            .collect(),
    }));
    let context = SegmentWorkerContext {
        state,
        client: download_client().unwrap(),
        job_id: "job_adaptive_moderate_regression".into(),
        url: "https://cdn.example.com/file.bin".into(),
        segment_pressure_key: "https://cdn.example.com:443".into(),
        handoff_auth: None,
        temp_path,
        total_bytes: 1024 * 1024 * 1024,
        profile,
        validators: EntityValidators::default(),
        progress: progress.clone(),
        metadata: metadata.clone(),
        metadata_persisted_at: Arc::new(Mutex::new(Instant::now())),
        stop: Arc::new(AtomicBool::new(false)),
        control_signal: WorkerControlSignal::default(),
        ramp_blocked: ramp_blocked.clone(),
        priority_throttle: Arc::new(Mutex::new(DynamicThrottleState::default())),
        priority_throttle_enabled: false,
        stall_timeout: None,
        reconnects: Arc::new(SegmentReconnectTracker::default()),
        target_workers: target_workers.clone(),
        active_workers: Arc::new(AtomicUsize::new(32)),
    };
    let admission = AdaptiveSegmentAdmission {
        context,
        active_segments,
        metadata,
        progress,
        admitted_workers: Arc::new(AtomicUsize::new(32)),
        target_workers,
        segment_lease: None,
        queue_depth: 128,
        min_split_size: 1024 * 1024,
        last_ramp_total_bytes: AtomicU64::new(120 * 1024 * 1024),
        last_ramp_speed_bps: AtomicU64::new(40 * 1024 * 1024),
        regression_windows: AtomicUsize::new(0),
    };

    assert!(!admission.can_admit_more().await);
    assert!(ramp_blocked.load(Ordering::Relaxed));

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn fast_adaptive_sustain_holds_at_soft_cap_without_clear_improvement() {
    let (root, ramp_blocked, target_workers, admission) = adaptive_admission_for_test(
        "adaptive-ramp-soft-cap-hold",
        32,
        31,
        120 * 1024 * 1024,
        204 * 1024 * 1024,
        40 * 1024 * 1024,
    );

    assert!(!admission.can_admit_more().await);
    assert!(!ramp_blocked.load(Ordering::Relaxed));
    assert_eq!(target_workers.load(Ordering::Relaxed), 32);

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn fast_adaptive_sustain_allows_peak_cap_after_clear_improvement() {
    let (root, ramp_blocked, target_workers, admission) = adaptive_admission_for_test(
        "adaptive-ramp-soft-cap-grow",
        32,
        31,
        120 * 1024 * 1024,
        208 * 1024 * 1024,
        40 * 1024 * 1024,
    );

    assert!(admission.can_admit_more().await);
    assert!(!ramp_blocked.load(Ordering::Relaxed));
    assert_eq!(target_workers.load(Ordering::Relaxed), 32);

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn dynamic_segment_claim_does_not_persist_queue_splits_immediately() {
    let root = test_download_runtime_dir("dynamic-claim-no-persist");
    let temp_path = root.join("download.part");
    let metadata = Arc::new(Mutex::new(SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 64,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: vec![SegmentProgress {
            index: 0,
            range: ByteRange { start: 0, end: 63 },
            downloaded_bytes: 0,
            completed: false,
        }],
    }));
    let active = Arc::new(Mutex::new(HashSet::new()));

    let claimed = claim_dynamic_segment_work(&temp_path, &metadata, &active, 4, 8, None)
        .await
        .unwrap()
        .expect("dynamic queue should claim a segment");

    assert_eq!(claimed.range, ByteRange { start: 0, end: 15 });
    assert!(
        !segment_meta_path(&temp_path).exists(),
        "claiming split work should leave sidecar persistence to the shared persist cadence"
    );
    assert_eq!(metadata.lock().await.segments.len(), 4);

    let _ = tokio::fs::remove_dir_all(root).await;
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
    assert!(plan_segmented_ranges(
        256 * 1024 * 1024,
        ResumeSupport::Supported,
        Some(1024),
        performance_profile(DownloadPerformanceMode::Fast),
    )
    .is_none());
}

#[test]
fn unverified_hoster_bulk_tasks_disallow_segmented_downloads() {
    let task = http_segment_policy_task(true, Some("https://example.com/source"));

    assert!(!task_allows_segmented_download(&task));
}

#[test]
fn fuckingfast_hoster_bulk_tasks_allow_safe_segmented_downloads() {
    let mut task = http_segment_policy_task(
        true,
        Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar"),
    );
    task.url = "https://dl.fuckingfast.co/dl/token/Game.part01.rar".into();

    assert!(task_allows_segmented_download(&task));
}

#[test]
fn datanodes_hoster_bulk_tasks_allow_safe_segmented_downloads() {
    let mut task = http_segment_policy_task(
        true,
        Some("https://datanodes.to/abc123456789/fg-optional-bonus-content.bin"),
    );
    task.url = "https://s42.datanodes.to/d/abc123456789/fg-optional-bonus-content.bin".into();

    assert!(task_allows_segmented_download(&task));
}

#[test]
fn hoster_acceleration_off_disallows_verified_hoster_bulk_segmentation() {
    let mut task = http_segment_policy_task(
        true,
        Some("https://datanodes.to/abc123456789/fg-optional-bonus-content.bin"),
    );
    task.url = "https://s42.datanodes.to/d/abc123456789/fg-optional-bonus-content.bin".into();

    assert!(!task_allows_segmented_download_with_mode(
        &task,
        BulkHosterAccelerationMode::Off
    ));

    let mut fuckingfast_task = http_segment_policy_task(
        true,
        Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar"),
    );
    fuckingfast_task.url = "https://dl.fuckingfast.co/dl/token/Game.part01.rar".into();

    assert!(!task_allows_segmented_download_with_mode(
        &fuckingfast_task,
        BulkHosterAccelerationMode::Off
    ));
}

#[test]
fn hoster_acceleration_caps_segments_by_performance_mode() {
    let policy = crate::hosters::HosterAccelerationPolicy {
        backoff_key: "hoster:datanodes:abc123456789".into(),
        balanced_initial_segments: 4,
        balanced_max_segments: 4,
        fast_initial_segments: 6,
        fast_max_segments: 10,
    };

    assert_eq!(
        hoster_initial_segment_cap_for_mode(&policy, DownloadPerformanceMode::Stable),
        1
    );
    assert_eq!(
        hoster_initial_segment_cap_for_mode(&policy, DownloadPerformanceMode::Balanced),
        4
    );
    assert_eq!(
        hoster_initial_segment_cap_for_mode(&policy, DownloadPerformanceMode::Fast),
        6
    );
    assert_eq!(
        hoster_adaptive_segment_cap_for_mode(&policy, DownloadPerformanceMode::Fast),
        10
    );
}

#[test]
fn segment_budget_uses_active_connection_leases() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_3",
            "https://s1.datanodes.to/d/ghi/file.bin",
            SegmentConnectionBudget {
                total: 16,
                per_origin: 8,
            },
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/abc/file.bin",
                    4,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s2.datanodes.to/d/def/file.bin",
                    4,
                ),
            ],
        ),
        Some(4)
    );
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_4",
            "https://s3.datanodes.to/d/jkl/file.bin",
            SegmentConnectionBudget {
                total: 8,
                per_origin: 4,
            },
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/abc/file.bin",
                    4,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s2.datanodes.to/d/def/file.bin",
                    4,
                ),
            ],
        ),
        None
    );
}

#[test]
fn normal_download_segment_budget_limits_same_origin_connections() {
    let budget = normal_segment_budget_for_mode(DownloadPerformanceMode::Balanced)
        .expect("balanced normal downloads should use brokered segment budgets");

    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::Normal,
            "job_3",
            "https://cdn.example.com/third.bin",
            budget,
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::Normal,
                    "https://cdn.example.com/first.bin",
                    4,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::Normal,
                    "https://cdn.example.com/second.bin",
                    2,
                ),
            ],
        ),
        Some(2)
    );
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::Normal,
            "job_4",
            "https://cdn.example.com/fourth.bin",
            budget,
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::Normal,
                    "https://cdn.example.com/first.bin",
                    4,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::Normal,
                    "https://cdn.example.com/second.bin",
                    4,
                ),
            ],
        ),
        None
    );
}

#[test]
fn segment_budget_wait_action_defers_bulk_admissions_and_preserves_active_segment_state() {
    assert_eq!(
        DownloadAdmission::direct_bulk().segment_budget_wait_action(false),
        SegmentBudgetWaitAction::Defer
    );
    assert_eq!(
        DownloadAdmission::protected_hoster_bulk().segment_budget_wait_action(false),
        SegmentBudgetWaitAction::Defer
    );
    assert_eq!(
        DownloadAdmission::normal().segment_budget_wait_action(false),
        SegmentBudgetWaitAction::FallbackSingleStream
    );
    assert_eq!(
        DownloadAdmission::normal().segment_budget_wait_action(true),
        SegmentBudgetWaitAction::Defer
    );
}

#[test]
fn datanodes_balanced_budget_keeps_four_same_origin_jobs_segmented_with_two_segment_floor() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_4",
            "https://s1.datanodes.to/d/jkl/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Balanced).unwrap(),
            4,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/abc/file.bin",
                    4,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/def/file.bin",
                    2,
                ),
                (
                    "job_3",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/ghi/file.bin",
                    2,
                ),
            ],
        ),
        Some(2)
    );
}

#[test]
fn datanodes_oldest_balanced_worker_keeps_full_segment_cap() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_1",
            "https://s1.datanodes.to/d/abc/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Balanced).unwrap(),
            4,
            &[],
        ),
        Some(4)
    );
}

#[test]
fn datanodes_secondary_balanced_workers_start_with_two_segments() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_2",
            "https://s1.datanodes.to/d/def/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Balanced).unwrap(),
            4,
            &[(
                "job_1",
                SegmentConnectionClass::ProtectedHosterBulk,
                "https://s1.datanodes.to/d/abc/file.bin",
                4,
            )],
        ),
        Some(2)
    );
}

#[test]
fn datanodes_oldest_fast_worker_keeps_full_segment_cap() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_1",
            "https://s1.datanodes.to/d/abc/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Fast).unwrap(),
            6,
            &[],
        ),
        Some(6)
    );
}

#[test]
fn datanodes_fast_worker_can_grow_to_adaptive_segment_cap() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_1",
            "https://s1.datanodes.to/d/abc/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Fast).unwrap(),
            10,
            &[(
                "job_1",
                SegmentConnectionClass::ProtectedHosterBulk,
                "https://s1.datanodes.to/d/abc/file.bin",
                6,
            )],
        ),
        Some(10)
    );
}

#[test]
fn datanodes_secondary_fast_workers_start_with_two_segments() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_2",
            "https://s1.datanodes.to/d/def/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Fast).unwrap(),
            6,
            &[(
                "job_1",
                SegmentConnectionClass::ProtectedHosterBulk,
                "https://s1.datanodes.to/d/abc/file.bin",
                6,
            )],
        ),
        Some(2)
    );
}

#[test]
fn datanodes_third_fast_worker_stays_segmented_with_two_segment_floor() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_3",
            "https://s1.datanodes.to/d/ghi/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Fast).unwrap(),
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/abc/file.bin",
                    6,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/def/file.bin",
                    2,
                ),
            ],
        ),
        Some(2)
    );
}

#[test]
fn fuckingfast_oldest_balanced_worker_keeps_full_segment_cap() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_1",
            "https://dl.fuckingfast.co/dl/token-a/Game.part01.rar",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Balanced).unwrap(),
            4,
            &[],
        ),
        Some(4)
    );
}

#[test]
fn fuckingfast_secondary_balanced_workers_start_with_two_segments() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_2",
            "https://dl.fuckingfast.co/dl/token-b/Game.part02.rar",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Balanced).unwrap(),
            4,
            &[(
                "job_1",
                SegmentConnectionClass::ProtectedHosterBulk,
                "https://dl.fuckingfast.co/dl/token-a/Game.part01.rar",
                4,
            )],
        ),
        Some(2)
    );
}

#[test]
fn fuckingfast_third_fast_worker_stays_segmented_with_two_segment_floor() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_3",
            "https://dl.fuckingfast.co/dl/token-c/Game.part03.rar",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Fast).unwrap(),
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://dl.fuckingfast.co/dl/token-a/Game.part01.rar",
                    6,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://dl.fuckingfast.co/dl/token-b/Game.part02.rar",
                    2,
                ),
            ],
        ),
        Some(2)
    );
}

#[test]
fn stale_inflight_hoster_warmups_can_be_replaced() {
    clear_hoster_warmup_cache_for_tests();
    let key = hoster_warmup_key_for_tests("job_warmup", "https://datanodes.to/abc123/file.bin");
    let now = Instant::now();

    assert!(mark_hoster_warmup_inflight_for_tests(&key, now));
    assert!(!mark_hoster_warmup_inflight_for_tests(
        &key,
        now + HOSTER_WARMUP_INFLIGHT_TTL / 2
    ));
    assert!(mark_hoster_warmup_inflight_for_tests(
        &key,
        now + HOSTER_WARMUP_INFLIGHT_TTL + Duration::from_secs(1)
    ));
}

#[test]
fn datanodes_warmup_completion_wakes_scheduler() {
    let source = include_str!("http.rs");
    let warmup_function = source
        .split("pub(super) fn spawn_datanodes_hoster_warmups")
        .nth(1)
        .expect("DataNodes warmup spawning should exist");

    assert!(
        warmup_function.contains("schedule_downloads(app.clone(), state.clone())"),
        "ready or failed DataNodes warmups should wake the scheduler"
    );
}

#[test]
fn scheduler_uses_optional_snapshot_claims() {
    let source = include_str!("mod.rs");
    let scheduler_function = source
        .split("pub fn schedule_downloads")
        .nth(1)
        .expect("scheduler entrypoint should exist");

    assert!(
        scheduler_function.contains("if !state.request_scheduler_wake()"),
        "production scheduler wakeups should use the per-state wake guard before spawning"
    );
    assert!(
        scheduler_function.contains("run_scheduler_loop(app, state).await"),
        "production scheduler wakeups should delegate repeated passes to the guarded runner"
    );
    assert!(
        scheduler_function.contains("claim_schedulable_jobs_for_scheduler().await"),
        "production scheduler wakeups should use the optional-snapshot claim path"
    );
    assert!(
        !scheduler_function.contains("claim_schedulable_jobs().await"),
        "production scheduler wakeups should not force full snapshots on no-op claims"
    );
}

#[test]
fn scheduler_runner_loops_while_pending_wakes_exist() {
    let source = include_str!("mod.rs");
    let scheduler_loop = source
        .split("async fn run_scheduler_loop")
        .nth(1)
        .expect("scheduler loop helper should exist");

    assert!(
        scheduler_loop.contains("loop {"),
        "scheduler runner should loop instead of spawning one task per wake"
    );
    assert!(
        scheduler_loop.contains("run_scheduler_once(&app, &state).await"),
        "scheduler runner should preserve the existing single-pass scheduling behavior"
    );
    assert!(
        scheduler_loop.contains("if !state.complete_scheduler_run()"),
        "scheduler runner should release or continue through the per-state wake guard"
    );
}

#[test]
fn datanodes_warmup_cache_consumes_ready_links_and_drops_expired_links() {
    clear_hoster_warmup_cache_for_tests();
    let source_url = "https://datanodes.to/abc123456/Game.part.rar";
    put_hoster_warmup_for_tests(
        "job_warm",
        source_url,
        "https://s1.datanodes.to/d/abc123456/Game.part.rar",
        Instant::now() + Duration::from_secs(60),
    );

    assert_eq!(
        take_warmed_hoster_url_for_tests("job_warm", source_url).as_deref(),
        Some("https://s1.datanodes.to/d/abc123456/Game.part.rar")
    );
    assert!(take_warmed_hoster_url_for_tests("job_warm", source_url).is_none());

    put_hoster_warmup_for_tests(
        "job_expired",
        source_url,
        "https://s2.datanodes.to/d/abc123456/Game.part.rar",
        Instant::now() - Duration::from_secs(1),
    );
    assert!(take_warmed_hoster_url_for_tests("job_expired", source_url).is_none());
}

#[test]
fn direct_bulk_and_non_bulk_hoster_tasks_still_allow_segmented_downloads() {
    let direct_bulk = http_segment_policy_task(true, None);
    let non_bulk_hoster = http_segment_policy_task(false, Some("https://fuckingfast.co/source"));

    assert!(task_allows_segmented_download(&direct_bulk));
    assert!(task_allows_segmented_download(&non_bulk_hoster));
}

#[test]
fn healthy_hoster_bulk_progress_releases_fairness_scheduler() {
    let hoster_bulk = http_segment_policy_task(true, Some("https://fuckingfast.co/source"));
    let direct_bulk = http_segment_policy_task(true, None);

    assert!(task_releases_bulk_hoster_fairness(&hoster_bulk, 64 * 1024));
    assert!(!task_releases_bulk_hoster_fairness(
        &hoster_bulk,
        64 * 1024 - 1
    ));
    assert!(!task_releases_bulk_hoster_fairness(&direct_bulk, 96 * 1024));
}

#[test]
fn protected_bulk_hoster_stall_timeout_is_mode_specific() {
    let hoster_bulk = http_segment_policy_task(true, Some("https://datanodes.to/source"));
    let direct_bulk = http_segment_policy_task(true, None);

    assert_eq!(
        protected_bulk_hoster_stall_timeout(
            &hoster_bulk,
            performance_profile(DownloadPerformanceMode::Balanced),
        ),
        Some(Duration::from_secs(25))
    );
    assert_eq!(
        protected_bulk_hoster_stall_timeout(
            &hoster_bulk,
            performance_profile(DownloadPerformanceMode::Fast),
        ),
        Some(Duration::from_secs(15))
    );
    assert_eq!(
        protected_bulk_hoster_stall_timeout(
            &hoster_bulk,
            performance_profile(DownloadPerformanceMode::Stable),
        ),
        Some(Duration::from_secs(90))
    );
    assert_eq!(
        protected_bulk_hoster_stall_timeout(
            &direct_bulk,
            performance_profile(DownloadPerformanceMode::Balanced),
        ),
        None
    );
}

#[test]
fn protected_bulk_hoster_stall_errors_are_retryable_network_failures() {
    let error = bulk_hoster_stall_error(Duration::from_secs(25));

    assert_eq!(error.category, FailureCategory::Network);
    assert!(error.retryable);
    assert!(error.message.contains("25 seconds"));
}

#[test]
fn http_attempt_defers_segment_budget_waits_without_deferring_priority_throttle() {
    let source = include_str!("http.rs");
    let attempt = source
        .split("async fn run_http_download_attempt_for_url")
        .nth(1)
        .expect("HTTP download attempt function should exist");

    assert!(attempt.contains("hoster_priority_throttle_decision"));
    assert!(attempt.contains("throttle_download_with_dynamic_limit"));
    assert!(attempt.contains("priority_throttle_limited"));
    assert!(attempt.contains("speed_limit.is_some() || priority_throttle_limited"));
    assert!(attempt.contains("segment_budget_wait_action"));
    assert!(attempt.contains("DownloadOutcome::Deferred"));
}

#[test]
fn segmented_hoster_workers_use_aggregate_priority_throttle_without_deferring() {
    let source = include_str!("segmented.rs");
    let worker = source
        .split("pub(super) async fn download_segment_worker")
        .nth(1)
        .expect("segmented worker should exist");

    assert!(worker.contains("hoster_priority_throttle_decision"));
    assert!(worker.contains("throttle_download_with_dynamic_limit"));
    assert!(worker.contains("priority_throttle_limited"));
    assert!(source.contains("priority_throttle"));
    assert!(!source.contains("return Ok(DownloadOutcome::Deferred"));
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
fn torrent_low_throughput_monitor_reports_zero_speed_with_few_churning_peers() {
    let now = Instant::now();
    let mut monitor = TorrentLowThroughputMonitor::default();
    let mut update = torrent_runtime_update(1024, 4096, 0);
    update.peers = Some(2);
    update.diagnostics = Some(crate::storage::TorrentRuntimeDiagnostics {
        live_peers: 2,
        seen_peers: 8,
        connecting_peers: 2,
        peer_connection_attempts: 6,
        listen_port: None,
        ..Default::default()
    });

    assert!(!monitor.should_report(&update, now));
    assert!(
        monitor.should_report(&update, now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW),
        "few peer torrents with churn and no progress should still be reported as stalled"
    );
}

#[test]
fn torrent_low_throughput_monitor_uses_fetched_progress_window_over_instant_kbps() {
    let now = Instant::now();
    let mut monitor = TorrentLowThroughputMonitor::default();
    let mut update = torrent_runtime_update(0, 0, 32 * 1024);
    update.diagnostics = Some(crate::storage::TorrentRuntimeDiagnostics {
        live_peers: TORRENT_LOW_THROUGHPUT_LIVE_PEER_THRESHOLD,
        seen_peers: 20,
        contributing_peers: 2,
        listen_port: Some(42000),
        ..Default::default()
    });

    assert!(!monitor.should_report(&update, now));
    update.fetched_bytes = TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND
        * TORRENT_LOW_THROUGHPUT_REPORT_WINDOW.as_secs();
    update.downloaded_bytes = update.fetched_bytes;
    assert!(
        !monitor.should_report(&update, now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW),
        "steady fetched-byte progress should prevent a low instant-speed stall report"
    );
    assert!(!monitor.should_report(
        &update,
        now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW + Duration::from_secs(1)
    ));
    assert!(
        monitor.should_report(
            &update,
            now + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW
                + Duration::from_secs(1)
                + TORRENT_LOW_THROUGHPUT_REPORT_WINDOW
        ),
        "the monitor should report if fetched-byte progress stops after the recovery window"
    );
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

#[test]
fn range_backoff_does_not_apply_to_different_files_on_same_host() {
    let backoff = RangeBackoffRegistry::default();
    let now = Instant::now();
    let rejected_url = "https://dl.fuckingfast.co/dl/token-part03/Game.part03.rar?download=1";
    let other_path_url = "https://dl.fuckingfast.co/dl/token-part04/Game.part04.rar?download=1";
    let other_query_url = "https://dl.fuckingfast.co/dl/token-part03/Game.part03.rar?download=2";

    backoff.record_rejection(rejected_url, now);

    assert!(backoff.is_backed_off(rejected_url, now + Duration::from_secs(1)));
    assert!(!backoff.is_backed_off(other_path_url, now + Duration::from_secs(1)));
    assert!(!backoff.is_backed_off(other_query_url, now + Duration::from_secs(1)));
}

#[test]
fn range_backoff_supports_source_keyed_hoster_policies() {
    let backoff = RangeBackoffRegistry::default();
    let now = Instant::now();
    let key = "hoster:datanodes:abc123456789";

    assert!(!backoff.is_key_backed_off(key, now));
    backoff.record_key_rejection(key, now);

    assert!(backoff.is_key_backed_off(key, now + Duration::from_secs(1)));
    assert!(!backoff.is_key_backed_off("hoster:datanodes:other-file", now + Duration::from_secs(1)));
    assert!(!backoff.is_key_backed_off(key, now + RANGE_BACKOFF_DURATION));
}

#[test]
fn large_bulk_member_at_seventeen_kib_per_second_retries_without_partial_reset() {
    let profile = performance_profile(DownloadPerformanceMode::Balanced);
    let recovery_state = crate::state::BulkMemberSlowRecoveryState {
        retry_attempts: 0,
        max_retry_attempts: 3,
    };

    assert_eq!(
        bulk_slow_stream_recovery_action(
            17 * 1024 * 20,
            Duration::from_secs(20),
            Some(500 * 1024 * 1024),
            2 * 1024 * 1024,
            None,
            profile,
            Some(recovery_state),
        ),
        BulkSlowStreamRecoveryAction::Retry
    );
}

#[test]
fn bulk_slow_recovery_ignores_non_bulk_and_speed_limited_downloads() {
    let profile = performance_profile(DownloadPerformanceMode::Balanced);
    let recovery_state = crate::state::BulkMemberSlowRecoveryState {
        retry_attempts: 0,
        max_retry_attempts: 3,
    };

    assert_eq!(
        bulk_slow_stream_recovery_action(
            17 * 1024 * 20,
            Duration::from_secs(20),
            Some(500 * 1024 * 1024),
            2 * 1024 * 1024,
            None,
            profile,
            None,
        ),
        BulkSlowStreamRecoveryAction::Continue
    );
    assert_eq!(
        bulk_slow_stream_recovery_action(
            17 * 1024 * 20,
            Duration::from_secs(20),
            Some(500 * 1024 * 1024),
            2 * 1024 * 1024,
            Some(64 * 1024),
            profile,
            Some(recovery_state),
        ),
        BulkSlowStreamRecoveryAction::Continue
    );
}

#[test]
fn near_complete_bulk_slow_recovery_preserves_partial_file() {
    let profile = performance_profile(DownloadPerformanceMode::Balanced);
    let recovery_state = crate::state::BulkMemberSlowRecoveryState {
        retry_attempts: 1,
        max_retry_attempts: 3,
    };

    assert_eq!(
        bulk_slow_stream_recovery_action(
            17 * 1024 * 20,
            Duration::from_secs(20),
            Some(500 * 1024 * 1024),
            499 * 1024 * 1024,
            None,
            profile,
            Some(recovery_state),
        ),
        BulkSlowStreamRecoveryAction::Retry
    );
}

#[test]
fn exhausted_bulk_slow_recovery_recycles_stream_and_preserves_partial() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let recovery_state = crate::state::BulkMemberSlowRecoveryState {
        retry_attempts: 3,
        max_retry_attempts: 3,
    };

    assert_eq!(
        bulk_slow_stream_recovery_action(
            64 * 1024 * 15,
            Duration::from_secs(15),
            Some(500 * 1024 * 1024),
            2 * 1024 * 1024,
            None,
            profile,
            Some(recovery_state),
        ),
        BulkSlowStreamRecoveryAction::Retry
    );
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

#[test]
fn segmented_progress_initial_bytes_are_index_aligned_after_dynamic_splits() {
    let segments = vec![
        SegmentProgress {
            index: 0,
            range: ByteRange { start: 0, end: 15 },
            downloaded_bytes: 10,
            completed: false,
        },
        SegmentProgress {
            index: 2,
            range: ByteRange { start: 16, end: 31 },
            downloaded_bytes: 4,
            completed: false,
        },
        SegmentProgress {
            index: 1,
            range: ByteRange { start: 32, end: 47 },
            downloaded_bytes: 8,
            completed: false,
        },
    ];

    let counters = SegmentedProgressCounters::new(segment_existing_lengths_by_index(
        Path::new("unused"),
        &segments,
    ));

    assert_eq!(counters.total_downloaded(), 22);
    counters.store_segment_bytes(1, 12);
    assert_eq!(counters.total_downloaded(), 26);
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
    assert!(
        !segment_meta_temp_path(&temp_path).exists(),
        "segment metadata should be finalized with a rename and no stale temp sidecar"
    );
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

#[tokio::test]
async fn segment_state_persists_concurrent_writers_without_temp_file_race() {
    let root = test_download_runtime_dir("segment-concurrent-sidecar");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let validators = EntityValidators::default();
    prepare_direct_segment_file(&temp_path, plan.total_bytes)
        .await
        .unwrap();

    let writer_count = 96;
    let barrier = Arc::new(tokio::sync::Barrier::new(writer_count));
    let mut handles = Vec::with_capacity(writer_count);

    for index in 0..writer_count {
        let barrier = barrier.clone();
        let temp_path = temp_path.clone();
        let plan = plan.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let mut state = new_segment_state_for_test(&plan, EntityValidators::default());
            let segment_index = index % state.segments.len();
            state.segments[segment_index].downloaded_bytes =
                (index as u64 % state.segments[segment_index].range.len()).saturating_add(1);
            persist_segment_state(&temp_path, &state).await
        }));
    }

    for handle in handles {
        handle
            .await
            .expect("segment metadata writer should not panic")
            .expect("segment metadata writer should not race on temp file replacement");
    }

    let reloaded = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .expect("final segment metadata should remain readable");
    assert_eq!(reloaded.total_bytes, plan.total_bytes);
    assert_eq!(reloaded.segments.len(), plan.segments.len());
    assert!(
        !segment_meta_temp_path(&temp_path).exists(),
        "fixed legacy metadata temp path should not be left behind"
    );

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn record_segment_progress_releases_metadata_lock_before_persisting() {
    let root = test_download_runtime_dir("segment-progress-lock-release");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let metadata = Arc::new(Mutex::new(new_segment_state_for_test(
        &plan,
        EntityValidators::default(),
    )));
    let metadata_lock = segment_metadata_lock(&temp_path);
    let metadata_guard = metadata_lock.lock().await;

    let record_task = {
        let metadata = Arc::clone(&metadata);
        let temp_path = temp_path.clone();
        tokio::spawn(async move {
            record_segment_progress(&temp_path, &metadata, 1, 2, false, true).await
        })
    };

    let progress_observed = tokio::time::timeout(Duration::from_millis(300), async {
        loop {
            if let Ok(state) = metadata.try_lock() {
                let segment = state
                    .segments
                    .iter()
                    .find(|segment| segment.index == 1)
                    .expect("segment should exist");
                if segment.downloaded_bytes == 2 && !segment.completed {
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .is_ok();

    assert!(
        progress_observed,
        "segment progress should release the metadata mutex before waiting on sidecar persistence"
    );

    drop(metadata_guard);
    record_task
        .await
        .expect("progress task should not panic")
        .expect("progress task should finish after sidecar lock is released");

    let reloaded = load_or_create_segment_state(&temp_path, &plan, &EntityValidators::default())
        .await
        .expect("forced progress persist should write readable metadata");
    assert_eq!(reloaded.segments[1].downloaded_bytes, 2);
    assert!(!reloaded.segments[1].completed);

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn blocked_segment_progress_persist_flushes_latest_metadata_snapshot() {
    let root = test_download_runtime_dir("segment-progress-latest-snapshot");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let metadata = Arc::new(Mutex::new(new_segment_state_for_test(
        &plan,
        EntityValidators::default(),
    )));
    let metadata_lock = segment_metadata_lock(&temp_path);
    let metadata_guard = metadata_lock.lock().await;

    let persist_task = {
        let metadata = Arc::clone(&metadata);
        let temp_path = temp_path.clone();
        tokio::spawn(async move {
            record_segment_progress(&temp_path, &metadata, 0, 4, true, true).await
        })
    };

    tokio::time::timeout(Duration::from_millis(300), async {
        loop {
            if let Ok(state) = metadata.try_lock() {
                if state.segments[0].completed {
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first progress update should not keep the metadata mutex while persistence waits");

    record_segment_progress(&temp_path, &metadata, 1, 3, false, false)
        .await
        .expect("coalesced progress update should succeed while persistence waits");

    drop(metadata_guard);
    persist_task
        .await
        .expect("persisting progress task should not panic")
        .expect("persisting progress task should finish after sidecar lock is released");

    let reloaded = load_or_create_segment_state(&temp_path, &plan, &EntityValidators::default())
        .await
        .expect("forced progress persist should write readable metadata");
    assert_eq!(reloaded.segments[0].downloaded_bytes, 4);
    assert!(reloaded.segments[0].completed);
    assert_eq!(reloaded.segments[1].downloaded_bytes, 3);
    assert!(!reloaded.segments[1].completed);

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
async fn missing_segment_metadata_preserves_preallocated_partial_and_requires_restart() {
    let root = test_download_runtime_dir("segment-missing-metadata-preserve");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let validators = EntityValidators::default();
    tokio::fs::write(&temp_path, vec![0_u8; plan.total_bytes as usize])
        .await
        .unwrap();

    let error = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .expect_err("missing segment metadata with an existing partial should not reset progress");

    assert_eq!(error.category, FailureCategory::Resume);
    assert!(error.message.contains("Resume metadata is missing"));
    assert_eq!(
        error.resume_metadata_issue,
        Some(SegmentResumeMetadataIssue::Missing)
    );
    assert!(temp_path.exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn corrupt_segment_metadata_preserves_preallocated_partial_and_requires_restart() {
    let root = test_download_runtime_dir("segment-corrupt-metadata-preserve");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let validators = EntityValidators::default();
    tokio::fs::write(&temp_path, vec![0_u8; plan.total_bytes as usize])
        .await
        .unwrap();
    tokio::fs::write(segment_meta_path(&temp_path), b"not json")
        .await
        .unwrap();

    let error = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .expect_err("corrupt segment metadata with an existing partial should not reset progress");

    assert_eq!(error.category, FailureCategory::Resume);
    assert!(error.message.contains("Resume metadata is missing"));
    assert_eq!(
        error.resume_metadata_issue,
        Some(SegmentResumeMetadataIssue::Corrupt)
    );
    assert!(temp_path.exists());
    assert!(segment_meta_path(&temp_path).exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segment_state_preserves_partial_and_requires_restart_when_validators_change() {
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
    let error = load_or_create_segment_state(&temp_path, &plan, &next_validators)
        .await
        .expect_err("validator conflicts with an existing partial should require restart");

    assert_eq!(error.category, FailureCategory::Resume);
    assert!(error.message.contains("Resume metadata is missing"));
    assert_eq!(
        error.resume_metadata_issue,
        Some(SegmentResumeMetadataIssue::ValidatorConflict)
    );
    assert!(temp_path.exists());
    assert!(segment_meta_path(&temp_path).exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segment_state_preserves_partial_and_reports_incompatible_metadata() {
    let root = test_download_runtime_dir("segment-plan-incompatible");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let mut state = new_segment_state_for_test(&plan, EntityValidators::default());
    state.total_bytes = state.total_bytes.saturating_add(1);
    tokio::fs::write(&temp_path, b"abcdefghijkl").await.unwrap();
    persist_segment_state(&temp_path, &state).await.unwrap();

    let error = load_or_create_segment_state(&temp_path, &plan, &EntityValidators::default())
        .await
        .expect_err("incompatible segment metadata should require restart");

    assert_eq!(error.category, FailureCategory::Resume);
    assert!(error.message.contains("Resume metadata is missing"));
    assert_eq!(
        error.resume_metadata_issue,
        Some(SegmentResumeMetadataIssue::PlanIncompatible)
    );
    assert!(temp_path.exists());
    assert!(segment_meta_path(&temp_path).exists());

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
        schema_version: default_segment_state_schema_version(),
        total_bytes: 12,
        validators: validators.clone(),
        effective_url: Some(url.clone()),
        target_path: Some(root.join("download.bin").display().to_string()),
        temp_path: Some(temp_path.display().to_string()),
        last_verified_file_len: 12,
        retry_generation: 0,
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
        segment_pressure_key: "http://127.0.0.1:80".into(),
        handoff_auth: None,
        temp_path: temp_path.clone(),
        total_bytes: 12,
        profile: performance_profile(DownloadPerformanceMode::Balanced),
        validators,
        progress: Arc::new(SegmentedProgressCounters::new(vec![4])),
        metadata: Arc::new(Mutex::new(stored)),
        metadata_persisted_at: Arc::new(Mutex::new(Instant::now())),
        stop: Arc::new(AtomicBool::new(false)),
        control_signal: WorkerControlSignal::default(),
        ramp_blocked: Arc::new(AtomicBool::new(false)),
        priority_throttle: Arc::new(Mutex::new(DynamicThrottleState::default())),
        priority_throttle_enabled: false,
        stall_timeout: None,
        reconnects: Arc::new(SegmentReconnectTracker::default()),
        target_workers: Arc::new(AtomicUsize::new(1)),
        active_workers: Arc::new(AtomicUsize::new(1)),
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

#[tokio::test]
async fn segment_worker_collector_signals_stop_before_returning_error() {
    let stop = Arc::new(AtomicBool::new(false));
    let peer_observed_stop = Arc::new(AtomicBool::new(false));
    let mut workers = tokio::task::JoinSet::new();
    workers.spawn(async {
        Err(download_error(
            FailureCategory::Network,
            "segment failed quickly".into(),
            true,
        ))
    });
    {
        let stop = stop.clone();
        let peer_observed_stop = peer_observed_stop.clone();
        workers.spawn(async move {
            while !stop.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            peer_observed_stop.store(true, Ordering::Relaxed);
            Ok(DownloadOutcome::Paused)
        });
    }

    let (_outcome, error) = await_segment_workers_with_stop(workers, stop.clone()).await;

    assert_eq!(
        error
            .expect("first worker error should be returned")
            .message,
        "segment failed quickly"
    );
    assert!(stop.load(Ordering::Relaxed));
    assert!(peer_observed_stop.load(Ordering::Relaxed));
}

#[test]
fn retry_delay_honors_retry_after_and_applies_stable_jitter() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::RETRY_AFTER,
        reqwest::header::HeaderValue::from_static("120"),
    );

    assert_eq!(
        retry_delay_for_response(
            StatusCode::TOO_MANY_REQUESTS,
            &headers,
            0,
            "job_a",
            "https://example.com/file.bin",
        ),
        Duration::from_secs(60)
    );

    let first = retry_delay_for_response(
        StatusCode::SERVICE_UNAVAILABLE,
        &reqwest::header::HeaderMap::new(),
        1,
        "job_a",
        "https://example.com/file.bin",
    );
    let second = retry_delay_for_response(
        StatusCode::SERVICE_UNAVAILABLE,
        &reqwest::header::HeaderMap::new(),
        1,
        "job_b",
        "https://example.com/file.bin",
    );

    assert!(first >= REQUEST_RETRY_DELAYS[1]);
    assert!(first <= REQUEST_RETRY_DELAYS[1] + Duration::from_millis(250));
    assert_ne!(
        first, second,
        "bulk retry jitter should be stable but de-synchronized"
    );
}

#[tokio::test]
async fn direct_segment_buffered_writer_appends_chunks_after_initial_seek() {
    let root = test_download_runtime_dir("direct-segment-buffered-writer");
    let temp_path = root.join("download.bin.part");

    prepare_direct_segment_file(&temp_path, 12).await.unwrap();
    let mut writer = open_direct_segment_writer_at(&temp_path, 4).await.unwrap();
    write_segment_chunk(&mut writer, b"ru").await.unwrap();
    write_segment_chunk(&mut writer, b"st").await.unwrap();
    flush_segment_writer(&mut writer).await.unwrap();

    let bytes = tokio::fs::read(&temp_path).await.unwrap();
    assert_eq!(&bytes[4..8], b"rust");
    assert!(!segment_path(&temp_path, 0).exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[test]
fn normal_fast_segment_budget_allows_adaptive_fast_plus_ceiling() {
    let budget = normal_segment_budget_for_mode(DownloadPerformanceMode::Fast)
        .expect("fast normal downloads should use brokered segment budgets");

    assert_eq!(budget.total, 128);
    assert_eq!(budget.per_origin, 64);
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::Normal,
            "job_fast_1",
            "https://cdn.example.com/fast.bin",
            budget,
            usize::MAX,
            &[],
        ),
        Some(64)
    );
}

#[test]
fn fast_profile_uses_adaptive_sustain_defaults() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);

    assert_eq!(profile.initial_segments, 16);
    assert_eq!(profile.soft_max_segments, 32);
    assert_eq!(profile.max_segments, 64);
    assert_eq!(profile.adaptive_ramp_step, 4);
    assert_eq!(profile.adaptive_ramp_interval, Duration::from_secs(2));
}

#[test]
fn gofile_fast_profile_uses_conservative_direct_cap() {
    let profile = profile_for_effective_http_url(
        DownloadPerformanceMode::Fast,
        "https://store1.gofile.io/download/web/file-token/BeamNG-drive-SteamRIP.com.rar",
    );

    assert_eq!(profile.initial_segments, 8);
    assert_eq!(profile.soft_max_segments, 16);
    assert_eq!(profile.max_segments, 16);
    assert_eq!(profile.adaptive_ramp_step, 4);
}

#[test]
fn host_score_temporarily_caps_fast_profile_after_reconnects() {
    let _guard = segment_host_score_test_guard();
    reset_segment_host_scores_for_tests();
    let now = Instant::now();
    let url = "https://cdn.example.com/downloads/game.rar";

    record_segment_host_success(url, 32, now);
    record_segment_host_failure(url, 16, "segment reconnect", now + Duration::from_secs(1));

    let score = segment_host_score_snapshot(url, now + Duration::from_secs(2))
        .expect("host score should be retained during its TTL");
    assert_eq!(score.best_cap, 16);
    assert_eq!(score.recent_reconnects, 1);
    assert_eq!(
        score.last_failure_reason.as_deref(),
        Some("segment reconnect")
    );

    let profile = profile_for_effective_http_url_at(
        DownloadPerformanceMode::Fast,
        url,
        now + Duration::from_secs(2),
    );
    assert_eq!(profile.max_segments, 16);
    assert_eq!(profile.soft_max_segments, 16);

    assert!(segment_host_score_snapshot(url, now + Duration::from_secs(31 * 60)).is_none());
}

#[test]
fn segment_pressure_requires_repeated_429_before_capping_fast_profile() {
    let _guard = segment_host_score_test_guard();
    reset_segment_host_scores_for_tests();
    let now = Instant::now();
    let key = "hoster:test-file";
    let url = "https://rotating-cdn.example.com/downloads/game.rar";
    let error = error_for_http_status(StatusCode::TOO_MANY_REQUESTS, false);

    let first = record_segment_reconnect_pressure_for_error(key, 16, &error, now);
    assert_eq!(first.reduced_target, None);

    let uncapped = profile_for_effective_http_url_with_pressure_key_at(
        DownloadPerformanceMode::Fast,
        url,
        Some(key),
        now + Duration::from_secs(1),
    );
    assert_eq!(uncapped.initial_segments, 16);
    assert_eq!(uncapped.soft_max_segments, 32);
    assert_eq!(uncapped.max_segments, 64);

    record_segment_reconnect_pressure_for_error(key, 16, &error, now + Duration::from_secs(2));
    let third =
        record_segment_reconnect_pressure_for_error(key, 16, &error, now + Duration::from_secs(3));

    assert_eq!(third.reduced_target, Some(8));
    let capped = profile_for_effective_http_url_with_pressure_key_at(
        DownloadPerformanceMode::Fast,
        url,
        Some(key),
        now + Duration::from_secs(4),
    );
    assert_eq!(capped.initial_segments, 8);
    assert_eq!(capped.soft_max_segments, 8);
    assert_eq!(capped.max_segments, 8);
}

#[test]
fn non_rate_limit_segment_errors_do_not_reduce_future_fast_caps() {
    let _guard = segment_host_score_test_guard();
    reset_segment_host_scores_for_tests();
    let now = Instant::now();
    let key = "hoster:network-noise";
    let error = download_error(
        FailureCategory::Network,
        "Download failed: error decoding response body".into(),
        true,
    );

    for offset in 0..4 {
        let decision = record_segment_reconnect_pressure_for_error(
            key,
            16,
            &error,
            now + Duration::from_secs(offset),
        );
        assert_eq!(decision.reduced_target, None);
    }

    let profile = profile_for_effective_http_url_with_pressure_key_at(
        DownloadPerformanceMode::Fast,
        "https://cdn.example.com/downloads/game.rar",
        Some(key),
        now + Duration::from_secs(5),
    );
    assert_eq!(profile.initial_segments, 16);
    assert_eq!(profile.soft_max_segments, 32);
    assert_eq!(profile.max_segments, 64);
}

#[test]
fn segment_pressure_expires_and_allows_fast_profile_recovery() {
    let _guard = segment_host_score_test_guard();
    reset_segment_host_scores_for_tests();
    let now = Instant::now();
    let key = "hoster:ttl";
    let url = "https://cdn.example.com/downloads/game.rar";
    let error = error_for_http_status(StatusCode::TOO_MANY_REQUESTS, false);

    for offset in 0..3 {
        record_segment_reconnect_pressure_for_error(
            key,
            16,
            &error,
            now + Duration::from_secs(offset),
        );
    }

    let capped = profile_for_effective_http_url_with_pressure_key_at(
        DownloadPerformanceMode::Fast,
        url,
        Some(key),
        now + Duration::from_secs(4),
    );
    assert_eq!(capped.max_segments, 8);

    let recovered = profile_for_effective_http_url_with_pressure_key_at(
        DownloadPerformanceMode::Fast,
        url,
        Some(key),
        now + Duration::from_secs(31 * 60),
    );
    assert_eq!(recovered.initial_segments, 16);
    assert_eq!(recovered.soft_max_segments, 32);
    assert_eq!(recovered.max_segments, 64);
}

#[test]
fn segment_write_coalescer_batches_small_chunks_and_flushes_tail() {
    let mut coalescer = SegmentWriteCoalescer::new(8);

    assert_eq!(coalescer.push(b"abc"), None);
    assert_eq!(coalescer.push(b"defg"), None);
    assert_eq!(coalescer.push(b"h"), Some(b"abcdefgh".to_vec()));
    assert_eq!(coalescer.flush(), None);

    assert_eq!(coalescer.push(b"xy"), None);
    assert_eq!(coalescer.flush(), Some(b"xy".to_vec()));
    assert_eq!(coalescer.flush(), None);
}

#[test]
fn worker_control_signal_maps_live_control_without_state_lookup() {
    let signal = WorkerControlSignal::default();

    assert_eq!(signal.current_outcome(), None);
    signal.store_control(WorkerControl::Paused);
    assert_eq!(signal.current_outcome(), Some(DownloadOutcome::Paused));
    signal.store_control(WorkerControl::Canceled);
    assert_eq!(signal.current_outcome(), Some(DownloadOutcome::Canceled));
    signal.store_control(WorkerControl::Missing);
    assert_eq!(signal.current_outcome(), Some(DownloadOutcome::Canceled));
    signal.store_control(WorkerControl::Continue);
    assert_eq!(signal.current_outcome(), None);
}

#[test]
fn direct_bulk_fast_budget_reuses_normal_fast_connection_ceiling() {
    let budget = direct_bulk_segment_budget_for_mode(DownloadPerformanceMode::Fast)
        .expect("fast direct bulk downloads should use brokered segment budgets");
    let normal_budget = normal_segment_budget_for_mode(DownloadPerformanceMode::Fast)
        .expect("fast normal downloads should use brokered segment budgets");

    assert_eq!(budget, normal_budget);
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::DirectBulk,
            "bulk_fast_1",
            "https://cdn.example.com/bulk-fast.bin",
            budget,
            usize::MAX,
            &[],
        ),
        Some(64)
    );
}

#[test]
fn direct_bulk_and_normal_segment_budgets_are_isolated_by_class() {
    let budget = direct_bulk_segment_budget_for_mode(DownloadPerformanceMode::Fast)
        .expect("fast direct bulk downloads should use brokered segment budgets");

    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::DirectBulk,
            "bulk_fast_1",
            "https://cdn.example.com/bulk-fast.bin",
            budget,
            usize::MAX,
            &[(
                "normal_fast_1",
                SegmentConnectionClass::Normal,
                "https://cdn.example.com/normal-fast.bin",
                64,
            )],
        ),
        Some(64)
    );
}

#[test]
fn segment_metadata_persist_gate_coalesces_regular_writes_and_allows_forced_writes() {
    let now = Instant::now();
    let mut last_persisted_at = now;

    assert!(
        !should_persist_segment_metadata(
            &mut last_persisted_at,
            now + Duration::from_secs(1),
            false,
        ),
        "regular segment progress should not persist before the shared interval"
    );
    assert_eq!(last_persisted_at, now);

    let interval_elapsed = now + PROGRESS_PERSIST_INTERVAL;
    assert!(
        should_persist_segment_metadata(&mut last_persisted_at, interval_elapsed, false),
        "one worker should persist after the shared interval elapses"
    );
    assert_eq!(last_persisted_at, interval_elapsed);

    assert!(
        !should_persist_segment_metadata(&mut last_persisted_at, interval_elapsed, false),
        "peer segment workers should be coalesced after another worker persists"
    );
    assert!(
        should_persist_segment_metadata(&mut last_persisted_at, interval_elapsed, true),
        "forced interruption and completion writes should bypass coalescing"
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
        schema_version: default_segment_state_schema_version(),
        total_bytes: plan.total_bytes,
        validators,
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
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

fn segmented_state_for_test(
    total_bytes: u64,
    ranges: Vec<(u64, u64, u64, bool)>,
) -> SegmentedDownloadState {
    SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: ranges
            .into_iter()
            .enumerate()
            .map(
                |(index, (start, end, downloaded_bytes, completed))| SegmentProgress {
                    index,
                    range: ByteRange { start, end },
                    downloaded_bytes,
                    completed,
                },
            )
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
    output_dirs: std::cell::RefCell<Vec<PathBuf>>,
}

impl ArchiveExtractor for RecordingArchiveExtractor {
    fn extract(&self, first_part: &Path, output_dir: &Path) -> Result<(), String> {
        self.calls.borrow_mut().push(first_part.to_path_buf());
        self.output_dirs.borrow_mut().push(output_dir.to_path_buf());
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

struct FlatContentArchiveExtractor;

impl ArchiveExtractor for FlatContentArchiveExtractor {
    fn extract(&self, _first_part: &Path, output_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(output_dir).unwrap();
        std::fs::write(output_dir.join("content.bin"), b"duplicate").unwrap();
        Ok(())
    }
}

struct SymlinkArchiveExtractor;

impl ArchiveExtractor for SymlinkArchiveExtractor {
    fn extract(&self, _first_part: &Path, output_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(output_dir).unwrap();
        let target = output_dir.join("target.bin");
        let link = output_dir.join("linked.bin");
        std::fs::write(&target, b"target").unwrap();
        create_file_symlink_for_test(&target, &link)
    }
}

#[cfg(unix)]
fn create_file_symlink_for_test(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn create_file_symlink_for_test(target: &Path, link: &Path) -> Result<(), String> {
    std::os::windows::fs::symlink_file(target, link)
        .map_err(|_| "symlink creation is not available in this test environment".to_string())
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

fn extracting_staging_dirs(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.contains(".extracting-"))
        })
        .collect()
}

fn http_segment_policy_task(
    is_bulk_member: bool,
    resolved_from_url: Option<&str>,
) -> crate::state::DownloadTask {
    crate::state::DownloadTask {
        id: "job_policy".into(),
        url: "https://cdn.example.com/file.bin".into(),
        filename: "file.bin".into(),
        transfer_kind: TransferKind::Http,
        torrent: None,
        handoff_auth: None,
        resolved_from_url: resolved_from_url.map(str::to_string),
        source: None,
        is_bulk_member,
        bulk_archive_id: is_bulk_member.then_some("bulk_policy".into()),
        retry_attempts: 0,
        target_path: PathBuf::from("C:/Downloads/file.bin"),
        temp_path: PathBuf::from("C:/Downloads/file.bin.part"),
    }
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
    let root = test_download_runtime_dir("sha256-digest");
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
