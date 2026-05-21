use super::*;
use crate::storage::{
    BulkDownloadSettings, HostRegistrationStatus, HosterPreflightInfo, HosterPreflightStatus,
    StartupRecoveryStatus, TorrentPeerConnectionWatchdogMode, TorrentRuntimeDiagnostics,
};

#[path = "destructive_cleanup.rs"]
mod destructive_cleanup;
#[path = "diagnostics.rs"]
mod diagnostics;
#[path = "enqueue_settings.rs"]
mod enqueue_settings;
#[path = "local_recovery.rs"]
mod local_recovery;
#[path = "progress.rs"]
mod progress;
#[path = "scheduler.rs"]
mod scheduler;
#[path = "torrent.rs"]
mod torrent;

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
async fn reveal_interrupted_job_returns_parent_instead_of_partial_file() {
    let download_dir = test_runtime_dir("reveal-partial-existing");
    let target_path = download_dir.join("file.zip");
    let temp_path = download_dir.join("file.zip.part");
    std::fs::write(&temp_path, b"partial").unwrap();
    let mut job = download_job("job_22", JobState::Failed, ResumeSupport::Supported, 50);
    job.target_path = target_path.display().to_string();
    job.temp_path = temp_path.display().to_string();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![job]);

    let resolved = state.resolve_revealable_path("job_22").await.unwrap();

    assert_eq!(resolved, download_dir);

    let _ = std::fs::remove_dir_all(resolved);
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
        removal_state: None,
        created_at: 1,
        progress: 0.0,
        total_bytes: 100,
        downloaded_bytes,
        speed: 0,
        eta: 0,
        active_segments: None,
        planned_segments: None,
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
        pending_diagnostic_events: Vec::new(),
        startup_recovery: None,
        next_job_number: 99,
        job_indexes,
        active_workers: HashSet::new(),
        bulk_hoster_worker_health: HashMap::new(),
        bulk_hoster_fairness: HashMap::new(),
        datanodes_priority_defer_until: HashMap::new(),
        download_admission_defers: HashMap::new(),
        hoster_priority_cap_reports: HashMap::new(),
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
    job.url = format!("https://unverified-hoster.example/{id}");
    job.resolved_from_url = Some(job.url.clone());
    job.bulk_archive = Some(archive);
    job
}

fn fuckingfast_bulk_job(id: &str, archive: BulkArchiveInfo) -> DownloadJob {
    let mut job = download_job(id, JobState::Queued, ResumeSupport::Unknown, 0);
    job.url = format!("https://fuckingfast.co/{id}#Game.part.rar");
    job.resolved_from_url = Some(job.url.clone());
    job.bulk_archive = Some(archive);
    job
}

