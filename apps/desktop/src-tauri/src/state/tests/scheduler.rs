use super::*;
use crate::state::scheduler::SchedulerAdmissionIndex;

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
async fn scheduler_claim_without_available_slots_omits_snapshot() {
    let download_dir = test_runtime_dir("scheduler-no-slot-no-snapshot");
    let archive = bulk_archive_info(&download_dir, "bulk_no_slot");
    let mut active_normal = download_job(
        "job_active_normal",
        JobState::Downloading,
        ResumeSupport::Supported,
        25,
    );
    active_normal.target_path = download_dir.join("active.zip").display().to_string();
    active_normal.temp_path = download_dir.join("active.zip.part").display().to_string();
    let mut active_bulk = download_job(
        "job_active_bulk",
        JobState::Downloading,
        ResumeSupport::Supported,
        40,
    );
    active_bulk.bulk_archive = Some(archive);
    active_bulk.target_path = download_dir.join("bulk.zip").display().to_string();
    active_bulk.temp_path = download_dir.join("bulk.zip.part").display().to_string();
    let queued = download_job("job_queued", JobState::Queued, ResumeSupport::Unknown, 0);
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active_normal, active_bulk, queued],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.settings.bulk.max_concurrent_downloads = 1;
        runtime.active_workers.insert("job_active_normal".into());
        runtime.active_workers.insert("job_active_bulk".into());
    }

    let claim = state
        .claim_schedulable_jobs_for_scheduler()
        .await
        .expect("scheduler claim should work");

    assert!(claim.tasks.is_empty());
    assert!(
        claim.snapshot.is_none(),
        "scheduler no-op claims should not clone a full DesktopSnapshot"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn scheduler_claim_with_tasks_returns_snapshot_for_emission() {
    let download_dir = test_runtime_dir("scheduler-claim-snapshot");
    let mut queued = download_job("job_queued", JobState::Queued, ResumeSupport::Unknown, 0);
    queued.target_path = download_dir.join("queued.zip").display().to_string();
    queued.temp_path = download_dir.join("queued.zip.part").display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![queued]);

    let claim = state
        .claim_schedulable_jobs_for_scheduler()
        .await
        .expect("scheduler claim should work");

    assert_eq!(claim.tasks.len(), 1);
    assert_eq!(claim.tasks[0].id, "job_queued");
    let snapshot = claim
        .snapshot
        .expect("scheduler claims with tasks should include the emission snapshot");
    assert_eq!(snapshot.jobs.len(), 1);
    assert_eq!(snapshot.jobs[0].id, "job_queued");
    assert_eq!(snapshot.jobs[0].state, JobState::Starting);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn compatibility_scheduler_claim_materializes_snapshot_for_noop_claims() {
    let download_dir = test_runtime_dir("scheduler-compat-snapshot");
    let mut active = download_job(
        "job_active",
        JobState::Downloading,
        ResumeSupport::Supported,
        25,
    );
    active.target_path = download_dir.join("active.zip").display().to_string();
    active.temp_path = download_dir.join("active.zip.part").display().to_string();
    let queued = download_job("job_queued", JobState::Queued, ResumeSupport::Unknown, 0);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active, queued]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.active_workers.insert("job_active".into());
    }

    let (snapshot, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("compatibility scheduler claim should work");

    assert!(tasks.is_empty());
    assert_eq!(snapshot.jobs.len(), 2);
    assert_eq!(snapshot.jobs[0].id, "job_active");
    assert_eq!(snapshot.jobs[1].id, "job_queued");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn scheduler_wake_guard_starts_first_run_and_coalesces_later_wakes() {
    let download_dir = test_runtime_dir("scheduler-wake-starts-coalesces");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);

    assert!(
        state.request_scheduler_wake(),
        "first scheduler wake should start a runner"
    );
    assert!(
        !state.request_scheduler_wake(),
        "second scheduler wake should be coalesced while a runner is active"
    );
    assert!(
        !state.request_scheduler_wake(),
        "additional scheduler wakes should keep coalescing into the pending pass"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn scheduler_wake_guard_consumes_pending_pass_then_releases_runner() {
    let download_dir = test_runtime_dir("scheduler-wake-finish");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);

    assert!(state.request_scheduler_wake());
    assert!(!state.request_scheduler_wake());
    assert!(
        state.complete_scheduler_run(),
        "finishing with a pending wake should keep the runner active"
    );
    assert!(
        !state.complete_scheduler_run(),
        "finishing without a pending wake should release the runner"
    );
    assert!(
        state.request_scheduler_wake(),
        "future scheduler wakes should start a new runner after release"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn scheduler_admission_index_preserves_mixed_queue_order() {
    fn numbered_fuckingfast_job(
        id: &str,
        archive: BulkArchiveInfo,
        part_number: u32,
    ) -> DownloadJob {
        let mut job = fuckingfast_bulk_job(id, archive);
        job.filename = format!("Game.part{part_number:03}.rar");
        job.url = format!("https://fuckingfast.co/{id}#{}", job.filename);
        job.resolved_from_url = Some(job.url.clone());
        job
    }

    let download_dir = test_runtime_dir("scheduler-index-mixed-order");
    let archive = bulk_archive_info(&download_dir, "bulk_scheduler_index_mixed");
    let normal_first = download_job("job_normal_1", JobState::Queued, ResumeSupport::Unknown, 0);
    let protected_late = numbered_fuckingfast_job("job_hoster_3", archive.clone(), 3);
    let mut direct_bulk = download_job(
        "job_direct_bulk",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    direct_bulk.bulk_archive = Some(archive.clone());
    let protected_first = numbered_fuckingfast_job("job_hoster_1", archive.clone(), 1);
    let normal_second = download_job("job_normal_2", JobState::Queued, ResumeSupport::Unknown, 0);
    let protected_middle = numbered_fuckingfast_job("job_hoster_2", archive, 2);
    let runtime = runtime_state_with_jobs(vec![
        normal_first,
        protected_late,
        direct_bulk,
        protected_first,
        normal_second,
        protected_middle,
    ]);

    let index = SchedulerAdmissionIndex::new(&runtime);
    let ordered_ids = index
        .scheduler_job_order()
        .iter()
        .map(|index| runtime.jobs[*index].id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        ordered_ids,
        vec![
            "job_normal_1",
            "job_hoster_1",
            "job_hoster_2",
            "job_hoster_3",
            "job_direct_bulk",
            "job_normal_2",
        ],
        "protected archive groups should expand at their first queue position while non-protected rows keep queue order"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn scheduler_admission_index_keeps_non_protected_rows_in_queue_order() {
    let download_dir = test_runtime_dir("scheduler-index-non-protected-order");
    let archive = bulk_archive_info(&download_dir, "bulk_scheduler_index_direct");
    let normal_first = download_job("job_normal_1", JobState::Queued, ResumeSupport::Unknown, 0);
    let mut direct_bulk = download_job(
        "job_direct_bulk",
        JobState::Queued,
        ResumeSupport::Unknown,
        0,
    );
    direct_bulk.bulk_archive = Some(archive);
    let normal_second = download_job("job_normal_2", JobState::Queued, ResumeSupport::Unknown, 0);
    let runtime = runtime_state_with_jobs(vec![normal_first, direct_bulk, normal_second]);

    let index = SchedulerAdmissionIndex::new(&runtime);
    let ordered_ids = index
        .scheduler_job_order()
        .iter()
        .map(|index| runtime.jobs[*index].id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        ordered_ids,
        vec!["job_normal_1", "job_direct_bulk", "job_normal_2"]
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn scheduler_admission_index_computes_acceleration_caps_by_hoster_key() {
    let download_dir = test_runtime_dir("scheduler-index-acceleration-caps");
    let archive = bulk_archive_info(&download_dir, "bulk_scheduler_index_caps");
    let datanodes = datanodes_bulk_job("job_datanodes_fast", archive.clone());
    let fuckingfast = fuckingfast_bulk_job("job_fuckingfast_fast", archive);
    let mut runtime = runtime_state_with_jobs(vec![datanodes, fuckingfast]);
    runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
    runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Fast;

    let datanodes_key = protected_bulk_hoster_fairness_key(
        runtime
            .job("job_datanodes_fast")
            .expect("DataNodes job should exist"),
    )
    .expect("DataNodes key should exist");
    let fuckingfast_key = protected_bulk_hoster_fairness_key(
        runtime
            .job("job_fuckingfast_fast")
            .expect("FuckingFast job should exist"),
    )
    .expect("FuckingFast key should exist");

    let index = SchedulerAdmissionIndex::new(&runtime);

    assert_eq!(index.accelerated_bulk_slot_floor(), 8);
    assert_eq!(index.max_adaptive_concurrency_for_key(&datanodes_key), 8);
    assert_eq!(index.max_adaptive_concurrency_for_key(&fuckingfast_key), 8);
    assert_eq!(
        index.max_adaptive_concurrency_for_key("https://missing.example:443"),
        BULK_HOSTER_MAX_ADAPTIVE_CONCURRENCY
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[test]
fn scheduler_claim_uses_single_admission_index() {
    let source = include_str!("../scheduler.rs");
    let claim = source
        .split("pub async fn claim_schedulable_jobs_for_scheduler")
        .nth(1)
        .expect("scheduler claim function should exist");

    assert!(
        claim.contains("let admission_index = SchedulerAdmissionIndex::new(&state);"),
        "scheduler claims should build the admission index once per pass"
    );
    assert!(
        claim.contains("admission_index.accelerated_bulk_slot_floor()"),
        "bulk slot floor should come from the admission index"
    );
    assert!(
        claim.contains("admission_index.max_adaptive_concurrency_for_key("),
        "per-key adaptive caps should come from the admission index"
    );
    assert!(
        claim.contains("admission_index.scheduler_job_order()"),
        "scheduler order should come from the admission index"
    );
}

#[test]
fn scheduler_source_does_not_keep_repeated_full_queue_scan_helpers() {
    let scheduler_source = include_str!("../scheduler.rs");
    let state_source = include_str!("../../state.rs");

    assert!(
        !scheduler_source.contains("fn accelerated_bulk_slot_floor(state: &RuntimeState)"),
        "accelerated bulk slot floor should not remain as a separate full-queue scan"
    );
    assert!(
        !state_source.contains("fn protected_bulk_hoster_max_adaptive_concurrency_for_key("),
        "per-key adaptive caps should not remain as a separate full-queue scan"
    );
}

#[tokio::test]
async fn scheduler_claim_diagnostic_is_memory_visible_and_persisted() {
    let download_dir = test_runtime_dir("diagnostic-events-scheduler-claim");
    let job = download_job("job_1", JobState::Queued, ResumeSupport::Unknown, 0);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let claim = state.claim_schedulable_jobs_for_scheduler().await.unwrap();

    assert_eq!(claim.tasks.len(), 1);
    let snapshot = state
        .diagnostics_snapshot(HostRegistrationDiagnostics {
            status: HostRegistrationStatus::Configured,
            entries: Vec::new(),
        })
        .await;
    assert!(snapshot.recent_events.iter().any(|event| {
        event.category == "download"
            && event.job_id.as_deref() == Some("job_1")
            && event.message == "Starting job_1"
    }));

    let mut history = Vec::new();
    for _ in 0..20 {
        history = state
            .diagnostic_event_history()
            .await
            .expect("diagnostic event history should load");
        if history.iter().any(|event| {
            event.category == "download"
                && event.job_id.as_deref() == Some("job_1")
                && event.message == "Starting job_1"
        }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(history.iter().any(|event| {
        event.category == "download"
            && event.job_id.as_deref() == Some("job_1")
            && event.message == "Starting job_1"
    }));

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
async fn scheduler_deferred_hoster_blocks_same_origin_but_allows_other_origin_and_direct_bulk() {
    let download_dir = test_runtime_dir("bulk-hoster-admission-defer");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_admission_defer");
    let mut deferred = datanodes_bulk_job("job_datanodes_1", archive.clone());
    deferred.url = "https://datanodes.to/shared123/Game.part01.rar".into();
    deferred.resolved_from_url = Some(deferred.url.clone());
    let mut same_origin = datanodes_bulk_job("job_datanodes_2", archive.clone());
    same_origin.url = "https://datanodes.to/shared456/Game.part02.rar".into();
    same_origin.resolved_from_url = Some(same_origin.url.clone());
    let other_origin = fuckingfast_bulk_job("job_fuckingfast_1", archive.clone());
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
        vec![deferred, same_origin, other_origin, direct_bulk],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 4;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.download_admission_defers.insert(
            "job_datanodes_1".into(),
            DownloadAdmissionDefer {
                until: Instant::now() + Duration::from_secs(30),
                reason: "segment connection budget is full".into(),
            },
        );
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");
    let task_ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(task_ids, vec!["job_fuckingfast_1", "job_direct_bulk"]);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn scheduler_reclaims_job_after_admission_defer_expires() {
    let download_dir = test_runtime_dir("bulk-hoster-admission-defer-expired");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_admission_defer_expired");
    let job = datanodes_bulk_job("job_datanodes_1", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 1;
        runtime.download_admission_defers.insert(
            "job_datanodes_1".into(),
            DownloadAdmissionDefer {
                until: Instant::now() - Duration::from_secs(1),
                reason: "expired segment connection budget wait".into(),
            },
        );
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming jobs should work");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "job_datanodes_1");
    assert!(!state
        .inner
        .read()
        .await
        .download_admission_defers
        .contains_key("job_datanodes_1"));

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn defer_active_job_releases_worker_and_records_bounded_defer() {
    let download_dir = test_runtime_dir("bulk-admission-defer-active-job");
    let mut job = download_job(
        "job_direct_bulk",
        JobState::Starting,
        ResumeSupport::Supported,
        25,
    );
    job.bulk_archive = Some(bulk_archive_info(&download_dir, "bulk_admission_defer"));
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);
    {
        let mut runtime = state.inner.write().await;
        runtime.active_workers.insert("job_direct_bulk".into());
    }

    let snapshot = state
        .defer_active_job(
            "job_direct_bulk",
            "segment connection budget is full".into(),
            Duration::from_secs(2),
        )
        .await
        .expect("active job should be deferred");

    let deferred_job = snapshot
        .jobs
        .iter()
        .find(|job| job.id == "job_direct_bulk")
        .expect("job should remain in snapshot");
    assert_eq!(deferred_job.state, JobState::Queued);
    assert_eq!(deferred_job.speed, 0);
    let runtime = state.inner.read().await;
    assert!(!runtime.active_workers.contains("job_direct_bulk"));
    let defer = runtime
        .download_admission_defers
        .get("job_direct_bulk")
        .expect("defer should be tracked");
    assert!(defer.until > Instant::now());
    assert!(defer.reason.contains("segment connection budget"));
    assert!(runtime
        .diagnostic_events
        .iter()
        .any(|event| event.message.contains("segment connection budget")));

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
async fn accelerated_datanodes_fairness_expands_fast_window_after_recent_healthy_progress() {
    let download_dir = test_runtime_dir("bulk-datanodes-fast-ramp");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_fast_ramp");
    let jobs = (1..=8)
        .map(|index| datanodes_bulk_job(&format!("job_datanodes_{index}"), archive.clone()))
        .collect::<Vec<_>>();
    let state = shared_state_with_jobs(download_dir.join("state.json"), jobs);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 8;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Fast;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
    }

    let (_, first_tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("initial DataNodes claim should work");
    assert_eq!(first_tasks.len(), 1);
    assert_eq!(first_tasks[0].id, "job_datanodes_1");

    {
        let mut runtime = state.inner.write().await;
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_1",
            512 * 1024,
            Duration::from_secs(5),
            3,
        );
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("accelerated DataNodes claim should expand");
    let task_ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        task_ids,
        vec![
            "job_datanodes_2",
            "job_datanodes_3",
            "job_datanodes_4",
            "job_datanodes_5",
            "job_datanodes_6",
            "job_datanodes_7",
            "job_datanodes_8",
        ],
        "fast protected hoster batches should expand the contiguous archive window once the active worker is healthy but aggregate speed is low"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_datanodes_balanced_requires_priority_runway_before_next_claim() {
    let download_dir = test_runtime_dir("bulk-datanodes-balanced-priority-runway");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_balanced_runway");
    let mut active = datanodes_bulk_job("job_datanodes_1", archive.clone());
    active.state = JobState::Downloading;
    active.speed = 512 * 1024;
    active.downloaded_bytes = 4 * 1024 * 1024;
    let queued = datanodes_bulk_job("job_datanodes_2", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active, queued]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_datanodes_1".into());
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_1",
            512 * 1024,
            Duration::from_secs(7),
            3,
        );
    }

    let (_, early_tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming during runway should work");
    assert!(early_tasks.is_empty());

    {
        let mut runtime = state.inner.write().await;
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_1",
            512 * 1024,
            Duration::from_secs(8),
            3,
        );
    }
    let (_, ready_tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming after runway should work");
    assert_eq!(ready_tasks.len(), 1);
    assert_eq!(ready_tasks[0].id, "job_datanodes_2");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_datanodes_balanced_ramps_after_slow_positive_progress_samples() {
    let download_dir = test_runtime_dir("bulk-datanodes-balanced-slow-progress-ramp");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_balanced_slow_progress");
    let mut active = datanodes_bulk_job("job_datanodes_1", archive.clone());
    active.state = JobState::Downloading;
    active.speed = 24 * 1024;
    active.downloaded_bytes = 512 * 1024;
    let queued = datanodes_bulk_job("job_datanodes_2", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active, queued]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_datanodes_1".into());
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_1",
            24 * 1024,
            Duration::from_secs(8),
            2,
        );
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming after slow DataNodes progress should work");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "job_datanodes_2");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_datanodes_balanced_stalled_progress_blocks_next_claim() {
    let download_dir = test_runtime_dir("bulk-datanodes-balanced-stalled-progress");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_balanced_stalled");
    let mut active = datanodes_bulk_job("job_datanodes_1", archive.clone());
    active.state = JobState::Downloading;
    active.speed = 0;
    active.downloaded_bytes = 512 * 1024;
    let queued = datanodes_bulk_job("job_datanodes_2", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active, queued]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_datanodes_1".into());
        let now = Instant::now();
        let profile = datanodes_accelerated_hoster_concurrency(
            &runtime.settings,
            runtime
                .job("job_datanodes_1")
                .expect("DataNodes job should exist"),
        )
        .map(|max_concurrency| BulkHosterWorkerProfile::Accelerated { max_concurrency })
        .expect("DataNodes job should be accelerated");
        let job = runtime
            .job("job_datanodes_1")
            .expect("DataNodes job should exist");
        let mut health = BulkHosterWorkerHealth::from_job_with_profile(
            job,
            profile,
            now - Duration::from_secs(8),
        );
        health.mark_transferring(job.downloaded_bytes, now - Duration::from_secs(8));
        runtime
            .bulk_hoster_worker_health
            .insert("job_datanodes_1".into(), health);
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming after stalled DataNodes progress should work");

    assert!(tasks.is_empty());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_fuckingfast_balanced_ramps_after_slow_positive_progress_samples() {
    let download_dir = test_runtime_dir("bulk-fuckingfast-balanced-slow-progress-ramp");
    let archive = bulk_archive_info(&download_dir, "bulk_fuckingfast_balanced_slow_progress");
    let mut active = fuckingfast_bulk_job("job_fuckingfast_1", archive.clone());
    active.state = JobState::Downloading;
    active.speed = 24 * 1024;
    active.downloaded_bytes = 512 * 1024;
    let queued = fuckingfast_bulk_job("job_fuckingfast_2", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active, queued]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_fuckingfast_1".into());
        seed_accelerated_hoster_health(
            &mut runtime,
            "job_fuckingfast_1",
            24 * 1024,
            Duration::from_secs(8),
            2,
        );
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming after slow FuckingFast progress should work");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "job_fuckingfast_2");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_fuckingfast_throughput_rescue_expands_same_archive_window() {
    fn numbered_fuckingfast_job(
        id: &str,
        archive: BulkArchiveInfo,
        part_number: u32,
    ) -> DownloadJob {
        let mut job = fuckingfast_bulk_job(id, archive);
        job.filename = format!("WWE_2K25.part{part_number:03}.rar");
        job.url = format!("https://fuckingfast.co/{id}#{}", job.filename);
        job.resolved_from_url = Some(job.url.clone());
        job.total_bytes = 500 * 1024 * 1024;
        job
    }

    let download_dir = test_runtime_dir("bulk-fuckingfast-throughput-rescue");
    let archive = bulk_archive_info(&download_dir, "bulk_fuckingfast_throughput_rescue");
    let mut active = numbered_fuckingfast_job("job_fuckingfast_17", archive.clone(), 17);
    active.state = JobState::Downloading;
    active.speed = 303 * 1024;
    active.downloaded_bytes = 8 * 1024 * 1024;

    let mut jobs = vec![active];
    for part_number in 18..=23 {
        jobs.push(numbered_fuckingfast_job(
            &format!("job_fuckingfast_{part_number}"),
            archive.clone(),
            part_number,
        ));
    }

    let state = shared_state_with_jobs(download_dir.join("state.json"), jobs);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 2;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_fuckingfast_17".into());
        seed_accelerated_hoster_health(
            &mut runtime,
            "job_fuckingfast_17",
            303 * 1024,
            Duration::from_secs(8),
            2,
        );
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming during FuckingFast throughput rescue should work");
    let task_ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();

    assert!(
        task_ids.len() >= 3,
        "slow protected-hoster batches should expand past the configured two-file floor; got {task_ids:?}"
    );
    assert_eq!(
        &task_ids[..3],
        [
            "job_fuckingfast_18",
            "job_fuckingfast_19",
            "job_fuckingfast_20",
        ],
        "throughput rescue should keep the same archive window contiguous"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn archive_window_schedules_earliest_unfinished_part_before_later_parts() {
    fn numbered_fuckingfast_job(
        id: &str,
        archive: BulkArchiveInfo,
        part_number: u32,
    ) -> DownloadJob {
        let mut job = fuckingfast_bulk_job(id, archive);
        job.filename = format!("WWE_2K25.part{part_number:03}.rar");
        job.url = format!("https://fuckingfast.co/{id}#{}", job.filename);
        job.resolved_from_url = Some(job.url.clone());
        job.total_bytes = 500 * 1024 * 1024;
        job
    }

    let download_dir = test_runtime_dir("bulk-fuckingfast-archive-window-order");
    let archive = bulk_archive_info(&download_dir, "bulk_fuckingfast_archive_window_order");
    let mut active = numbered_fuckingfast_job("job_fuckingfast_17", archive.clone(), 17);
    active.state = JobState::Downloading;
    active.speed = 5 * 1024 * 1024;
    active.downloaded_bytes = 32 * 1024 * 1024;
    let later = numbered_fuckingfast_job("job_fuckingfast_19", archive.clone(), 19);
    let earlier = numbered_fuckingfast_job("job_fuckingfast_18", archive, 18);

    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![active, later, earlier],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 6;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_fuckingfast_17".into());
        seed_accelerated_hoster_health(
            &mut runtime,
            "job_fuckingfast_17",
            5 * 1024 * 1024,
            Duration::from_secs(8),
            3,
        );
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming with out-of-order multipart rows should work");

    assert_eq!(tasks.len(), 1);
    assert_eq!(
        tasks[0].id, "job_fuckingfast_18",
        "same-archive multipart rows must not let a later queued part leapfrog the earliest unfinished part"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_fuckingfast_balanced_stalled_progress_blocks_next_claim() {
    let download_dir = test_runtime_dir("bulk-fuckingfast-balanced-stalled-progress");
    let archive = bulk_archive_info(&download_dir, "bulk_fuckingfast_balanced_stalled");
    let mut active = fuckingfast_bulk_job("job_fuckingfast_1", archive.clone());
    active.state = JobState::Downloading;
    active.speed = 0;
    active.downloaded_bytes = 512 * 1024;
    let queued = fuckingfast_bulk_job("job_fuckingfast_2", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active, queued]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_fuckingfast_1".into());
        let now = Instant::now();
        let job = runtime
            .job("job_fuckingfast_1")
            .expect("FuckingFast job should exist");
        let mut health = BulkHosterWorkerHealth::from_job_with_profile(
            job,
            bulk_hoster_worker_profile_for_job(&runtime.settings, job),
            now - Duration::from_secs(8),
        );
        health.mark_transferring(job.downloaded_bytes, now - Duration::from_secs(8));
        runtime
            .bulk_hoster_worker_health
            .insert("job_fuckingfast_1".into(), health);
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming after stalled FuckingFast progress should work");

    assert!(tasks.is_empty());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_datanodes_fast_requires_shorter_priority_runway_before_next_claim() {
    let download_dir = test_runtime_dir("bulk-datanodes-fast-priority-runway");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_fast_runway");
    let mut active = datanodes_bulk_job("job_datanodes_1", archive.clone());
    active.state = JobState::Downloading;
    active.speed = 512 * 1024;
    active.downloaded_bytes = 4 * 1024 * 1024;
    let queued = datanodes_bulk_job("job_datanodes_2", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active, queued]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 8;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Fast;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_datanodes_1".into());
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_1",
            512 * 1024,
            Duration::from_secs(4),
            3,
        );
    }

    let (_, early_tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming during fast runway should work");
    assert!(early_tasks.is_empty());

    {
        let mut runtime = state.inner.write().await;
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_1",
            512 * 1024,
            Duration::from_secs(5),
            3,
        );
    }
    let (_, ready_tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming after fast runway should work");
    assert_eq!(ready_tasks.len(), 1);
    assert_eq!(ready_tasks[0].id, "job_datanodes_2");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn datanodes_priority_defer_cooldown_skips_then_releases_queued_job() {
    let download_dir = test_runtime_dir("bulk-datanodes-priority-cooldown");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_priority_cooldown");
    let queued = datanodes_bulk_job("job_datanodes_deferred", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![queued]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.defer_datanodes_priority_job_until(
            "job_datanodes_deferred",
            Instant::now() + Duration::from_secs(20),
        );
    }

    let (_, skipped_tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming during priority cooldown should work");
    assert!(skipped_tasks.is_empty());

    {
        let mut runtime = state.inner.write().await;
        runtime.defer_datanodes_priority_job_until(
            "job_datanodes_deferred",
            Instant::now() - Duration::from_secs(1),
        );
    }
    let (_, released_tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming after priority cooldown should work");
    assert_eq!(released_tasks.len(), 1);
    assert_eq!(released_tasks[0].id, "job_datanodes_deferred");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn datanodes_priority_defer_cooldown_does_not_let_later_same_host_leapfrog() {
    let download_dir = test_runtime_dir("bulk-datanodes-priority-cooldown-fifo");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_priority_cooldown_fifo");
    let first = datanodes_bulk_job("job_datanodes_1", archive.clone());
    let second = datanodes_bulk_job("job_datanodes_2", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![first, second]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.defer_datanodes_priority_job_until(
            "job_datanodes_1",
            Instant::now() + Duration::from_secs(20),
        );
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming during priority cooldown should work");
    assert!(
        tasks.is_empty(),
        "later same-host DataNodes rows must not start ahead of an earlier deferred row"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn datanodes_oldest_priority_pressure_blocks_new_hoster_but_allows_direct_bulk() {
    let download_dir = test_runtime_dir("bulk-datanodes-priority-pressure");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_priority_pressure");
    let mut older = datanodes_bulk_job("job_datanodes_1", archive.clone());
    older.state = JobState::Downloading;
    older.speed = 48 * 1024;
    older.downloaded_bytes = 8 * 1024 * 1024;
    let mut newer = datanodes_bulk_job("job_datanodes_2", archive.clone());
    newer.state = JobState::Downloading;
    newer.speed = 512 * 1024;
    newer.downloaded_bytes = 2 * 1024 * 1024;
    let queued_hoster = datanodes_bulk_job("job_datanodes_3", archive.clone());
    let mut direct_bulk = download_job(
        "job_direct_bulk",
        JobState::Queued,
        ResumeSupport::Supported,
        0,
    );
    direct_bulk.url = "https://cdn.example.com/direct.bin".into();
    direct_bulk.bulk_archive = Some(archive);
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![older, newer, queued_hoster, direct_bulk],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_datanodes_1".into());
        runtime.active_workers.insert("job_datanodes_2".into());
        seed_pressured_older_datanodes_health(&mut runtime, "job_datanodes_1");
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_2",
            512 * 1024,
            Duration::from_secs(8),
            3,
        );
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming under priority pressure should work");
    let task_ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(task_ids, vec!["job_direct_bulk"]);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn datanodes_priority_pressure_allows_other_hoster_but_blocks_same_hoster() {
    let download_dir = test_runtime_dir("bulk-datanodes-priority-pressure-other-hoster");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_priority_pressure_other");
    let mut older = datanodes_bulk_job("job_datanodes_1", archive.clone());
    older.state = JobState::Downloading;
    older.speed = 48 * 1024;
    older.downloaded_bytes = 8 * 1024 * 1024;
    let mut newer = datanodes_bulk_job("job_datanodes_2", archive.clone());
    newer.state = JobState::Downloading;
    newer.speed = 512 * 1024;
    newer.downloaded_bytes = 2 * 1024 * 1024;
    let queued_datanodes = datanodes_bulk_job("job_datanodes_3", archive.clone());
    let queued_fuckingfast = protected_hoster_bulk_job("job_fuckingfast_1", archive);
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![older, newer, queued_datanodes, queued_fuckingfast],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.max_concurrent_downloads = 1;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_datanodes_1".into());
        runtime.active_workers.insert("job_datanodes_2".into());
        seed_pressured_older_datanodes_health(&mut runtime, "job_datanodes_1");
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_2",
            512 * 1024,
            Duration::from_secs(8),
            3,
        );
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming under DataNodes pressure should work");
    let task_ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(task_ids, vec!["job_fuckingfast_1"]);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_datanodes_claims_only_one_same_key_worker_per_scheduler_pass() {
    let download_dir = test_runtime_dir("bulk-datanodes-one-per-pass");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_one_per_pass");
    let mut jobs = Vec::new();
    for index in 1..=4 {
        let mut job = datanodes_bulk_job(&format!("job_datanodes_{index}"), archive.clone());
        job.state = JobState::Downloading;
        job.speed = 512 * 1024;
        job.downloaded_bytes = 4 * 1024 * 1024;
        jobs.push(job);
    }
    jobs.push(datanodes_bulk_job("job_datanodes_5", archive.clone()));
    jobs.push(datanodes_bulk_job("job_datanodes_6", archive));
    let state = shared_state_with_jobs(download_dir.join("state.json"), jobs);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 8;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Fast;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        for index in 1..=4 {
            let id = format!("job_datanodes_{index}");
            runtime.active_workers.insert(id.clone());
            seed_accelerated_datanodes_health(
                &mut runtime,
                &id,
                512 * 1024,
                Duration::from_secs(5),
                3,
            );
        }
        runtime.bulk_hoster_fairness.insert(
            "https://datanodes.to:443".into(),
            BulkHosterFairnessController {
                target_active: 8,
                aggregate_baseline_speed: Some(4 * 512 * 1024),
                degraded_since: None,
                cooldown_until: None,
                last_freeze_reported_at: None,
            },
        );
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("claiming with high adaptive target should work");
    let task_ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(task_ids, vec!["job_datanodes_5"]);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn datanodes_priority_pressure_blocks_same_key_warmup_candidates() {
    let download_dir = test_runtime_dir("bulk-datanodes-priority-warmup-blocked");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_priority_warmup_blocked");
    let mut older = datanodes_bulk_job("job_datanodes_1", archive.clone());
    older.state = JobState::Downloading;
    older.speed = 48 * 1024;
    older.downloaded_bytes = 8 * 1024 * 1024;
    let mut newer = datanodes_bulk_job("job_datanodes_2", archive.clone());
    newer.state = JobState::Downloading;
    newer.speed = 512 * 1024;
    newer.downloaded_bytes = 2 * 1024 * 1024;
    let queued = datanodes_bulk_job("job_datanodes_3", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![older, newer, queued]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.active_workers.insert("job_datanodes_1".into());
        runtime.active_workers.insert("job_datanodes_2".into());
        seed_pressured_older_datanodes_health(&mut runtime, "job_datanodes_1");
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_2",
            512 * 1024,
            Duration::from_secs(8),
            3,
        );
    }

    assert!(
        state.datanodes_hoster_warmup_candidates().await.is_empty(),
        "warmup must not resolve later DataNodes rows while same-key priority pressure is active"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_datanodes_workers_are_not_cascade_throttled() {
    let download_dir = test_runtime_dir("bulk-datanodes-priority-throttle-cap");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_priority_throttle_cap");
    let mut older = datanodes_bulk_job("job_datanodes_1", archive.clone());
    older.state = JobState::Downloading;
    older.speed = 48 * 1024;
    older.downloaded_bytes = 8 * 1024 * 1024;
    let mut newer = datanodes_bulk_job("job_datanodes_2", archive);
    newer.state = JobState::Downloading;
    newer.speed = 512 * 1024;
    newer.downloaded_bytes = 2 * 1024 * 1024;
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![older, newer]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_datanodes_1".into());
        runtime.active_workers.insert("job_datanodes_2".into());
        seed_pressured_older_datanodes_health(&mut runtime, "job_datanodes_1");
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_2",
            512 * 1024,
            Duration::from_secs(8),
            3,
        );
    }

    let decision = state
        .hoster_priority_throttle_decision("job_datanodes_2")
        .await;
    assert!(
        decision.is_none(),
        "accelerated DataNodes workers should rely on admission control instead of cascade throttling"
    );

    let protected_decision = state
        .hoster_priority_throttle_decision("job_datanodes_1")
        .await;
    assert!(
        protected_decision.is_none(),
        "the protected oldest worker must not throttle itself"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_fuckingfast_workers_are_not_cascade_throttled() {
    let download_dir = test_runtime_dir("bulk-fuckingfast-priority-throttle-cap");
    let archive = bulk_archive_info(&download_dir, "bulk_fuckingfast_priority_throttle_cap");
    let mut older = fuckingfast_bulk_job("job_fuckingfast_1", archive.clone());
    older.state = JobState::Downloading;
    older.speed = 48 * 1024;
    older.downloaded_bytes = 8 * 1024 * 1024;
    let mut newer = fuckingfast_bulk_job("job_fuckingfast_2", archive);
    newer.state = JobState::Downloading;
    newer.speed = 512 * 1024;
    newer.downloaded_bytes = 2 * 1024 * 1024;
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![older, newer]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_fuckingfast_1".into());
        runtime.active_workers.insert("job_fuckingfast_2".into());
        seed_priority_hoster_health(
            &mut runtime,
            "job_fuckingfast_1",
            1024 * 1024,
            Duration::from_secs(30),
            3,
        );
        seed_priority_hoster_health(
            &mut runtime,
            "job_fuckingfast_2",
            512 * 1024,
            Duration::from_secs(8),
            3,
        );
    }

    let decision = state
        .hoster_priority_throttle_decision("job_fuckingfast_2")
        .await;
    assert!(
        decision.is_none(),
        "accelerated FuckingFast workers should rely on admission control instead of cascade throttling"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn hoster_priority_cascade_halves_each_newer_worker_in_same_archive_and_hoster() {
    let download_dir = test_runtime_dir("bulk-hoster-priority-cascade");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_priority_cascade");
    let mut oldest = protected_hoster_bulk_job("job_hoster_1", archive.clone());
    oldest.state = JobState::Downloading;
    oldest.speed = 1024 * 1024;
    oldest.downloaded_bytes = 16 * 1024 * 1024;
    let mut second = protected_hoster_bulk_job("job_hoster_2", archive.clone());
    second.state = JobState::Downloading;
    second.speed = 1024 * 1024;
    second.downloaded_bytes = 8 * 1024 * 1024;
    let mut third = protected_hoster_bulk_job("job_hoster_3", archive);
    third.state = JobState::Downloading;
    third.speed = 1024 * 1024;
    third.downloaded_bytes = 4 * 1024 * 1024;
    let state =
        shared_state_with_jobs(download_dir.join("state.json"), vec![oldest, second, third]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        for (id, transfer_age) in [
            ("job_hoster_1", Duration::from_secs(30)),
            ("job_hoster_2", Duration::from_secs(20)),
            ("job_hoster_3", Duration::from_secs(10)),
        ] {
            runtime.active_workers.insert(id.into());
            seed_priority_hoster_health(&mut runtime, id, 1024 * 1024, transfer_age, 3);
        }
    }

    assert!(
        state
            .hoster_priority_throttle_decision("job_hoster_1")
            .await
            .is_none(),
        "oldest worker should never throttle itself"
    );
    let second_decision = state
        .hoster_priority_throttle_decision("job_hoster_2")
        .await
        .expect("first newer worker should be throttled");
    assert_eq!(second_decision.protected_job_id, "job_hoster_1");
    assert_eq!(second_decision.reference_bytes_per_second, 1024 * 1024);
    assert_eq!(second_decision.cap_bytes_per_second, 512 * 1024);

    let third_decision = state
        .hoster_priority_throttle_decision("job_hoster_3")
        .await
        .expect("second newer worker should be throttled");
    assert_eq!(third_decision.protected_job_id, "job_hoster_2");
    assert_eq!(third_decision.reference_bytes_per_second, 512 * 1024);
    assert_eq!(third_decision.cap_bytes_per_second, 256 * 1024);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn hoster_priority_cascade_isolated_by_archive_and_hoster_origin() {
    let download_dir = test_runtime_dir("bulk-hoster-priority-isolated");
    let archive_a = bulk_archive_info(&download_dir, "bulk_hoster_priority_a");
    let archive_b = bulk_archive_info(&download_dir, "bulk_hoster_priority_b");
    let mut ff_a = protected_hoster_bulk_job("job_ff_a", archive_a.clone());
    ff_a.state = JobState::Downloading;
    ff_a.speed = 1024 * 1024;
    ff_a.downloaded_bytes = 8 * 1024 * 1024;
    let mut ff_b = protected_hoster_bulk_job("job_ff_b", archive_b);
    ff_b.state = JobState::Downloading;
    ff_b.speed = 1024 * 1024;
    ff_b.downloaded_bytes = 8 * 1024 * 1024;
    let mut datanodes_a = datanodes_bulk_job("job_datanodes_a", archive_a);
    datanodes_a.state = JobState::Downloading;
    datanodes_a.speed = 1024 * 1024;
    datanodes_a.downloaded_bytes = 8 * 1024 * 1024;
    let state = shared_state_with_jobs(
        download_dir.join("state.json"),
        vec![ff_a, ff_b, datanodes_a],
    );
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        for (id, transfer_age) in [
            ("job_ff_a", Duration::from_secs(30)),
            ("job_ff_b", Duration::from_secs(20)),
            ("job_datanodes_a", Duration::from_secs(10)),
        ] {
            runtime.active_workers.insert(id.into());
            seed_priority_hoster_health(&mut runtime, id, 1024 * 1024, transfer_age, 3);
        }
    }

    for id in ["job_ff_a", "job_ff_b", "job_datanodes_a"] {
        assert!(
            state.hoster_priority_throttle_decision(id).await.is_none(),
            "{id} should not be throttled by another archive or hoster origin"
        );
    }

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn hoster_priority_cascade_uses_baseline_when_older_live_sample_is_zero() {
    let download_dir = test_runtime_dir("bulk-hoster-priority-zero-fallback");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_priority_zero");
    let mut oldest = protected_hoster_bulk_job("job_hoster_1", archive.clone());
    oldest.state = JobState::Downloading;
    oldest.speed = 0;
    oldest.downloaded_bytes = 16 * 1024 * 1024;
    let mut second = protected_hoster_bulk_job("job_hoster_2", archive);
    second.state = JobState::Downloading;
    second.speed = 1024 * 1024;
    second.downloaded_bytes = 8 * 1024 * 1024;
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![oldest, second]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_hoster_1".into());
        runtime.active_workers.insert("job_hoster_2".into());
        seed_priority_hoster_health(
            &mut runtime,
            "job_hoster_1",
            1024 * 1024,
            Duration::from_secs(30),
            3,
        );
        let downloaded = runtime
            .job("job_hoster_1")
            .expect("oldest job should exist")
            .downloaded_bytes;
        runtime.update_bulk_hoster_worker_health("job_hoster_1", downloaded, 0, Instant::now());
        runtime
            .job_mut("job_hoster_1")
            .expect("oldest job should exist")
            .speed = 0;
        seed_priority_hoster_health(
            &mut runtime,
            "job_hoster_2",
            1024 * 1024,
            Duration::from_secs(20),
            3,
        );
    }

    let decision = state
        .hoster_priority_throttle_decision("job_hoster_2")
        .await
        .expect("newer worker should use the older worker baseline instead of a zero live sample");
    assert_eq!(decision.reference_bytes_per_second, 1024 * 1024);
    assert_eq!(decision.cap_bytes_per_second, 512 * 1024);

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn hoster_priority_cascade_waits_when_older_worker_has_no_usable_speed() {
    let download_dir = test_runtime_dir("bulk-hoster-priority-no-baseline");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_priority_no_baseline");
    let mut oldest = protected_hoster_bulk_job("job_hoster_1", archive.clone());
    oldest.state = JobState::Downloading;
    oldest.speed = 0;
    let mut second = protected_hoster_bulk_job("job_hoster_2", archive);
    second.state = JobState::Downloading;
    second.speed = 1024 * 1024;
    second.downloaded_bytes = 8 * 1024 * 1024;
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![oldest, second]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_hoster_1".into());
        runtime.active_workers.insert("job_hoster_2".into());
        let now = Instant::now();
        let oldest_job = runtime
            .job("job_hoster_1")
            .expect("oldest job should exist");
        let mut health =
            BulkHosterWorkerHealth::from_job(oldest_job, now - Duration::from_secs(30));
        health.mark_transferring(oldest_job.downloaded_bytes, now - Duration::from_secs(30));
        runtime
            .bulk_hoster_worker_health
            .insert("job_hoster_1".into(), health);
        seed_priority_hoster_health(
            &mut runtime,
            "job_hoster_2",
            1024 * 1024,
            Duration::from_secs(20),
            3,
        );
    }

    assert!(
        state
            .hoster_priority_throttle_decision("job_hoster_2")
            .await
            .is_none(),
        "newer workers should not be capped until the older worker has live speed or baseline"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn hoster_priority_cascade_is_disabled_when_hoster_fairness_is_off() {
    let download_dir = test_runtime_dir("bulk-hoster-priority-off");
    let archive = bulk_archive_info(&download_dir, "bulk_hoster_priority_off");
    let mut oldest = protected_hoster_bulk_job("job_hoster_1", archive.clone());
    oldest.state = JobState::Downloading;
    oldest.speed = 1024 * 1024;
    oldest.downloaded_bytes = 16 * 1024 * 1024;
    let mut second = protected_hoster_bulk_job("job_hoster_2", archive);
    second.state = JobState::Downloading;
    second.speed = 1024 * 1024;
    second.downloaded_bytes = 8 * 1024 * 1024;
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![oldest, second]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Off;
        runtime.active_workers.insert("job_hoster_1".into());
        runtime.active_workers.insert("job_hoster_2".into());
        seed_priority_hoster_health(
            &mut runtime,
            "job_hoster_1",
            1024 * 1024,
            Duration::from_secs(30),
            3,
        );
        seed_priority_hoster_health(
            &mut runtime,
            "job_hoster_2",
            1024 * 1024,
            Duration::from_secs(20),
            3,
        );
    }

    assert!(
        state
            .hoster_priority_throttle_decision("job_hoster_2")
            .await
            .is_none(),
        "hoster fairness off should disable the cascade throttle escape hatch"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_datanodes_without_baseline_are_not_cascade_throttled() {
    let download_dir = test_runtime_dir("bulk-datanodes-priority-throttle-no-baseline");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_priority_no_baseline");
    let mut older = datanodes_bulk_job("job_datanodes_1", archive.clone());
    older.state = JobState::Downloading;
    older.speed = 32 * 1024;
    older.downloaded_bytes = 512 * 1024;
    let mut newer = datanodes_bulk_job("job_datanodes_2", archive);
    newer.state = JobState::Downloading;
    newer.speed = 512 * 1024;
    newer.downloaded_bytes = 2 * 1024 * 1024;
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![older, newer]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.active_workers.insert("job_datanodes_1".into());
        runtime.active_workers.insert("job_datanodes_2".into());
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_1",
            32 * 1024,
            Duration::from_secs(30),
            3,
        );
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_2",
            512 * 1024,
            Duration::from_secs(8),
            3,
        );
    }

    let decision = state
        .hoster_priority_throttle_decision("job_datanodes_2")
        .await;
    assert!(
        decision.is_none(),
        "accelerated DataNodes workers should not be capped even before a baseline exists"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_datanodes_stay_uncapped_after_older_worker_recovers() {
    let download_dir = test_runtime_dir("bulk-datanodes-priority-throttle-release");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_priority_throttle_release");
    let mut older = datanodes_bulk_job("job_datanodes_1", archive.clone());
    older.state = JobState::Downloading;
    older.speed = 48 * 1024;
    older.downloaded_bytes = 8 * 1024 * 1024;
    let mut newer = datanodes_bulk_job("job_datanodes_2", archive);
    newer.state = JobState::Downloading;
    newer.speed = 512 * 1024;
    newer.downloaded_bytes = 2 * 1024 * 1024;
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![older, newer]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.active_workers.insert("job_datanodes_1".into());
        runtime.active_workers.insert("job_datanodes_2".into());
        seed_pressured_older_datanodes_health(&mut runtime, "job_datanodes_1");
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_2",
            512 * 1024,
            Duration::from_secs(8),
            3,
        );
        for _ in 0..2 {
            let downloaded = runtime
                .job("job_datanodes_1")
                .expect("older job should exist")
                .downloaded_bytes
                .saturating_add(900 * 1024);
            runtime.update_bulk_hoster_worker_health(
                "job_datanodes_1",
                downloaded,
                900 * 1024,
                Instant::now(),
            );
            runtime
                .job_mut("job_datanodes_1")
                .expect("older job should exist")
                .downloaded_bytes = downloaded;
        }
    }

    assert!(
        state
            .hoster_priority_throttle_decision("job_datanodes_2")
            .await
            .is_none(),
        "accelerated DataNodes workers should stay uncapped during partial recovery"
    );

    {
        let mut runtime = state.inner.write().await;
        let downloaded = runtime
            .job("job_datanodes_1")
            .expect("older job should exist")
            .downloaded_bytes
            .saturating_add(900 * 1024);
        runtime.update_bulk_hoster_worker_health(
            "job_datanodes_1",
            downloaded,
            900 * 1024,
            Instant::now(),
        );
        runtime
            .job_mut("job_datanodes_1")
            .expect("older job should exist")
            .downloaded_bytes = downloaded;
    }

    let decision = state
        .hoster_priority_throttle_decision("job_datanodes_2")
        .await;
    assert!(
        decision.is_none(),
        "accelerated DataNodes workers should stay uncapped after older worker recovery"
    );

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_datanodes_transient_zero_sample_does_not_freeze_claims() {
    let download_dir = test_runtime_dir("bulk-datanodes-transient-zero-sample");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_transient_zero");
    let mut active = datanodes_bulk_job("job_datanodes_1", archive.clone());
    active.state = JobState::Downloading;
    active.downloaded_bytes = 4 * 1024 * 1024;
    active.speed = 512 * 1024;
    let queued = datanodes_bulk_job("job_datanodes_2", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active, queued]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_datanodes_1".into());
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_1",
            512 * 1024,
            Duration::from_secs(8),
            3,
        );
        let downloaded = runtime.job("job_datanodes_1").unwrap().downloaded_bytes;
        runtime.update_bulk_hoster_worker_health("job_datanodes_1", downloaded, 0, Instant::now());
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("accelerated DataNodes claim should tolerate one zero sample");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "job_datanodes_2");

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn accelerated_datanodes_transient_zero_before_runway_still_blocks_claims() {
    let download_dir = test_runtime_dir("bulk-datanodes-transient-zero-before-runway");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_transient_zero_before");
    let mut active = datanodes_bulk_job("job_datanodes_1", archive.clone());
    active.state = JobState::Downloading;
    active.downloaded_bytes = 4 * 1024 * 1024;
    active.speed = 512 * 1024;
    let queued = datanodes_bulk_job("job_datanodes_2", archive);
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![active, queued]);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 4;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Balanced;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
        runtime.settings.bulk.hoster_fairness_mode = BulkHosterFairnessMode::Adaptive;
        runtime.active_workers.insert("job_datanodes_1".into());
        seed_accelerated_datanodes_health(
            &mut runtime,
            "job_datanodes_1",
            512 * 1024,
            Duration::from_secs(7),
            3,
        );
        let downloaded = runtime.job("job_datanodes_1").unwrap().downloaded_bytes;
        runtime.update_bulk_hoster_worker_health("job_datanodes_1", downloaded, 0, Instant::now());
    }

    let (_, tasks) = state
        .claim_schedulable_jobs()
        .await
        .expect("accelerated DataNodes claim should respect runway after zero sample");

    assert!(tasks.is_empty());

    let _ = std::fs::remove_dir_all(download_dir);
}

#[tokio::test]
async fn datanodes_warmup_candidates_follow_acceleration_mode_and_horizon() {
    let download_dir = test_runtime_dir("bulk-datanodes-warmup-candidates");
    let archive = bulk_archive_info(&download_dir, "bulk_datanodes_warmup");
    let jobs = (1..=10)
        .map(|index| datanodes_bulk_job(&format!("job_datanodes_{index}"), archive.clone()))
        .collect::<Vec<_>>();
    let state = shared_state_with_jobs(download_dir.join("state.json"), jobs);
    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.max_concurrent_downloads = 12;
        runtime.settings.bulk.download_performance_mode = DownloadPerformanceMode::Fast;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Safe;
    }

    let candidates = state.datanodes_hoster_warmup_candidates().await;
    assert_eq!(candidates.len(), 8);
    assert_eq!(candidates[0].job_id, "job_datanodes_1");

    {
        let mut runtime = state.inner.write().await;
        runtime.settings.bulk.hoster_acceleration_mode = BulkHosterAccelerationMode::Off;
    }
    assert!(state.datanodes_hoster_warmup_candidates().await.is_empty());

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
