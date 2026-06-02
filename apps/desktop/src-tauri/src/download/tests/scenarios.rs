use super::*;
use crate::state::{
    BatchDownloadEntry, EnqueueOptions, ExternalReseedAttempt, SharedState, TorrentRuntimePhase,
    TorrentRuntimeSnapshot,
};
use crate::storage::{
    BulkArchiveOutputKind, BulkArchiveStatus, BulkFinalizeMode, DesktopSnapshot, DownloadJob,
    DownloadSource, FailureCategory, HandoffAuth, HandoffAuthHeader, IntegrityStatus, JobState,
    ProtectedDownloadAuthScope, Settings, TransferKind,
};
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[tokio::test]
async fn scenario_normal_http_completion_verifies_bytes_integrity_and_temp_cleanup() {
    let (_root, state) = scenario_state("normal-http").await;
    let body = b"normal scenario bytes";
    let expected_sha256 = sha256_hex(body);
    let (url, request_handle) = spawn_recording_response_server(vec![
        http_head_response(body.len(), "normal.txt"),
        http_range_rejected_response(),
        http_ok_response(body, "normal.txt"),
    ])
    .await;

    let enqueued = state
        .enqueue_download_with_options(
            url.clone(),
            EnqueueOptions {
                filename_hint: Some("normal.txt".into()),
                expected_sha256: Some(expected_sha256.clone()),
                ..Default::default()
            },
        )
        .await
        .expect("normal HTTP scenario should enqueue");
    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    let task = only_task(tasks);

    assert_eq!(task.id, enqueued.job_id);
    assert_eq!(task.transfer_kind, TransferKind::Http);
    assert_eq!(task.filename, "normal.txt");

    let app = NoopDownloadUi;
    let outcome = run_http_download_attempt(&app, &state, &task)
        .await
        .expect("normal HTTP fixture should complete through the engine");
    assert_eq!(outcome, DownloadOutcome::Completed);

    let requests = request_handle.await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("HEAD "));
    assert!(requests[1].starts_with("GET "));
    assert!(requests[1]
        .to_ascii_lowercase()
        .contains("range: bytes=0-0"));
    assert!(requests[2].starts_with("GET "));
    assert!(requests[2]
        .to_ascii_lowercase()
        .contains("accept-encoding: identity"));

    let snapshot = state.snapshot().await;
    let job = snapshot_job(&snapshot, &task.id);
    let final_path = PathBuf::from(&job.target_path);

    assert_eq!(job.state, JobState::Completed);
    assert_eq!(job.downloaded_bytes, body.len() as u64);
    assert_eq!(job.progress, 100.0);
    assert_eq!(job.filename, "normal.txt");
    assert_eq!(
        job.integrity_check.as_ref().map(|check| check.status),
        Some(IntegrityStatus::Verified)
    );
    assert_eq!(tokio::fs::read(&final_path).await.unwrap(), body);
    assert!(!task.temp_path.exists());
}

#[tokio::test]
async fn scenario_http_resume_refusal_preserves_partial_until_restart_completes_cleanly() {
    let (_root, state) = scenario_state("resume-refusal-restart").await;
    let partial = b"partial-bytes";
    let restarted_body = b"clean restart bytes";
    let (url, request_handle) = spawn_recording_response_server(vec![
        http_head_response(restarted_body.len(), "resume.bin"),
        http_ok_response(b"server ignored the range", "resume.bin"),
        http_head_response(restarted_body.len(), "resume.bin"),
        http_range_rejected_response(),
        http_ok_response(restarted_body, "resume.bin"),
    ])
    .await;

    let enqueued = state
        .enqueue_download_with_options(
            url,
            EnqueueOptions {
                filename_hint: Some("resume.bin".into()),
                ..Default::default()
            },
        )
        .await
        .expect("resume scenario should enqueue");
    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    let task = only_task(tasks);
    assert_eq!(task.id, enqueued.job_id);

    tokio::fs::write(&task.temp_path, partial).await.unwrap();
    state
        .mark_job_downloading(
            &task.id,
            partial.len() as u64,
            Some(restarted_body.len() as u64),
            ResumeSupport::Supported,
            None,
        )
        .await
        .unwrap();

    let app = NoopDownloadUi;
    let outcome = run_download(&app, &state, &task)
        .await
        .expect("resume refusal with partial data should pause recoverably");
    assert_eq!(outcome, DownloadOutcome::Paused);
    assert_eq!(tokio::fs::read(&task.temp_path).await.unwrap(), partial);

    let snapshot = state.snapshot().await;
    let job = snapshot_job(&snapshot, &task.id);
    assert_eq!(job.state, JobState::Paused);
    assert_eq!(job.downloaded_bytes, partial.len() as u64);
    assert_eq!(job.failure_category, Some(FailureCategory::Resume));
    let error = job.error.as_deref().unwrap_or_default();
    assert!(error.contains("partial data preserved"));
    assert!(error.contains("Restart"));

    let snapshot = state.restart_job(&task.id).await.unwrap();
    assert_eq!(snapshot_job(&snapshot, &task.id).state, JobState::Queued);
    assert!(!task.temp_path.exists());

    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    let restarted_task = only_task(tasks);
    assert_eq!(restarted_task.id, task.id);
    let outcome = match run_http_download_attempt(&app, &state, &restarted_task).await {
        Ok(outcome) => outcome,
        Err(error) => {
            let requests = request_handle.await.unwrap();
            panic!(
                "restart should discard partial data and complete from zero: {error:?}; requests: {requests:?}"
            );
        }
    };
    assert_eq!(outcome, DownloadOutcome::Completed);

    let requests = request_handle.await.unwrap();
    assert_eq!(requests.len(), 5);
    assert!(requests[0].starts_with("HEAD "));
    assert!(requests[1].starts_with("GET "));
    assert!(requests[1]
        .to_ascii_lowercase()
        .contains("range: bytes=13-"));
    assert!(requests[2].starts_with("HEAD "));
    assert!(requests[3].starts_with("GET "));
    assert!(requests[3]
        .to_ascii_lowercase()
        .contains("range: bytes=0-0"));
    assert!(requests[4].starts_with("GET "));
    assert!(!requests[4].to_ascii_lowercase().contains("range:"));

    let snapshot = state.snapshot().await;
    let job = snapshot_job(&snapshot, &task.id);
    let final_path = PathBuf::from(&job.target_path);
    assert_eq!(job.state, JobState::Completed);
    assert_eq!(tokio::fs::read(&final_path).await.unwrap(), restarted_body);
    assert!(!restarted_task.temp_path.exists());
}

