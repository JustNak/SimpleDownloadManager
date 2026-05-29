use super::*;

fn browser_source() -> DownloadSource {
    DownloadSource {
        entry_point: "browser_download".into(),
        browser: "chrome".into(),
        extension_version: "adoption-test".into(),
        page_url: Some("https://web.telegram.org/k/".into()),
        page_title: Some("Telegram".into()),
        referrer: None,
        incognito: Some(false),
    }
}

#[tokio::test]
async fn adopt_browser_download_records_completed_file_in_place() {
    let download_dir = test_runtime_dir("browser-adopt-complete");
    let source_path = download_dir.join("browser").join("W3_AI_Data_Dilemma.zip");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, b"zip-bytes").unwrap();
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);

    let result = state
        .adopt_browser_download(
            "https://canvas.example/files/123/download".into(),
            browser_source(),
            source_path.display().to_string(),
            Some("ignored.txt".into()),
            Some(9),
            Some("application/zip".into()),
        )
        .await
        .expect("completed browser file should be adopted");

    let job = result
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == result.job_id)
        .expect("adopted job should exist");
    assert_eq!(job.state, JobState::Completed);
    assert_eq!(job.transfer_kind, TransferKind::BrowserAdopted);
    assert_eq!(job.filename, "W3_AI_Data_Dilemma.zip");
    assert_eq!(job.target_path, source_path.display().to_string());
    assert_eq!(job.downloaded_bytes, 9);
    assert_eq!(job.total_bytes, 9);
    assert_eq!(job.progress, 100.0);

    let duplicate = state
        .adopt_browser_download(
            "https://canvas.example/files/123/download".into(),
            browser_source(),
            source_path.display().to_string(),
            None,
            None,
            None,
        )
        .await
        .expect("duplicate adoption should return existing completed job");
    assert_eq!(duplicate.status, EnqueueStatus::DuplicateExistingJob);
    assert_eq!(duplicate.job_id, result.job_id);

    let _ = tokio::fs::remove_dir_all(download_dir).await;
}
