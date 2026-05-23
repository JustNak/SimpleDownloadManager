use super::*;

fn browser_source() -> DownloadSource {
    DownloadSource {
        entry_point: "browser_download".into(),
        browser: "chrome".into(),
        extension_version: "blob-test".into(),
        page_url: Some("https://web.telegram.org/k/".into()),
        page_title: Some("Telegram".into()),
        referrer: None,
        incognito: Some(false),
    }
}

#[tokio::test]
async fn browser_blob_stream_writes_chunks_and_completes_app_owned_job() {
    let download_dir = test_runtime_dir("browser-blob-stream-complete");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);
    let mut settings = state.settings().await;
    settings.download_directory = download_dir.display().to_string();
    state.save_settings(settings).await.unwrap();

    let begin = state
        .begin_browser_blob_download(
            "stream-1".into(),
            browser_source(),
            Some("clip.webm".into()),
            Some(4),
            Some("video/webm".into()),
        )
        .await
        .expect("blob stream should begin");

    let job = begin
        .snapshot
        .jobs
        .iter()
        .find(|job| job.id == begin.job_id)
        .expect("begin snapshot should include blob job");
    assert_eq!(job.transfer_kind, TransferKind::BrowserBlob);
    assert_eq!(job.state, JobState::Downloading);
    assert_eq!(job.resume_support, ResumeSupport::Unsupported);
    assert_eq!(job.filename, "clip.webm");

    state
        .append_browser_blob_download_chunk("stream-1", 0, b"he")
        .await
        .expect("first blob chunk should append");
    state
        .append_browser_blob_download_chunk("stream-1", 2, b"ya")
        .await
        .expect("second blob chunk should append");

    let completed = state
        .finish_browser_blob_download("stream-1")
        .await
        .expect("blob stream should finish");
    let completed_job = completed
        .jobs
        .iter()
        .find(|job| job.id == begin.job_id)
        .expect("completed snapshot should include blob job");
    assert_eq!(completed_job.state, JobState::Completed);
    assert_eq!(completed_job.downloaded_bytes, 4);
    assert_eq!(completed_job.total_bytes, 4);
    assert_eq!(
        tokio::fs::read(&completed_job.target_path).await.unwrap(),
        b"heya"
    );
    assert!(!PathBuf::from(&completed_job.temp_path).exists());

    let _ = tokio::fs::remove_dir_all(download_dir).await;
}

#[tokio::test]
async fn browser_blob_stream_rejects_out_of_order_chunks() {
    let download_dir = test_runtime_dir("browser-blob-stream-offset");
    let state = shared_state_with_jobs(download_dir.join("state.json"), vec![]);
    let mut settings = state.settings().await;
    settings.download_directory = download_dir.display().to_string();
    state.save_settings(settings).await.unwrap();

    state
        .begin_browser_blob_download(
            "stream-offset".into(),
            browser_source(),
            Some("clip.webm".into()),
            Some(4),
            Some("video/webm".into()),
        )
        .await
        .expect("blob stream should begin");

    let error = state
        .append_browser_blob_download_chunk("stream-offset", 2, b"ya")
        .await
        .expect_err("out-of-order chunks should reject");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("offset"));

    let _ = tokio::fs::remove_dir_all(download_dir).await;
}

#[test]
fn normalize_job_marks_interrupted_browser_blob_stream_failed() {
    let mut job = download_job(
        "job_blob",
        JobState::Downloading,
        ResumeSupport::Unsupported,
        2,
    );
    job.transfer_kind = TransferKind::BrowserBlob;
    job.url = "browser-blob://stream-1".into();

    let normalized = normalize_job(job, &Settings::default());

    assert_eq!(normalized.state, JobState::Failed);
    assert_eq!(normalized.failure_category, Some(FailureCategory::Network));
    assert!(normalized
        .error
        .unwrap()
        .contains("browser blob stream was interrupted"));
}