#[tokio::test]
async fn scenario_segmented_http_resumes_after_transient_segment_interruption() {
    let (_root, state) = scenario_state("segmented-http").await;
    let enqueued = state
        .enqueue_download_with_options(
            "http://127.0.0.1/segmented.bin".into(),
            EnqueueOptions {
                filename_hint: Some("segmented.bin".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    let task = only_task(tasks);
    assert_eq!(task.id, enqueued.job_id);

    prepare_direct_segment_file(&task.temp_path, 12)
        .await
        .unwrap();
    let plan = three_segment_test_plan();
    let segment = plan.segments[0];
    let segment = SegmentProgress {
        index: 0,
        range: segment,
        downloaded_bytes: 0,
        completed: false,
    };
    let metadata = Arc::new(Mutex::new(new_segment_state_for_test(
        &plan,
        EntityValidators::default(),
    )));
    let initial_segment_state = metadata.lock().await.clone();
    persist_segment_state(&task.temp_path, &initial_segment_state)
        .await
        .unwrap();

    let first_response = concat!(
        "HTTP/1.1 206 Partial Content\r\n",
        "Content-Range: bytes 0-3/12\r\n",
        "Transfer-Encoding: chunked\r\n",
        "\r\n",
        "2\r\nab\r\nzz\r\n"
    );
    let (first_url, first_request) = spawn_one_response_server(first_response).await;
    let context = segment_context(&state, &task, first_url);

    let first_error = download_segment_worker(
        SegmentWorkerContext {
            metadata: metadata.clone(),
            ..context.clone()
        },
        segment.clone(),
    )
    .await
    .expect_err("short segment response should be treated as transient network failure");
    let request = first_request.await.unwrap();
    assert_eq!(first_error.category, FailureCategory::Network);
    assert!(request.to_ascii_lowercase().contains("range: bytes=0-3"));
    assert_eq!(
        &tokio::fs::read(&task.temp_path).await.unwrap()[0..2],
        b"ab"
    );

    let resumed_segment = metadata
        .lock()
        .await
        .segments
        .iter()
        .find(|progress| progress.index == 0)
        .cloned()
        .unwrap();
    let second_response = "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-3/12\r\nContent-Length: 2\r\n\r\ncd";
    let (second_url, second_request) = spawn_one_response_server(second_response).await;
    let outcome = download_segment_worker(
        SegmentWorkerContext {
            url: second_url,
            metadata: metadata.clone(),
            ..context
        },
        resumed_segment,
    )
    .await
    .unwrap();
    let request = second_request.await.unwrap();

    assert_eq!(outcome, DownloadOutcome::Completed);
    assert!(request.to_ascii_lowercase().contains("range: bytes=2-3"));
    assert_eq!(
        &tokio::fs::read(&task.temp_path).await.unwrap()[0..4],
        b"abcd"
    );
    assert!(metadata
        .lock()
        .await
        .segments
        .iter()
        .find(|progress| progress.index == 0)
        .is_some_and(|progress| progress.completed && progress.downloaded_bytes == 4));
}

#[tokio::test]
async fn scenario_dynamic_segment_worker_reconnects_without_aborting_job() {
    let (_root, state) = scenario_state("segmented-reconnect-worker").await;
    let enqueued = state
        .enqueue_download_with_options(
            "http://127.0.0.1/reconnect.bin".into(),
            EnqueueOptions {
                filename_hint: Some("reconnect.bin".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    let task = only_task(tasks);
    assert_eq!(task.id, enqueued.job_id);

    prepare_direct_segment_file(&task.temp_path, 4)
        .await
        .unwrap();
    let plan = RangePlan {
        total_bytes: 4,
        segments: vec![ByteRange { start: 0, end: 3 }],
    };
    let metadata = Arc::new(Mutex::new(new_segment_state_for_test(
        &plan,
        EntityValidators::default(),
    )));
    persist_segment_state(&task.temp_path, &metadata.lock().await.clone())
        .await
        .unwrap();

    let interrupted_first_segment = concat!(
        "HTTP/1.1 206 Partial Content\r\n",
        "Content-Range: bytes 0-3/4\r\n",
        "Transfer-Encoding: chunked\r\n",
        "\r\n",
        "2\r\nab\r\nzz\r\n"
    )
    .to_string();
    let (url, request_handle) = spawn_recording_response_server(vec![
        interrupted_first_segment,
        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-3/4\r\nContent-Length: 2\r\nConnection: close\r\n\r\ncd".into(),
    ])
    .await;
    let active_segments = Arc::new(Mutex::new(HashSet::new()));
    let mut context = segment_context(&state, &task, url);
    context.metadata = metadata.clone();
    context.total_bytes = 4;
    context.progress = Arc::new(SegmentedProgressCounters::new(vec![0]));

    let result = download_dynamic_segment_worker(context, active_segments, 1, 1).await;
    let requests = request_handle.await.unwrap();
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => panic!(
            "transient segment disconnect should be retried locally: {error:?}; requests: {requests:?}"
        ),
    };
    assert_eq!(outcome, DownloadOutcome::Completed);
    assert_eq!(requests.len(), 2);
    assert!(requests[0]
        .to_ascii_lowercase()
        .contains("range: bytes=0-3"));
    assert!(requests[1]
        .to_ascii_lowercase()
        .contains("range: bytes=2-3"));
    assert_eq!(tokio::fs::read(&task.temp_path).await.unwrap(), b"abcd");
    assert!(metadata
        .lock()
        .await
        .segments
        .iter()
        .all(|segment| segment.completed));
}

#[tokio::test]
async fn scenario_protected_browser_handoff_uses_auth_without_persisting_secret() {
    let (root, state) = scenario_state("protected-handoff").await;
    let mut settings = state.settings().await;
    settings.extension_integration.authenticated_handoff_enabled = true;
    settings.extension_integration.protected_download_auth_scope =
        ProtectedDownloadAuthScope::LegacyGlobal;
    state.save_settings(settings).await.unwrap();

    let auth = HandoffAuth {
        headers: vec![HandoffAuthHeader {
            name: "Cookie".into(),
            value: "session=abc".into(),
        }],
    };
    let source = DownloadSource {
        entry_point: "browser_download".into(),
        browser: "firefox".into(),
        extension_version: "scenario-test".into(),
        page_url: Some("https://example.test/protected".into()),
        page_title: Some("Protected fixture".into()),
        referrer: None,
        incognito: Some(false),
    };
    let body = b"protected bytes";
    let (url, request_handle) = spawn_cookie_download_server(body, "protected.bin").await;

    let enqueued = state
        .enqueue_download_with_options(
            url.clone(),
            EnqueueOptions {
                source: Some(source),
                filename_hint: Some("protected.bin".into()),
                handoff_auth: Some(auth),
                ..Default::default()
            },
        )
        .await
        .expect("protected browser handoff should enqueue with allowed auth");

    assert!(!persisted_state_text(&root).contains("session=abc"));

    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    let task = only_task(tasks);
    assert_eq!(task.id, enqueued.job_id);
    assert!(task.handoff_auth.is_some());

    let app = NoopDownloadUi;
    let outcome = run_http_download_attempt(&app, &state, &task)
        .await
        .expect("protected handoff should complete through the HTTP engine");
    assert_eq!(outcome, DownloadOutcome::Completed);

    let requests = request_handle.await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("HEAD "));
    assert!(requests[1].starts_with("GET "));
    assert!(requests[1]
        .to_ascii_lowercase()
        .contains("range: bytes=0-0"));
    assert!(requests[2].starts_with("GET "));
    assert!(requests
        .iter()
        .all(|request| request.to_ascii_lowercase().contains("cookie: session=abc")));
    let snapshot = state.snapshot().await;
    let final_path = PathBuf::from(&snapshot_job(&snapshot, &task.id).target_path);
    assert_eq!(tokio::fs::read(final_path).await.unwrap(), body);
    assert!(!persisted_state_text(&root).contains("session=abc"));

    state.clear_handoff_auth(&task.id).await;
    assert!(!persisted_state_text(&root).contains("session=abc"));
}

#[tokio::test]
async fn scenario_protected_browser_handoff_follows_signed_cdn_redirect_without_leaking_auth() {
    let (root, state) = scenario_state("protected-handoff-redirect").await;
    let mut settings = state.settings().await;
    settings.extension_integration.authenticated_handoff_enabled = true;
    settings.extension_integration.protected_download_auth_scope =
        ProtectedDownloadAuthScope::LegacyGlobal;
    state.save_settings(settings).await.unwrap();

    let auth = HandoffAuth {
        headers: vec![HandoffAuthHeader {
            name: "Cookie".into(),
            value: "session=abc".into(),
        }],
    };
    let source = DownloadSource {
        entry_point: "browser_download".into(),
        browser: "firefox".into(),
        extension_version: "scenario-test".into(),
        page_url: Some("https://example.test/protected".into()),
        page_title: Some("Protected fixture".into()),
        referrer: None,
        incognito: Some(false),
    };
    let body = b"redirected protected bytes";
    let (url, request_handle) =
        spawn_cookie_redirect_download_server(body, "protected-redirect.bin").await;

    let enqueued = state
        .enqueue_download_with_options(
            url.clone(),
            EnqueueOptions {
                source: Some(source),
                filename_hint: Some("protected-redirect.bin".into()),
                handoff_auth: Some(auth),
                ..Default::default()
            },
        )
        .await
        .expect("protected redirect handoff should enqueue with allowed auth");

    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    let task = only_task(tasks);
    assert_eq!(task.id, enqueued.job_id);
    assert!(task.handoff_auth.is_some());

    let app = NoopDownloadUi;
    let outcome = run_http_download_attempt(&app, &state, &task)
        .await
        .expect("protected redirect handoff should complete through the HTTP engine");
    assert_eq!(outcome, DownloadOutcome::Completed);

    let requests = request_handle.await.unwrap();
    assert_eq!(requests.len(), 6);
    for (index, request) in requests.iter().enumerate() {
        let request_lower = request.to_ascii_lowercase();
        if index % 2 == 0 {
            assert!(request_lower.contains("cookie: session=abc"));
        } else {
            assert!(
                !request_lower.contains("cookie:"),
                "browser credentials must not be forwarded to the redirected CDN origin",
            );
        }
    }

    let snapshot = state.snapshot().await;
    let final_path = PathBuf::from(&snapshot_job(&snapshot, &task.id).target_path);
    assert_eq!(tokio::fs::read(final_path).await.unwrap(), body);
    assert!(!persisted_state_text(&root).contains("session=abc"));
}

#[tokio::test]
async fn scenario_bulk_download_retry_finalize_and_cleanup_flow() {
    let (_root, state) = scenario_state("bulk-download").await;
    let entries = vec![
        BatchDownloadEntry {
            url: "http://127.0.0.1/files/part-a.bin".into(),
            filename_hint: Some("part-a.bin".into()),
            resolved_from_url: None,
            hoster_preflight: None,
        },
        BatchDownloadEntry {
            url: "http://127.0.0.1/files/part-b.bin".into(),
            filename_hint: Some("part-b.bin".into()),
            resolved_from_url: None,
            hoster_preflight: None,
        },
        BatchDownloadEntry {
            url: "http://127.0.0.1/files/excluded.bin".into(),
            filename_hint: Some("excluded.bin".into()),
            resolved_from_url: None,
            hoster_preflight: None,
        },
    ];
    let enqueued = state
        .enqueue_download_entries_with_bulk_options(
            entries,
            None,
            Some("Scenario Pack".into()),
            true,
            crate::storage::BulkArchiveOutputKind::Folder,
        )
        .await
        .unwrap();
    assert_eq!(enqueued.len(), 3);

    let (review_snapshot, review_tasks) = state.claim_schedulable_jobs().await.unwrap();
    assert!(
        review_tasks.is_empty(),
        "review-first bulk downloads should not start until selected rows are resumed"
    );
    let reviewed_jobs = review_snapshot
        .jobs
        .iter()
        .filter(|job| job.bulk_archive.is_some())
        .collect::<Vec<_>>();
    assert_eq!(reviewed_jobs.len(), 3);
    assert!(reviewed_jobs
        .iter()
        .all(|job| job.state == JobState::Paused));

    let excluded = reviewed_jobs
        .iter()
        .find(|job| job.filename == "excluded.bin")
        .expect("excluded review row should exist");
    let excluded_target = PathBuf::from(&excluded.target_path);
    if let Some(parent) = excluded_target.parent() {
        tokio::fs::create_dir_all(parent).await.unwrap();
    }
    tokio::fs::write(&excluded_target, b"user-owned file")
        .await
        .unwrap();
    state
        .delete_job(&excluded.id, false)
        .await
        .expect("unchecked bulk review rows should be removable without disk deletion");
    assert_eq!(
        tokio::fs::read(&excluded_target).await.unwrap(),
        b"user-owned file"
    );

    for filename in ["part-a.bin", "part-b.bin"] {
        let job = reviewed_jobs
            .iter()
            .find(|job| job.filename == filename)
            .expect("selected review row should exist");
        state.resume_job(&job.id).await.unwrap();
    }

    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    assert_eq!(tasks.len(), 2);
    let first = tasks
        .iter()
        .find(|task| task.filename == "part-a.bin")
        .unwrap()
        .clone();
    let second = tasks
        .iter()
        .find(|task| task.filename == "part-b.bin")
        .unwrap()
        .clone();
    let archive_id = first.bulk_archive_id.clone().unwrap();
    assert_eq!(first.bulk_archive_id, second.bulk_archive_id);
    assert!(first.is_bulk_member);
    assert!(second.is_bulk_member);

    state
        .fail_job(
            &first.id,
            "fixture server closed the bulk member stream",
            FailureCategory::Network,
        )
        .await
        .unwrap();
    let retry_candidates = state
        .bulk_member_retry_candidates(&archive_id)
        .await
        .unwrap();
    assert_eq!(retry_candidates.len(), 1);
    assert_eq!(retry_candidates[0].id, first.id);

    state
        .retry_bulk_member(&first.id, first.url.clone())
        .await
        .unwrap();
    let (_snapshot, retry_tasks) = state.claim_schedulable_jobs().await.unwrap();
    let first_retry = retry_tasks
        .into_iter()
        .find(|task| task.id == first.id)
        .expect("failed bulk member should be claimable after retry");

    complete_task_with_body(&state, &first_retry, b"part-a").await;
    assert!(state
        .bulk_archive_ready_for_job(&first_retry.id)
        .await
        .unwrap()
        .is_none());
    complete_task_with_body(&state, &second, b"part-b").await;

    let ready = state
        .bulk_archive_ready_for_job(&second.id)
        .await
        .unwrap()
        .expect("bulk archive should be ready after all members complete");
    assert_eq!(ready.archive_id, archive_id);
    assert_eq!(ready.entries.len(), 2);

    tokio::fs::create_dir_all(&ready.output_path).await.unwrap();
    for entry in &ready.entries {
        tokio::fs::copy(
            &entry.source_path,
            ready.output_path.join(&entry.archive_name),
        )
        .await
        .unwrap();
    }
    let snapshot = state
        .mark_bulk_archive_status(
            &ready.archive_id,
            BulkArchiveStatus::Completed,
            Some(false),
            Some(ready.output_path.display().to_string()),
            None,
            None,
            Some(BulkFinalizeMode::Move),
            Some(12),
            Some(12),
        )
        .await
        .unwrap();

    let members = snapshot
        .jobs
        .iter()
        .filter(|job| {
            job.bulk_archive
                .as_ref()
                .is_some_and(|archive| archive.id == archive_id)
        })
        .collect::<Vec<_>>();
    assert_eq!(members.len(), 2);
    assert!(members.iter().all(|job| job.state == JobState::Completed));
    assert!(members.iter().all(|job| {
        job.bulk_archive
            .as_ref()
            .is_some_and(|archive| archive.archive_status == BulkArchiveStatus::Completed)
    }));
    assert_eq!(
        tokio::fs::read(ready.output_path.join("part-a.bin"))
            .await
            .unwrap(),
        b"part-a"
    );
    assert_eq!(
        tokio::fs::read(ready.output_path.join("part-b.bin"))
            .await
            .unwrap(),
        b"part-b"
    );
}

#[tokio::test]
#[ignore = "requires SDM_BULK_BENCH_URLS with legal/user-authorized URLs"]
async fn live_bulk_download_rounds_from_env_cleanup_each_round() {
    let value = env::var("SDM_BULK_BENCH_URLS")
        .expect("set SDM_BULK_BENCH_URLS to legal/user-authorized labeled URL rounds");
    let rounds = crate::bulk_bench::parse_bulk_benchmark_rounds(&value)
        .expect("bulk benchmark rounds should parse");
    let duration = env::var("SDM_BULK_BENCH_DURATION_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(120));

    for round in rounds {
        let root = test_download_runtime_dir(&format!(
            "live-bulk-{}",
            sanitize_live_bulk_label(&round.label)
        ));
        let result = run_live_bulk_round(&round, &root, duration).await;
        let cleanup_result = tokio::fs::remove_dir_all(&root).await;
        assert!(
            cleanup_result.is_ok() || !root.exists(),
            "live bulk round cleanup should remove {}",
            root.display()
        );
        result.expect("live bulk round should run");
    }
}

async fn run_live_bulk_round(
    round: &crate::bulk_bench::BulkBenchmarkRound,
    root: &Path,
    duration: Duration,
) -> Result<(), String> {
    let state = SharedState::for_tests(root.join("state.json"), Vec::new());
    state
        .save_settings(Settings {
            download_directory: root.join("downloads").display().to_string(),
            ..Settings::default()
        })
        .await?;

    let entries = round
        .urls
        .iter()
        .map(|url| {
            let source_url = url.trim().to_string();
            let is_hoster = crate::hosters::is_supported_hoster_url(&source_url);
            BatchDownloadEntry {
                url: source_url.clone(),
                filename_hint: crate::hosters::source_filename_hint_for_url(&source_url),
                resolved_from_url: is_hoster.then_some(source_url),
                hoster_preflight: None,
            }
        })
        .collect::<Vec<_>>();
    let results = state
        .enqueue_download_entries_with_bulk_options(
            entries,
            None,
            Some(format!("live-{}", round.label)),
            false,
            BulkArchiveOutputKind::Folder,
        )
        .await
        .map_err(|error| error.message)?;
    let job_ids = results
        .iter()
        .map(|result| result.job_id.clone())
        .collect::<Vec<_>>();

    println!(
        "Starting live bulk round '{}' with {} jobs for {} seconds",
        round.label,
        job_ids.len(),
        duration.as_secs()
    );
    schedule_downloads(NoopDownloadUi, state.clone());
    tokio::time::sleep(duration).await;

    let before_cancel = state.snapshot().await;
    print_live_bulk_round_summary(&round.label, "before-cancel", &before_cancel, &job_ids);
    let _ = state.cancel_jobs(&job_ids).await;
    wait_for_live_bulk_round_to_stop(&state, &job_ids).await;
    let after_cancel = state.snapshot().await;
    print_live_bulk_round_summary(&round.label, "after-cancel", &after_cancel, &job_ids);

    Ok(())
}

async fn wait_for_live_bulk_round_to_stop(state: &SharedState, job_ids: &[String]) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let snapshot = state.snapshot().await;
        let any_active = snapshot.jobs.iter().any(|job| {
            job_ids.iter().any(|id| id == &job.id)
                && matches!(job.state, JobState::Starting | JobState::Downloading)
        });
        if !any_active {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn print_live_bulk_round_summary(
    label: &str,
    phase: &str,
    snapshot: &DesktopSnapshot,
    job_ids: &[String],
) {
    let jobs = snapshot
        .jobs
        .iter()
        .filter(|job| job_ids.iter().any(|id| id == &job.id))
        .collect::<Vec<_>>();
    let downloaded_bytes = jobs.iter().fold(0_u64, |total, job| {
        total.saturating_add(job.downloaded_bytes)
    });
    let speed = jobs
        .iter()
        .fold(0_u64, |total, job| total.saturating_add(job.speed));
    let states = jobs
        .iter()
        .map(|job| format!("{}:{:?}:{}B/s", job.filename, job.state, job.speed))
        .collect::<Vec<_>>()
        .join(", ");

    println!(
        "Live bulk round '{label}' {phase}: jobs={}, downloaded={} bytes, speed={} B/s [{}]",
        jobs.len(),
        downloaded_bytes,
        speed,
        states
    );
}

fn sanitize_live_bulk_label(label: &str) -> String {
    let sanitized = label
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    sanitized.trim_matches('-').to_string()
}

#[tokio::test]
async fn scenario_torrent_lifecycle_routes_pauses_reseeds_and_deletes_without_live_swarm() {
    let (_root, state) = scenario_state("torrent-lifecycle").await;
    let magnet = "magnet:?xt=urn:btih:420f3778a160fbe6eb0a67c8470256be13b0ecc8&dn=Scenario+Torrent";
    let enqueued = state
        .enqueue_download_with_options(
            magnet.into(),
            EnqueueOptions {
                transfer_kind: Some(TransferKind::Torrent),
                ..Default::default()
            },
        )
        .await
        .expect("magnet should enqueue as a torrent");
    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    let torrent_task = only_task(tasks);
    assert_eq!(torrent_task.id, enqueued.job_id);
    assert_eq!(torrent_task.transfer_kind, TransferKind::Torrent);
    assert!(torrent_task.torrent.is_some());

    let progress = torrent_update(0, 512, 1024, false);
    let snapshot = state
        .update_torrent_progress(&torrent_task.id, progress, false)
        .await
        .unwrap();
    let job = snapshot_job(&snapshot, &torrent_task.id);
    assert_eq!(job.state, JobState::Downloading);
    assert_eq!(job.downloaded_bytes, 512);
    assert_eq!(job.total_bytes, 1024);

    let snapshot = state.pause_job(&torrent_task.id).await.unwrap();
    assert_eq!(
        snapshot_job(&snapshot, &torrent_task.id).state,
        JobState::Paused
    );
    let snapshot = state.resume_job(&torrent_task.id).await.unwrap();
    assert_eq!(
        snapshot_job(&snapshot, &torrent_task.id).state,
        JobState::Queued
    );

    let snapshot = state
        .update_torrent_progress(
            &torrent_task.id,
            torrent_update(256, 1024, 1024, true),
            false,
        )
        .await
        .unwrap();
    let job = snapshot_job(&snapshot, &torrent_task.id);
    assert_eq!(job.state, JobState::Seeding);
    assert_eq!(job.progress, 100.0);
    assert!(job
        .torrent
        .as_ref()
        .and_then(|torrent| torrent.seeding_started_at)
        .is_some());

    let normal = state
        .enqueue_download_with_options(
            "http://127.0.0.1/queued-after-seeding.bin".into(),
            EnqueueOptions {
                filename_hint: Some("queued-after-seeding.bin".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    assert!(
        tasks.iter().any(|task| task.id == normal.job_id),
        "active seeding torrent should not consume normal download scheduler slots"
    );

    state.complete_torrent_job(&torrent_task.id).await.unwrap();
    let snapshot = state
        .update_torrent_progress(
            &torrent_task.id,
            torrent_update(300, 1024, 1024, true),
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        snapshot_job(&snapshot, &torrent_task.id).state,
        JobState::Seeding
    );

    let preparation = state
        .prepare_job_for_external_use_with_wait(
            &torrent_task.id,
            Duration::from_millis(1),
            Duration::from_millis(1),
        )
        .await
        .unwrap();
    assert!(preparation.paused_torrent);
    assert_eq!(
        snapshot_job(preparation.snapshot.as_ref().unwrap(), &torrent_task.id).state,
        JobState::Paused
    );

    state.begin_external_reseed(&torrent_task.id).await;
    let reseed = state
        .queue_external_reseed_attempt(&torrent_task.id)
        .await
        .unwrap();
    assert!(matches!(reseed, ExternalReseedAttempt::Queued(_)));

    let snapshot = state.cancel_job(&torrent_task.id).await.unwrap();
    assert_eq!(
        snapshot_job(&snapshot, &torrent_task.id).state,
        JobState::Canceled
    );

    let target_path = PathBuf::from(&snapshot_job(&snapshot, &torrent_task.id).target_path);
    tokio::fs::create_dir_all(target_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&target_path, b"torrent payload")
        .await
        .unwrap();
    let snapshot = state.delete_job(&torrent_task.id, true).await.unwrap();
    assert!(!snapshot.jobs.iter().any(|job| job.id == torrent_task.id));
    assert!(!target_path.exists());
}

#[tokio::test]
async fn scenario_failure_paths_cover_server_range_integrity_duplicate_missing_and_restart() {
    let (_root, state) = scenario_state("failure-paths").await;

    let server_error = error_for_http_status(StatusCode::SERVICE_UNAVAILABLE, false);
    assert_eq!(server_error.category, FailureCategory::Server);
    assert!(server_error.retryable);

    let range_rejection = download_error(
        FailureCategory::Resume,
        "The server did not honor a segmented range request.".into(),
        false,
    );
    assert!(segmented_error_allows_single_stream_fallback(
        &range_rejection
    ));

    let expected_sha256 = sha256_hex(b"expected");
    let enqueued = state
        .enqueue_download_with_options(
            "http://127.0.0.1/bad-checksum.bin".into(),
            EnqueueOptions {
                filename_hint: Some("bad-checksum.bin".into()),
                expected_sha256: Some(expected_sha256),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let duplicate = state
        .enqueue_download_with_options(
            "http://127.0.0.1/bad-checksum.bin".into(),
            EnqueueOptions {
                filename_hint: Some("bad-checksum.bin".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(duplicate.job_id, enqueued.job_id);

    let (_snapshot, tasks) = state.claim_schedulable_jobs().await.unwrap();
    let task = only_task(tasks);
    tokio::fs::write(&task.target_path, b"actual")
        .await
        .unwrap();
    let actual_sha256 = compute_sha256(&task.target_path).await.unwrap();
    let snapshot = state
        .complete_job_with_integrity(&task.id, 6, &task.target_path, Some(actual_sha256))
        .await
        .unwrap();
    let job = snapshot_job(&snapshot, &task.id);
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.failure_category, Some(FailureCategory::Integrity));
    assert_eq!(
        job.integrity_check.as_ref().map(|check| check.status),
        Some(IntegrityStatus::Failed)
    );

    let snapshot = state.retry_job(&task.id).await.unwrap();
    let job = snapshot_job(&snapshot, &task.id);
    assert_eq!(job.state, JobState::Queued);
    assert_eq!(
        job.integrity_check.as_ref().map(|check| check.status),
        Some(IntegrityStatus::Pending)
    );

    tokio::fs::write(&task.temp_path, b"partial").await.unwrap();
    let snapshot = state.restart_job(&task.id).await.unwrap();
    assert_eq!(snapshot_job(&snapshot, &task.id).state, JobState::Queued);
    assert!(!task.temp_path.exists());

    let _ = tokio::fs::remove_file(&task.target_path).await;
    let snapshot = state
        .complete_job(&task.id, 0, &task.target_path)
        .await
        .unwrap();
    assert_eq!(snapshot_job(&snapshot, &task.id).state, JobState::Completed);
    let missing = state.resolve_revealable_path(&task.id).await.unwrap_err();
    assert!(missing.message.contains("Downloaded file is missing"));
}

async fn scenario_state(name: &str) -> (PathBuf, SharedState) {
    let root = scenario_root(name);
    let state = SharedState::for_tests(root.join("state.json"), Vec::new());
    let download_directory = root.join("downloads").display().to_string();
    let settings = Settings {
        download_directory: download_directory.clone(),
        max_concurrent_downloads: 3,
        torrent: crate::storage::TorrentSettings {
            download_directory: root.join("torrents").display().to_string(),
            ..Default::default()
        },
        bulk: crate::storage::BulkDownloadSettings {
            output_directory: root.join("bulk").display().to_string(),
            max_concurrent_downloads: 3,
            ..crate::storage::BulkDownloadSettings::for_download_directory(&download_directory)
        },
        notifications_enabled: false,
        notification_sounds_enabled: false,
        ..Default::default()
    };
    state.save_settings(settings).await.unwrap();
    (root, state)
}

async fn spawn_recording_response_server(
    responses: Vec<String>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(responses.len());
        for response in responses {
            let Ok(Ok((mut socket, _))) =
                tokio::time::timeout(Duration::from_secs(30), listener.accept()).await
            else {
                break;
            };
            let mut buffer = vec![0_u8; 8192];
            let read = socket.read(&mut buffer).await.unwrap();
            requests.push(String::from_utf8_lossy(&buffer[..read]).to_string());
            socket.write_all(response.as_bytes()).await.unwrap();
        }
        requests
    });

    (format!("http://{address}/download.bin"), handle)
}

async fn spawn_cookie_download_server(
    body: &'static [u8],
    filename: &'static str,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(3);
        for _ in 0..3 {
            let Ok(Ok((mut socket, _))) =
                tokio::time::timeout(Duration::from_secs(30), listener.accept()).await
            else {
                break;
            };
            let mut buffer = vec![0_u8; 8192];
            let read = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let request_lower = request.to_ascii_lowercase();
            let response = if !request_lower.contains("cookie: session=abc") {
                "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string()
            } else if request.starts_with("HEAD ") {
                http_head_response(body.len(), filename)
            } else if request_lower.contains("range: bytes=0-0") {
                http_range_rejected_response()
            } else {
                http_ok_response(body, filename)
            };
            requests.push(request);
            socket.write_all(response.as_bytes()).await.unwrap();
        }
        requests
    });

    (format!("http://{address}/download.bin"), handle)
}

async fn spawn_cookie_redirect_download_server(
    body: &'static [u8],
    filename: &'static str,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let redirect_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect_address = redirect_listener.local_addr().unwrap();
    let target_address = target_listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(6);
        for _ in 0..3 {
            let Ok(Ok((mut redirect_socket, _))) =
                tokio::time::timeout(Duration::from_secs(30), redirect_listener.accept()).await
            else {
                break;
            };
            let mut redirect_buffer = vec![0_u8; 8192];
            let redirect_read = redirect_socket.read(&mut redirect_buffer).await.unwrap();
            let redirect_request =
                String::from_utf8_lossy(&redirect_buffer[..redirect_read]).to_string();
            let redirect_response = if redirect_request
                .to_ascii_lowercase()
                .contains("cookie: session=abc")
            {
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://{target_address}/cdn.bin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
            } else {
                "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string()
            };
            requests.push(redirect_request);
            redirect_socket
                .write_all(redirect_response.as_bytes())
                .await
                .unwrap();

            let Ok(Ok((mut target_socket, _))) =
                tokio::time::timeout(Duration::from_secs(30), target_listener.accept()).await
            else {
                break;
            };
            let mut target_buffer = vec![0_u8; 8192];
            let target_read = target_socket.read(&mut target_buffer).await.unwrap();
            let target_request = String::from_utf8_lossy(&target_buffer[..target_read]).to_string();
            let target_request_lower = target_request.to_ascii_lowercase();
            let target_response = if target_request.starts_with("HEAD ") {
                http_head_response(body.len(), filename)
            } else if target_request_lower.contains("range: bytes=0-0") {
                http_range_rejected_response()
            } else {
                http_ok_response(body, filename)
            };
            requests.push(target_request);
            target_socket
                .write_all(target_response.as_bytes())
                .await
                .unwrap();
        }
        requests
    });

    (format!("http://{redirect_address}/download.bin"), handle)
}

fn http_head_response(content_length: usize, filename: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {content_length}\r\nAccept-Ranges: bytes\r\nContent-Disposition: attachment; filename=\"{filename}\"\r\nConnection: close\r\n\r\n"
    )
}

fn http_ok_response(body: &[u8], filename: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nContent-Disposition: attachment; filename=\"{filename}\"\r\nConnection: close\r\n\r\n{}",
        body.len(),
        String::from_utf8_lossy(body)
    )
}

fn http_range_rejected_response() -> String {
    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
}

fn scenario_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("scenario-{name}-{}-{nonce}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn only_task(tasks: Vec<crate::state::DownloadTask>) -> crate::state::DownloadTask {
    assert_eq!(tasks.len(), 1);
    tasks.into_iter().next().unwrap()
}

fn snapshot_job<'a>(snapshot: &'a DesktopSnapshot, id: &str) -> &'a DownloadJob {
    snapshot
        .jobs
        .iter()
        .find(|job| job.id == id)
        .expect("scenario job should exist in snapshot")
}

fn persisted_state_text(root: &Path) -> String {
    std::fs::read_to_string(root.join("state.json")).unwrap_or_default()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn segment_context(
    state: &SharedState,
    task: &crate::state::DownloadTask,
    url: String,
) -> SegmentWorkerContext {
    SegmentWorkerContext {
        state: state.clone(),
        client: download_client().unwrap(),
        job_id: task.id.clone(),
        url,
        segment_pressure_key: "test:scenario".into(),
        handoff_auth: None,
        temp_path: task.temp_path.clone(),
        total_bytes: 12,
        profile: performance_profile(),
        validators: EntityValidators::default(),
        speed_limit: None,
        progress: Arc::new(SegmentedProgressCounters::new(vec![0])),
        metadata: Arc::new(Mutex::new(new_segment_state_for_test(
            &three_segment_test_plan(),
            EntityValidators::default(),
        ))),
        metadata_persisted_at: Arc::new(Mutex::new(Instant::now())),
        stop: Arc::new(AtomicBool::new(false)),
        control_signal: WorkerControlSignal::default(),
        ramp_blocked: Arc::new(AtomicBool::new(false)),
        priority_throttle: Arc::new(Mutex::new(DynamicThrottleState::default())),
        speed_throttle: Arc::new(Mutex::new(DynamicThrottleState::default())),
        priority_throttle_enabled: task.is_bulk_member && task.resolved_from_url.is_some(),
        stall_timeout: None,
        reconnects: Arc::new(SegmentReconnectTracker::default()),
        target_workers: Arc::new(AtomicUsize::new(1)),
        active_workers: Arc::new(AtomicUsize::new(1)),
        tail_lease_probe_cap: Arc::new(AtomicU64::new(0)),
    }
}

async fn complete_task_with_body(
    state: &SharedState,
    task: &crate::state::DownloadTask,
    body: &[u8],
) {
    tokio::fs::write(&task.temp_path, body).await.unwrap();
    tokio::fs::rename(&task.temp_path, &task.target_path)
        .await
        .unwrap();
    state
        .complete_job(&task.id, body.len() as u64, &task.target_path)
        .await
        .unwrap();
}

fn torrent_update(
    uploaded_bytes: u64,
    downloaded_bytes: u64,
    total_bytes: u64,
    finished: bool,
) -> TorrentRuntimeSnapshot {
    TorrentRuntimeSnapshot {
        engine_id: 7,
        info_hash: "420f3778a160fbe6eb0a67c8470256be13b0ecc8".into(),
        name: Some("Scenario Torrent".into()),
        total_files: Some(2),
        peers: Some(2),
        seeds: Some(3),
        downloaded_bytes,
        total_bytes,
        uploaded_bytes,
        fetched_bytes: downloaded_bytes,
        download_speed: if finished { 0 } else { 128 * 1024 },
        upload_speed: if finished { 32 * 1024 } else { 0 },
        eta: (!finished).then_some(4),
        phase: TorrentRuntimePhase::Live,
        finished,
        error: None,
        diagnostics: None,
    }
}
