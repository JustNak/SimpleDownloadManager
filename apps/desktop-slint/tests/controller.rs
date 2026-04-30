use simple_download_manager_desktop_core::contracts::{ProgressBatchContext, ProgressBatchKind};
use simple_download_manager_desktop_core::storage::{
    ConnectionState, DesktopSnapshot, DownloadJob, DownloadPrompt, JobState, Settings, TransferKind,
};
use simple_download_manager_desktop_slint::controller::{
    batch_details_from_context, job_row_from_job, progress_details_from_job,
    prompt_details_from_prompt, status_text_from_snapshot,
};

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
