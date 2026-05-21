use super::*;

#[tokio::test]
async fn local_recovery_preview_excludes_partial_files_and_uses_stable_candidate_ids() {
    let download_dir = test_runtime_dir("local-recovery-preview");
    let recovered_file = download_dir.join("Video").join("movie.mkv");
    let partial_file = download_dir.join("Video").join("movie.mkv.part");
    let browser_partial = download_dir.join("Video").join("clip.mp4.crdownload");
    let torrent_state = download_dir.join(".torrent-state");
    std::fs::create_dir_all(recovered_file.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&torrent_state).unwrap();
    std::fs::write(&recovered_file, b"complete").unwrap();
    std::fs::write(&partial_file, b"partial").unwrap();
    std::fs::write(&browser_partial, b"partial").unwrap();
    std::fs::write(torrent_state.join("piece.bin"), b"internal").unwrap();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);
    let mut settings = state.settings().await;
    settings.download_directory = download_dir.display().to_string();
    state.save_settings(settings).await.unwrap();

    let first = state.preview_local_recovery(None).await.unwrap();
    let second = state.preview_local_recovery(None).await.unwrap();

    assert_eq!(first.root, download_dir.display().to_string());
    assert_eq!(first.candidates.len(), 1);
    assert_eq!(first.candidates[0].filename, "movie.mkv");
    assert_eq!(
        first.candidates[0].path,
        recovered_file.display().to_string()
    );
    assert_eq!(first.candidates[0].id, second.candidates[0].id);
    assert!(first.skipped_count >= 3);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn local_recovery_import_creates_completed_non_retryable_rows_and_persists_them() {
    let download_dir = test_runtime_dir("local-recovery-import");
    let recovered_file = download_dir.join("Document").join("manual.pdf");
    std::fs::create_dir_all(recovered_file.parent().unwrap()).unwrap();
    std::fs::write(&recovered_file, b"complete document").unwrap();
    let state_path = download_dir.join("state.json");
    let state = shared_state_with_jobs(state_path.clone(), vec![]);
    let mut settings = state.settings().await;
    settings.download_directory = download_dir.display().to_string();
    state.save_settings(settings).await.unwrap();
    let candidate = state
        .preview_local_recovery(None)
        .await
        .unwrap()
        .candidates
        .into_iter()
        .find(|candidate| candidate.path == recovered_file.display().to_string())
        .expect("local file should be offered for recovery");

    let snapshot = state
        .import_local_recovery(vec![candidate.id])
        .await
        .unwrap();

    assert_eq!(snapshot.jobs.len(), 1);
    let job = &snapshot.jobs[0];
    assert_eq!(job.state, JobState::Completed);
    assert_eq!(job.progress, 100.0);
    assert_eq!(job.downloaded_bytes, job.total_bytes);
    assert_eq!(job.target_path, recovered_file.display().to_string());
    assert!(job.url.starts_with("recovered://local-file/"));
    assert_eq!(
        job.source
            .as_ref()
            .map(|source| source.entry_point.as_str()),
        Some("local_recovery")
    );
    let persisted = load_persisted_state(&state_path).unwrap();
    assert_eq!(persisted.jobs.len(), 1);

    let retry_error = state.retry_job(&job.id).await.unwrap_err();
    assert_eq!(retry_error.code, "UNSUPPORTED_RECOVERY_ACTION");
    assert!(retry_error.message.contains("Recovered local files"));
    let restart_error = state.restart_job(&job.id).await.unwrap_err();
    assert_eq!(restart_error.code, "UNSUPPORTED_RECOVERY_ACTION");

    let _ = std::fs::remove_dir_all(download_dir);
}
