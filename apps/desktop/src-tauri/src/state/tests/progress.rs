use super::*;

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
async fn update_job_progress_delta_returns_changed_job_and_settings() {
    let download_dir = test_runtime_dir("progress-delta-http");
    let storage_path = download_dir.join("state.json");
    let mut job = download_job(
        "job_delta",
        JobState::Downloading,
        ResumeSupport::Supported,
        0,
    );
    job.total_bytes = 200;
    let state = shared_state_with_jobs(storage_path, vec![job]);

    let delta = state
        .update_job_progress_delta("job_delta", 50, Some(200), 25, true)
        .await
        .expect("progress delta should update the job");

    assert_eq!(delta.job.id, "job_delta");
    assert_eq!(delta.job.downloaded_bytes, 50);
    assert_eq!(delta.job.total_bytes, 200);
    assert_eq!(delta.job.speed, 25);
    assert_eq!(delta.job.eta, 6);
    assert_eq!(
        delta.settings.download_directory,
        Settings::default().download_directory
    );

    let snapshot = state.snapshot().await;
    assert_eq!(snapshot.jobs[0].id, delta.job.id);
    assert_eq!(
        snapshot.jobs[0].downloaded_bytes,
        delta.job.downloaded_bytes
    );
    assert_eq!(snapshot.jobs[0].speed, delta.job.speed);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn segmented_progress_counts_are_visible_but_not_persisted() {
    let download_dir = test_runtime_dir("segmented-progress-counts-transient");
    let storage_path = download_dir.join("state.json");
    let mut job = download_job(
        "job_segments",
        JobState::Downloading,
        ResumeSupport::Supported,
        0,
    );
    job.total_bytes = 100;
    let state = shared_state_with_jobs(storage_path.clone(), vec![job]);

    state
        .update_segmented_job_progress("job_segments", 40, Some(100), 4096, 3, 16, true)
        .await
        .expect("segmented progress update should expose connection counts");

    let snapshot = state.snapshot().await;
    assert_eq!(snapshot.jobs[0].active_segments, Some(3));
    assert_eq!(snapshot.jobs[0].planned_segments, Some(16));

    let persisted = load_persisted_state(&storage_path).expect("persisted state should load");
    assert_eq!(persisted.jobs[0].active_segments, None);
    assert_eq!(persisted.jobs[0].planned_segments, None);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn segmented_progress_delta_counts_are_visible_but_not_persisted() {
    let download_dir = test_runtime_dir("segmented-progress-delta-counts-transient");
    let storage_path = download_dir.join("state.json");
    let mut job = download_job(
        "job_segments_delta",
        JobState::Downloading,
        ResumeSupport::Supported,
        0,
    );
    job.total_bytes = 100;
    let state = shared_state_with_jobs(storage_path.clone(), vec![job]);

    let delta = state
        .update_segmented_job_progress_delta("job_segments_delta", 40, Some(100), 4096, 3, 16, true)
        .await
        .expect("segmented progress delta should expose connection counts");

    assert_eq!(delta.job.active_segments, Some(3));
    assert_eq!(delta.job.planned_segments, Some(16));

    let persisted = load_persisted_state(&storage_path).expect("persisted state should load");
    assert_eq!(persisted.jobs[0].active_segments, None);
    assert_eq!(persisted.jobs[0].planned_segments, None);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn sync_downloaded_bytes_delta_updates_memory_and_returns_changed_job() {
    let download_dir = test_runtime_dir("sync-progress-delta");
    let storage_path = download_dir.join("state.json");
    let mut job = download_job(
        "job_sync_delta",
        JobState::Paused,
        ResumeSupport::Supported,
        0,
    );
    job.total_bytes = 100;
    job.active_segments = Some(2);
    job.planned_segments = Some(8);
    let state = shared_state_with_jobs(storage_path.clone(), vec![job]);

    let delta = state
        .sync_downloaded_bytes_delta("job_sync_delta", 25)
        .await
        .expect("sync progress delta should update the job");

    assert_eq!(delta.job.downloaded_bytes, 25);
    assert_eq!(delta.job.progress, 25.0);
    assert_eq!(delta.job.active_segments, None);
    assert_eq!(delta.job.planned_segments, None);

    let persisted = load_persisted_state(&storage_path).expect("persisted state should load");
    assert_eq!(persisted.jobs[0].downloaded_bytes, 25);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn batch_progress_jobs_returns_only_context_members_in_queue_order() {
    let download_dir = test_runtime_dir("batch-progress-delta-jobs");
    let first = download_job("job_1", JobState::Downloading, ResumeSupport::Supported, 10);
    let second = download_job("job_2", JobState::Downloading, ResumeSupport::Supported, 20);
    let third = download_job("job_3", JobState::Downloading, ResumeSupport::Supported, 30);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![first, second, third]);

    let jobs = state
        .batch_progress_jobs(&["job_3".into(), "missing".into(), "job_1".into()])
        .await;

    assert_eq!(
        jobs.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(),
        vec!["job_1", "job_3"],
        "batch progress delta refresh should scan queue state without cloning unrelated jobs"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn progress_job_snapshot_parts_returns_requested_job_and_settings() {
    let download_dir = test_runtime_dir("progress-job-selector");
    let first = download_job("job_1", JobState::Queued, ResumeSupport::Unknown, 0);
    let second = download_job("job_2", JobState::Downloading, ResumeSupport::Supported, 45);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![first, second]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.download_directory = download_dir.display().to_string();
    }

    let (job, settings) = state.progress_job_snapshot_parts("job_2").await;

    let job = job.expect("requested job should be returned");
    assert_eq!(job.id, "job_2");
    assert_eq!(job.downloaded_bytes, 45);
    assert_eq!(
        settings.download_directory,
        download_dir.display().to_string()
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn batch_progress_snapshot_parts_preserve_queue_order_and_deduplicate_ids() {
    let download_dir = test_runtime_dir("batch-selector-dedupes");
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![
            download_job("job_1", JobState::Queued, ResumeSupport::Unknown, 0),
            download_job("job_2", JobState::Downloading, ResumeSupport::Supported, 50),
            download_job("job_3", JobState::Paused, ResumeSupport::Supported, 10),
        ],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.download_directory = download_dir.display().to_string();
    }

    let ids = vec![
        "job_3".to_string(),
        "missing".to_string(),
        "job_1".to_string(),
        "job_3".to_string(),
    ];
    let (jobs, settings) = state.batch_progress_snapshot_parts(&ids).await;

    assert_eq!(
        jobs.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(),
        vec!["job_1", "job_3"]
    );
    assert_eq!(
        settings.download_directory,
        download_dir.display().to_string()
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn batch_progress_jobs_use_indexed_selection() {
    let source = include_str!("../progress.rs");

    assert!(
        source.contains("state.job_index(id)"),
        "batch progress selectors should use RuntimeState job_indexes instead of scanning and cloning the full queue"
    );
}

#[tokio::test]
async fn single_stream_progress_clears_segment_counts() {
    let download_dir = test_runtime_dir("single-stream-progress-clears-segment-counts");
    let storage_path = download_dir.join("state.json");
    let mut job = download_job(
        "job_single",
        JobState::Downloading,
        ResumeSupport::Supported,
        0,
    );
    job.total_bytes = 100;
    job.active_segments = Some(4);
    job.planned_segments = Some(16);
    let state = shared_state_with_jobs(storage_path, vec![job]);

    state
        .update_job_progress("job_single", 25, Some(100), 2048, true)
        .await
        .expect("single stream progress should clear segmented connection counts");

    let snapshot = state.snapshot().await;
    assert_eq!(snapshot.jobs[0].active_segments, None);
    assert_eq!(snapshot.jobs[0].planned_segments, None);

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