fn datanodes_bulk_job(id: &str, archive: BulkArchiveInfo) -> DownloadJob {
    let mut job = download_job(id, JobState::Queued, ResumeSupport::Unknown, 0);
    let file_code = id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>();
    job.url = format!("https://datanodes.to/{file_code}/Game.part.rar");
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

fn seed_priority_hoster_health(
    runtime: &mut RuntimeState,
    id: &str,
    speed: u64,
    transfer_age: Duration,
    healthy_samples: u8,
) {
    let now = Instant::now();
    let mut downloaded_bytes = runtime
        .job(id)
        .expect("hoster job should exist")
        .downloaded_bytes;
    let mut health = {
        let job = runtime.job(id).expect("hoster job should exist");
        BulkHosterWorkerHealth::from_job(job, now - transfer_age)
    };
    health.mark_transferring(downloaded_bytes, now - transfer_age);
    let samples = healthy_samples.max(1);
    for sample_index in 0..samples {
        downloaded_bytes = downloaded_bytes.saturating_add(speed.max(1));
        let sample_age = Duration::from_secs((samples - sample_index) as u64);
        health.update(downloaded_bytes, speed, now - sample_age.min(transfer_age));
    }
    runtime
        .bulk_hoster_worker_health
        .insert(id.to_string(), health);
    if let Some(job) = runtime.job_mut(id) {
        job.state = JobState::Downloading;
        job.speed = speed;
        job.downloaded_bytes = downloaded_bytes;
    }
}

fn seed_accelerated_hoster_health(
    runtime: &mut RuntimeState,
    id: &str,
    speed: u64,
    transfer_age: Duration,
    healthy_samples: u8,
) {
    let now = Instant::now();
    let profile = {
        let job = runtime.job(id).expect("hoster job should exist");
        bulk_hoster_worker_profile_for_job(&runtime.settings, job)
    };
    let mut downloaded_bytes = runtime
        .job(id)
        .expect("hoster job should exist")
        .downloaded_bytes;
    let mut health = {
        let job = runtime.job(id).expect("hoster job should exist");
        BulkHosterWorkerHealth::from_job_with_profile(job, profile, now - transfer_age)
    };
    health.mark_transferring(downloaded_bytes, now - transfer_age);
    let samples = healthy_samples.max(1);
    for sample_index in 0..samples {
        downloaded_bytes = downloaded_bytes.saturating_add(speed.max(1));
        let sample_age = Duration::from_secs((samples - sample_index) as u64);
        health.update(downloaded_bytes, speed, now - sample_age.min(transfer_age));
    }
    runtime
        .bulk_hoster_worker_health
        .insert(id.to_string(), health);
    if let Some(job) = runtime.job_mut(id) {
        job.state = JobState::Downloading;
        job.speed = speed;
        job.downloaded_bytes = downloaded_bytes;
    }
}

fn seed_accelerated_datanodes_health(
    runtime: &mut RuntimeState,
    id: &str,
    speed: u64,
    transfer_age: Duration,
    healthy_samples: u8,
) {
    let now = Instant::now();
    let profile = datanodes_accelerated_hoster_concurrency(
        &runtime.settings,
        runtime.job(id).expect("DataNodes job should exist"),
    )
    .map(|max_concurrency| BulkHosterWorkerProfile::Accelerated { max_concurrency })
    .expect("DataNodes job should be accelerated");
    let mut downloaded_bytes = runtime
        .job(id)
        .expect("DataNodes job should exist")
        .downloaded_bytes;
    let mut health = {
        let job = runtime.job(id).expect("DataNodes job should exist");
        BulkHosterWorkerHealth::from_job_with_profile(job, profile, now - transfer_age)
    };
    health.mark_transferring(downloaded_bytes, now - transfer_age);
    let samples = healthy_samples.max(1);
    for sample_index in 0..samples {
        downloaded_bytes = downloaded_bytes.saturating_add(speed.max(1));
        let sample_age = Duration::from_secs((samples - sample_index) as u64);
        health.update(downloaded_bytes, speed, now - sample_age.min(transfer_age));
    }
    runtime
        .bulk_hoster_worker_health
        .insert(id.to_string(), health);
    if let Some(job) = runtime.job_mut(id) {
        job.state = JobState::Downloading;
        job.speed = speed;
        job.downloaded_bytes = downloaded_bytes;
    }
}

fn seed_pressured_older_datanodes_health(runtime: &mut RuntimeState, id: &str) {
    let now = Instant::now();
    let profile = datanodes_accelerated_hoster_concurrency(
        &runtime.settings,
        runtime.job(id).expect("DataNodes job should exist"),
    )
    .map(|max_concurrency| BulkHosterWorkerProfile::Accelerated { max_concurrency })
    .expect("DataNodes job should be accelerated");
    let mut downloaded_bytes = runtime
        .job(id)
        .expect("DataNodes job should exist")
        .downloaded_bytes;
    let mut health = {
        let job = runtime.job(id).expect("DataNodes job should exist");
        BulkHosterWorkerHealth::from_job_with_profile(job, profile, now - Duration::from_secs(30))
    };
    health.mark_transferring(downloaded_bytes, now - Duration::from_secs(30));
    downloaded_bytes = downloaded_bytes.saturating_add(1024 * 1024);
    health.update(downloaded_bytes, 1024 * 1024, now - Duration::from_secs(20));
    downloaded_bytes = downloaded_bytes.saturating_add(96 * 1024);
    health.update(
        downloaded_bytes,
        48 * 1024,
        now - DATANODES_PRIORITY_PRESSURE_WINDOW - Duration::from_secs(1),
    );
    downloaded_bytes = downloaded_bytes.saturating_add(48 * 1024);
    health.update(downloaded_bytes, 48 * 1024, now);
    runtime
        .bulk_hoster_worker_health
        .insert(id.to_string(), health);
    if let Some(job) = runtime.job_mut(id) {
        job.state = JobState::Downloading;
        job.speed = 48 * 1024;
        job.downloaded_bytes = downloaded_bytes;
    }
}

fn shared_state_with_jobs(storage_path: PathBuf, jobs: Vec<DownloadJob>) -> SharedState {
    let diagnostic_event_store = Arc::new(DiagnosticEventStore::new(
        diagnostic_event_log_path_for(&storage_path),
    ));
    SharedState {
        inner: Arc::new(RwLock::new(runtime_state_with_jobs(jobs))),
        storage_path: Arc::new(storage_path),
        diagnostic_event_store,
        handoff_auth: Arc::new(RwLock::new(HashMap::new())),
        scheduler_wake: Arc::new(StdMutex::new(SchedulerWakeState::default())),
    }
}

fn diagnostic_test_event(message: &str, timestamp: u64) -> DiagnosticEvent {
    DiagnosticEvent {
        timestamp,
        level: DiagnosticLevel::Info,
        category: "test".into(),
        message: message.into(),
        job_id: None,
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
