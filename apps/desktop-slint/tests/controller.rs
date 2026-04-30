use simple_download_manager_desktop_core::storage::{
    ConnectionState, DesktopSnapshot, DownloadJob, JobState, Settings, TransferKind,
};
use simple_download_manager_desktop_slint::controller::{
    job_row_from_job, status_text_from_snapshot,
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
