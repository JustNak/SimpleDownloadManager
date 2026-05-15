use super::*;

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
