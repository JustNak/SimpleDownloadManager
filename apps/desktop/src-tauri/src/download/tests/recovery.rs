use super::*;

#[test]
fn retry_decision_pauses_recoverably_after_exhausted_retry_budget_with_partial_progress() {
    let error = download_error(
        FailureCategory::Network,
        "connection reset while reading body".into(),
        true,
    );

    assert_eq!(
        retry_decision_for_attempt_error(&error, 3, 3, true),
        RetryDecision::PauseRecoverably
    );
}

#[test]
fn retry_decision_fails_hard_without_partial_progress_or_for_resume_conflicts() {
    let network = download_error(
        FailureCategory::Network,
        "connection reset before any bytes arrived".into(),
        true,
    );
    assert_eq!(
        retry_decision_for_attempt_error(&network, 3, 3, false),
        RetryDecision::Fail
    );

    let resume = download_error(
        FailureCategory::Resume,
        "Resume metadata is missing or no longer matches this partial download.".into(),
        false,
    );
    assert_eq!(
        retry_decision_for_attempt_error(&resume, 0, 3, true),
        RetryDecision::Fail
    );
}

#[tokio::test]
async fn retry_exhaustion_pause_preserves_partial_progress() {
    let storage_path = test_storage_path("retry-exhaustion-pause-state");
    let mut job = torrent_job("job_retry_pause", JobState::Downloading);
    job.transfer_kind = TransferKind::Http;
    job.downloaded_bytes = 7 * 1024 * 1024;
    job.total_bytes = 32 * 1024 * 1024;
    job.progress = 21.875;
    job.resume_support = ResumeSupport::Supported;
    let state = SharedState::for_tests(storage_path, vec![job]);

    let snapshot = state
        .pause_job_after_retry_exhaustion(
            "job_retry_pause",
            "Network remained unstable after retries; paused with partial data preserved.",
            FailureCategory::Network,
        )
        .await
        .expect("recoverable pause should update state");

    let job = snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_retry_pause")
        .expect("job should remain in snapshot");
    assert_eq!(job.state, JobState::Paused);
    assert_eq!(job.downloaded_bytes, 7 * 1024 * 1024);
    assert_eq!(job.total_bytes, 32 * 1024 * 1024);
    assert_eq!(job.resume_support, ResumeSupport::Supported);
    assert_eq!(job.failure_category, Some(FailureCategory::Network));
    assert!(job
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("partial data preserved"));
}

#[tokio::test]
async fn segment_state_recovers_from_valid_backup_when_primary_metadata_is_corrupt() {
    let root = test_download_runtime_dir("segment-backup-recovery");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let validators = EntityValidators {
        etag: Some("\"backup-source\"".into()),
        last_modified: None,
    };
    let mut state = new_segment_state_for_test(&plan, validators.clone());
    state.segments[0].downloaded_bytes = 4;
    state.segments[0].completed = true;
    persist_segment_state(&temp_path, &state)
        .await
        .expect("initial metadata should persist");
    let backup_path = PathBuf::from(format!("{}.meta.bak", temp_path.display()));
    tokio::fs::copy(segment_meta_path(&temp_path), backup_path)
        .await
        .expect("test should stage a valid metadata backup");
    tokio::fs::write(segment_meta_path(&temp_path), b"{corrupt json")
        .await
        .expect("test should corrupt primary metadata");
    tokio::fs::write(&temp_path, vec![0_u8; plan.total_bytes as usize])
        .await
        .expect("partial file should exist");

    let recovered = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .expect("valid backup metadata should recover a partial download");

    assert_eq!(recovered.segments[0].downloaded_bytes, 4);
    assert!(recovered.segments[0].completed);
    assert_eq!(
        recovered.validators.etag.as_deref(),
        Some("\"backup-source\"")
    );
}
