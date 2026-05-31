use super::*;

#[test]
fn http_status_errors_are_classified_by_recoverability() {
    let unavailable = error_for_http_status(StatusCode::SERVICE_UNAVAILABLE, false);
    assert_eq!(unavailable.category, FailureCategory::Server);
    assert!(unavailable.retryable);
    assert_eq!(
        unavailable.http_status,
        Some(StatusCode::SERVICE_UNAVAILABLE)
    );

    let not_found = error_for_http_status(StatusCode::NOT_FOUND, false);
    assert_eq!(not_found.category, FailureCategory::Http);
    assert!(!not_found.retryable);
    assert_eq!(not_found.http_status, Some(StatusCode::NOT_FOUND));
}

#[test]
fn http_response_errors_preserve_retry_after_for_segment_reconnects() {
    let mut headers = HeaderMap::new();
    headers.insert(RETRY_AFTER, HeaderValue::from_static("120"));

    let error = error_for_http_response(StatusCode::TOO_MANY_REQUESTS, &headers, false);

    assert_eq!(error.http_status, Some(StatusCode::TOO_MANY_REQUESTS));
    assert_eq!(error.retry_after, Some(MAX_RETRY_AFTER_DELAY));
    assert_eq!(
        segment_reconnect_delay_for_error(
            &error,
            1,
            "job_rate_limited",
            "https://cdn.example.com/file.bin"
        ),
        MAX_RETRY_AFTER_DELAY
    );
}

#[test]
fn rate_limited_segment_reconnects_use_stable_jitter_without_retry_after() {
    let error = error_for_http_status(StatusCode::TOO_MANY_REQUESTS, false);
    let first =
        segment_reconnect_delay_for_error(&error, 2, "job_a", "https://cdn.example.com/file.bin");
    let second =
        segment_reconnect_delay_for_error(&error, 2, "job_b", "https://cdn.example.com/file.bin");

    assert!(first >= REQUEST_RETRY_DELAYS[1]);
    assert!(first <= REQUEST_RETRY_DELAYS[1] + MAX_RETRY_JITTER);
    assert_ne!(
        first, second,
        "rate-limited segment reconnects should be de-synchronized"
    );
}

#[test]
fn hoster_refresh_retries_expired_links_range_failures_and_early_eof() {
    let forbidden = error_for_http_status(StatusCode::FORBIDDEN, false);
    assert!(hoster_refresh_error_allows_retry(&forbidden));

    let gone = error_for_http_status(StatusCode::GONE, false);
    assert!(hoster_refresh_error_allows_retry(&gone));

    let range_rejected = download_error(
        FailureCategory::Resume,
        "The remote server rejected the resume request.".into(),
        false,
    );
    assert!(hoster_refresh_error_allows_retry(&range_rejected));

    let early_eof = download_error(
        FailureCategory::Network,
        "Download ended early. Received 1024 of 4096 bytes.".into(),
        true,
    );
    assert!(hoster_refresh_error_allows_retry(&early_eof));

    let integrity = download_error(
        FailureCategory::Integrity,
        "Downloaded file checksum did not match.".into(),
        false,
    );
    assert!(!hoster_refresh_error_allows_retry(&integrity));
}

#[tokio::test]
async fn download_client_does_not_decode_mislabelled_file_bodies() {
    let response = "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\n\r\nbad";
    let (url, _request_handle) = spawn_one_response_server(response).await;
    let client = download_client().unwrap();

    let response = send_request(&client, &url, 0, None, None)
        .await
        .expect("mislabelled file response should still start");
    let bytes = response
        .bytes()
        .await
        .expect("download client should stream raw file bytes without decompression");

    assert_eq!(&bytes[..], b"bad");
}

#[test]
fn hoster_refresh_before_attempt_fails_closed_instead_of_using_source_url() {
    let source = include_str!("../http.rs");

    assert!(source.contains("Err(error) => return Err(error),"));
    assert!(!source.contains("Ok(None) | Err(_) => task.url.clone()"));
}

#[test]
fn hoster_refresh_preserves_resolution_retryability() {
    let terminal = crate::hosters::HosterResolutionError {
        code: "HOSTER_RESOLUTION_FAILED",
        message: "DataNodes captcha-protected downloads are not supported.".into(),
        retryable: false,
    };
    let terminal_error = hoster_resolution_download_error(terminal);
    assert_eq!(terminal_error.category, FailureCategory::Http);
    assert!(!terminal_error.retryable);

    let transient = crate::hosters::HosterResolutionError {
        code: "HOSTER_RESOLUTION_FAILED",
        message: "hoster resolver: HTTP 503 Service Unavailable.".into(),
        retryable: true,
    };
    let transient_error = hoster_resolution_download_error(transient);
    assert_eq!(transient_error.category, FailureCategory::Http);
    assert!(transient_error.retryable);
}

#[test]
fn retry_delay_caps_at_last_configured_delay() {
    assert_eq!(retry_delay_for_attempt(0), REQUEST_RETRY_DELAYS[0]);
    assert_eq!(
        retry_delay_for_attempt(99),
        *REQUEST_RETRY_DELAYS.last().unwrap()
    );
}

#[test]
fn resume_support_uses_partial_content_before_header_hints() {
    assert_eq!(
        derive_resume_support_from_parts(StatusCode::PARTIAL_CONTENT, 10, None),
        ResumeSupport::Supported
    );
    assert_eq!(
        derive_resume_support_from_parts(StatusCode::OK, 10, Some("bytes")),
        ResumeSupport::Unsupported
    );
    assert_eq!(
        derive_resume_support_from_parts(StatusCode::OK, 0, Some("bytes")),
        ResumeSupport::Supported
    );
    assert_eq!(
        derive_resume_support_from_parts(StatusCode::OK, 0, None),
        ResumeSupport::Unknown
    );
}

#[test]
fn preflight_metadata_uses_head_headers() {
    let metadata = derive_preflight_metadata_from_parts(
        Some(4_096),
        Some("bytes"),
        Some("attachment; filename=\"server-report.pdf\""),
        "https://example.com/download",
        EntityValidators::default(),
    );

    assert_eq!(metadata.total_bytes, Some(4_096));
    assert_eq!(metadata.resume_support, ResumeSupport::Supported);
    assert_eq!(metadata.filename.as_deref(), Some("server-report.pdf"));
}

#[test]
fn browser_handoff_html_response_without_attachment_is_unusable() {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );

    let error = unusable_download_response_error_for_parts(
        "https://cdn.example.com/download",
        &headers,
        true,
        false,
    )
    .expect("browser handoff HTML should not be treated as file content");

    assert_eq!(error.category, FailureCategory::Http);
    assert!(!error.retryable);
    assert!(error.message.contains("HTML"));
}

#[test]
fn browser_handoff_explicit_html_attachment_is_usable() {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"report.html\""),
    );

    assert!(unusable_download_response_error_for_parts(
        "https://cdn.example.com/download",
        &headers,
        true,
        false,
    )
    .is_none());
}

#[test]
fn gofile_direct_json_response_without_attachment_is_unusable() {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    let error = unusable_download_response_error_for_parts(
        "https://file-ap-sgp-3.gofile.io/download/web/token",
        &headers,
        false,
        false,
    )
    .expect("Gofile JSON API responses should not be saved as file content");

    assert_eq!(error.category, FailureCategory::Http);
    assert!(!error.retryable);
    assert!(error.message.contains("Gofile"));
}

#[test]
fn content_disposition_filename_avoids_windows_reserved_device_names() {
    assert_eq!(
        parse_content_disposition_filename("attachment; filename=\"CON\"").as_deref(),
        Some("CON_")
    );
    assert_eq!(
        parse_content_disposition_filename("attachment; filename=\"con.txt\"").as_deref(),
        Some("con.txt_")
    );
}

#[test]
fn content_disposition_plain_filename_decodes_percent_encoded_name() {
    assert_eq!(
            parse_content_disposition_filename(
                "attachment; filename=\"%5BNanakoRaws%5D%20Tensei%20Shitara%20Slime%20S4%20-%2002.mkv\""
            )
            .as_deref(),
            Some("[NanakoRaws] Tensei Shitara Slime S4 - 02.mkv")
        );
}

#[test]
fn url_filename_decodes_percent_encoded_path_segment() {
    let filename = derive_filename_from_url(
            "https://example.com/%5BNanakoRaws%5D%20Tensei%20Shitara%20Slime%20Datta%20Ken%20S4%20-%2002%20%28AT-X%20TV%201080p%20HEVC%20AAC%29.mkv",
        );

    assert_eq!(
        filename.as_deref(),
        Some("[NanakoRaws] Tensei Shitara Slime Datta Ken S4 - 02 (AT-X TV 1080p HEVC AAC).mkv")
    );
}

#[test]
fn speed_limit_throttle_calculates_remaining_delay() {
    assert_eq!(
        throttle_delay_for_limit(1024, 4096, Duration::from_secs(2)),
        Some(Duration::from_secs(2))
    );
    assert_eq!(
        throttle_delay_for_limit(1024, 4096, Duration::from_secs(4)),
        None
    );
    assert_eq!(
        throttle_delay_for_limit(0, 4096, Duration::from_secs(0)),
        None
    );
}

#[test]
fn unverified_hoster_bulk_tasks_disallow_segmented_downloads() {
    let task = http_segment_policy_task(true, Some("https://example.com/source"));

    assert!(!task_allows_segmented_download(&task));
}

#[test]
fn hoster_acceleration_off_disallows_verified_hoster_bulk_segmentation() {
    let mut task = http_segment_policy_task(
        true,
        Some("https://datanodes.to/abc123456789/fg-optional-bonus-content.bin"),
    );
    task.url = "https://s42.datanodes.to/d/abc123456789/fg-optional-bonus-content.bin".into();

    assert!(!task_allows_segmented_download_with_mode(
        &task,
        BulkHosterAccelerationMode::Off
    ));

    let mut fuckingfast_task = http_segment_policy_task(
        true,
        Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar"),
    );
    fuckingfast_task.url = "https://dl.fuckingfast.co/dl/token/Game.part01.rar".into();

    assert!(!task_allows_segmented_download_with_mode(
        &fuckingfast_task,
        BulkHosterAccelerationMode::Off
    ));
}

#[test]
fn hoster_acceleration_uses_general_segment_caps() {
    let policy = crate::hosters::HosterAccelerationPolicy {
        backoff_key: "hoster:datanodes:abc123456789".into(),
        balanced_initial_segments: 4,
        balanced_max_segments: 4,
        fast_initial_segments: 6,
        fast_max_segments: 10,
    };

    assert_eq!(hoster_initial_segment_cap(&policy), 4);
    assert_eq!(hoster_adaptive_segment_cap(&policy), 4);
}

#[test]
fn normal_download_segment_budget_limits_same_origin_connections() {
    let budget = normal_segment_budget()
        .expect("balanced normal downloads should use brokered segment budgets");

    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::Normal,
            "job_3",
            "https://cdn.example.com/third.bin",
            budget,
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::Normal,
                    "https://cdn.example.com/first.bin",
                    4,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::Normal,
                    "https://cdn.example.com/second.bin",
                    2,
                ),
            ],
        ),
        Some(2)
    );
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::Normal,
            "job_4",
            "https://cdn.example.com/fourth.bin",
            budget,
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::Normal,
                    "https://cdn.example.com/first.bin",
                    4,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::Normal,
                    "https://cdn.example.com/second.bin",
                    4,
                ),
            ],
        ),
        None
    );
}

#[test]
fn direct_bulk_and_non_bulk_hoster_tasks_still_allow_segmented_downloads() {
    let direct_bulk = http_segment_policy_task(true, None);
    let non_bulk_hoster = http_segment_policy_task(false, Some("https://fuckingfast.co/source"));

    assert!(task_allows_segmented_download(&direct_bulk));
    assert!(task_allows_segmented_download(&non_bulk_hoster));
}

#[test]
fn healthy_hoster_bulk_progress_releases_fairness_scheduler() {
    let hoster_bulk = http_segment_policy_task(true, Some("https://fuckingfast.co/source"));
    let direct_bulk = http_segment_policy_task(true, None);

    assert!(task_releases_bulk_hoster_fairness(&hoster_bulk, 64 * 1024));
    assert!(!task_releases_bulk_hoster_fairness(
        &hoster_bulk,
        64 * 1024 - 1
    ));
    assert!(!task_releases_bulk_hoster_fairness(&direct_bulk, 96 * 1024));
}

#[test]
fn protected_bulk_hoster_stall_timeout_uses_general_policy() {
    let hoster_bulk = http_segment_policy_task(true, Some("https://datanodes.to/source"));
    let direct_bulk = http_segment_policy_task(true, None);

    assert_eq!(
        protected_bulk_hoster_stall_timeout(&hoster_bulk, performance_profile(),),
        Some(Duration::from_secs(25))
    );
    assert_eq!(
        protected_bulk_hoster_stall_timeout(&direct_bulk, performance_profile(),),
        None
    );
}

#[test]
fn protected_bulk_hoster_stall_errors_are_retryable_network_failures() {
    let error = bulk_hoster_stall_error(Duration::from_secs(25));

    assert_eq!(error.category, FailureCategory::Network);
    assert!(error.retryable);
    assert!(error.message.contains("25 seconds"));
}

#[test]
fn http_attempt_defers_segment_budget_waits_without_deferring_priority_throttle() {
    let source = include_str!("../http.rs");
    let attempt = source
        .split("async fn run_http_download_attempt_for_url")
        .nth(1)
        .expect("HTTP download attempt function should exist");

    assert!(attempt.contains("hoster_priority_throttle_decision"));
    assert!(attempt.contains("throttle_download_with_dynamic_limit"));
    assert!(attempt.contains("priority_throttle_limited"));
    assert!(attempt.contains("speed_limit.is_some() || priority_throttle_limited"));
    assert!(attempt.contains("segment_budget_wait_action"));
    assert!(attempt.contains("DownloadOutcome::Deferred"));
}

#[test]
fn low_speed_recovery_retries_only_after_sustained_unlimited_slowdown() {
    let profile = performance_profile();
    let mut monitor = LowSpeedMonitor::new(profile);

    assert_eq!(
        monitor.observe(4 * 1024, Duration::from_secs(10), false),
        LowSpeedDecision::Continue
    );
    assert_eq!(
        monitor.observe(4 * 1024, Duration::from_secs(21), false),
        LowSpeedDecision::Retry
    );
    assert_eq!(
        monitor.observe(0, Duration::from_secs(30), true),
        LowSpeedDecision::Continue
    );
}

#[test]
fn host_range_backoff_expires_after_ten_minutes() {
    let backoff = RangeBackoffRegistry::default();
    let now = Instant::now();
    let url = "https://example.com/downloads/file.zip";

    assert!(!backoff.is_backed_off(url, now));
    backoff.record_rejection(url, now);

    assert!(backoff.is_backed_off(url, now + Duration::from_secs(599)));
    assert!(!backoff.is_backed_off(url, now + RANGE_BACKOFF_DURATION));
}

#[test]
fn range_backoff_does_not_apply_to_different_files_on_same_host() {
    let backoff = RangeBackoffRegistry::default();
    let now = Instant::now();
    let rejected_url = "https://dl.fuckingfast.co/dl/token-part03/Game.part03.rar?download=1";
    let other_path_url = "https://dl.fuckingfast.co/dl/token-part04/Game.part04.rar?download=1";
    let other_query_url = "https://dl.fuckingfast.co/dl/token-part03/Game.part03.rar?download=2";

    backoff.record_rejection(rejected_url, now);

    assert!(backoff.is_backed_off(rejected_url, now + Duration::from_secs(1)));
    assert!(!backoff.is_backed_off(other_path_url, now + Duration::from_secs(1)));
    assert!(!backoff.is_backed_off(other_query_url, now + Duration::from_secs(1)));
}

#[test]
fn range_backoff_supports_source_keyed_hoster_policies() {
    let backoff = RangeBackoffRegistry::default();
    let now = Instant::now();
    let key = "hoster:datanodes:abc123456789";

    assert!(!backoff.is_key_backed_off(key, now));
    backoff.record_key_rejection(key, now);

    assert!(backoff.is_key_backed_off(key, now + Duration::from_secs(1)));
    assert!(!backoff.is_key_backed_off("hoster:datanodes:other-file", now + Duration::from_secs(1)));
    assert!(!backoff.is_key_backed_off(key, now + RANGE_BACKOFF_DURATION));
}

#[test]
fn large_bulk_member_at_seventeen_kib_per_second_retries_without_partial_reset() {
    let profile = performance_profile();
    let recovery_state = crate::state::BulkMemberSlowRecoveryState {
        retry_attempts: 0,
        max_retry_attempts: 3,
    };

    assert_eq!(
        bulk_slow_stream_recovery_action(
            17 * 1024 * 20,
            Duration::from_secs(20),
            Some(500 * 1024 * 1024),
            2 * 1024 * 1024,
            None,
            profile,
            Some(recovery_state),
        ),
        BulkSlowStreamRecoveryAction::Retry
    );
}

#[test]
fn bulk_slow_recovery_ignores_non_bulk_and_speed_limited_downloads() {
    let profile = performance_profile();
    let recovery_state = crate::state::BulkMemberSlowRecoveryState {
        retry_attempts: 0,
        max_retry_attempts: 3,
    };

    assert_eq!(
        bulk_slow_stream_recovery_action(
            17 * 1024 * 20,
            Duration::from_secs(20),
            Some(500 * 1024 * 1024),
            2 * 1024 * 1024,
            None,
            profile,
            None,
        ),
        BulkSlowStreamRecoveryAction::Continue
    );
    assert_eq!(
        bulk_slow_stream_recovery_action(
            17 * 1024 * 20,
            Duration::from_secs(20),
            Some(500 * 1024 * 1024),
            2 * 1024 * 1024,
            Some(64 * 1024),
            profile,
            Some(recovery_state),
        ),
        BulkSlowStreamRecoveryAction::Continue
    );
}

#[test]
fn near_complete_bulk_slow_recovery_preserves_partial_file() {
    let profile = performance_profile();
    let recovery_state = crate::state::BulkMemberSlowRecoveryState {
        retry_attempts: 1,
        max_retry_attempts: 3,
    };

    assert_eq!(
        bulk_slow_stream_recovery_action(
            17 * 1024 * 20,
            Duration::from_secs(20),
            Some(500 * 1024 * 1024),
            499 * 1024 * 1024,
            None,
            profile,
            Some(recovery_state),
        ),
        BulkSlowStreamRecoveryAction::Retry
    );
}

#[test]
fn exhausted_bulk_slow_recovery_recycles_stream_and_preserves_partial() {
    let profile = performance_profile();
    let recovery_state = crate::state::BulkMemberSlowRecoveryState {
        retry_attempts: 3,
        max_retry_attempts: 3,
    };

    assert_eq!(
        bulk_slow_stream_recovery_action(
            64 * 1024 * 15,
            Duration::from_secs(20),
            Some(500 * 1024 * 1024),
            2 * 1024 * 1024,
            None,
            profile,
            Some(recovery_state),
        ),
        BulkSlowStreamRecoveryAction::Retry
    );
}

#[tokio::test]
async fn send_request_asks_for_identity_encoding() {
    let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
    let (url, request_handle) = spawn_one_response_server(response).await;
    let client = download_client().unwrap();

    let _response = send_request(&client, &url, 0, None, None).await.unwrap();
    let request = request_handle.await.unwrap();

    assert!(request
        .to_ascii_lowercase()
        .contains("accept-encoding: identity"));
}

#[tokio::test]
async fn send_request_applies_authenticated_handoff_headers() {
    let (url, request_handle) = spawn_cookie_required_server().await;
    let client = download_client().unwrap();
    let auth = HandoffAuth {
        headers: vec![HandoffAuthHeader {
            name: "Cookie".into(),
            value: "session=abc".into(),
        }],
    };

    let response = send_request(&client, &url, 0, Some(&auth), None)
        .await
        .unwrap();
    let request = request_handle.await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(request.to_ascii_lowercase().contains("cookie: session=abc"));
    assert!(request
        .to_ascii_lowercase()
        .contains("accept-encoding: identity"));
}

#[tokio::test]
async fn protected_handoff_access_probe_rejects_missing_browser_auth() {
    let (url, request_handle) = spawn_cookie_required_server().await;

    let error = probe_browser_handoff_access(&url, None)
        .await
        .expect_err("missing browser auth should reject protected downloads before queuing");
    let request = request_handle.await.unwrap();

    assert_eq!(error.code, "PROTECTED_DOWNLOAD_AUTH_REQUIRED");
    assert_eq!(error.status, Some(403));
    assert!(request.to_ascii_lowercase().contains("range: bytes=0-0"));
    assert!(request
        .to_ascii_lowercase()
        .contains("accept-encoding: identity"));
}

#[tokio::test]
async fn protected_handoff_access_probe_accepts_captured_browser_auth() {
    let (url, request_handle) = spawn_cookie_required_server().await;
    let auth = HandoffAuth {
        headers: vec![HandoffAuthHeader {
            name: "Cookie".into(),
            value: "session=abc".into(),
        }],
    };

    let result = probe_browser_handoff_access(&url, Some(&auth)).await;
    let request = request_handle.await.unwrap();

    assert!(result.is_ok());
    assert!(request.to_ascii_lowercase().contains("cookie: session=abc"));
    assert!(request.to_ascii_lowercase().contains("range: bytes=0-0"));
}

#[tokio::test]
async fn protected_handoff_access_probe_rejects_html_landing_pages() {
    let response = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/html; charset=utf-8\r\n",
        "Content-Length: 38\r\n",
        "\r\n",
        "<html><body>login required</body></html>"
    );
    let (url, request_handle) = spawn_one_response_server(response).await;

    let error = probe_browser_handoff_access(&url, None)
        .await
        .expect_err("HTML landing pages should not be accepted as browser download bytes");
    let request = request_handle.await.unwrap();

    assert_eq!(error.code, PROTECTED_DOWNLOAD_AUTH_REQUIRED_CODE);
    assert_eq!(error.status, Some(200));
    assert!(error.message.contains("HTML"));
    assert!(request.to_ascii_lowercase().contains("range: bytes=0-0"));
}

#[tokio::test]
async fn protected_handoff_access_probe_allows_explicit_html_attachments() {
    let response = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/html; charset=utf-8\r\n",
        "Content-Disposition: attachment; filename=\"report.html\"\r\n",
        "Content-Length: 31\r\n",
        "\r\n",
        "<html><body>report</body></html>"
    );
    let (url, request_handle) = spawn_one_response_server(response).await;

    let result = probe_browser_handoff_access(&url, None).await;
    let request = request_handle.await.unwrap();

    assert!(result.is_ok());
    assert!(request.to_ascii_lowercase().contains("range: bytes=0-0"));
}

#[tokio::test]
async fn authenticated_handoff_redirects_to_cross_origin_without_forwarding_auth() {
    let (url, request_handle) =
        spawn_authenticated_cross_origin_redirect_server(b"redirected bytes").await;
    let client = download_client().unwrap();
    let auth = HandoffAuth {
        headers: vec![HandoffAuthHeader {
            name: "Cookie".into(),
            value: "session=abc".into(),
        }],
    };

    let response = send_request(&client, &url, 0, Some(&auth), None)
        .await
        .expect("signed cross-origin redirects should be followed without browser credentials");
    let requests = request_handle.await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(requests.len(), 2);
    assert!(requests[0]
        .to_ascii_lowercase()
        .contains("cookie: session=abc"));
    assert!(
        !requests[1].to_ascii_lowercase().contains("cookie:"),
        "browser credentials must not be forwarded to a redirected CDN origin",
    );
}

#[tokio::test]
async fn protected_handoff_access_probe_allows_cross_origin_signed_redirects() {
    let (url, request_handle) = spawn_authenticated_cross_origin_redirect_server(b"").await;
    let auth = HandoffAuth {
        headers: vec![HandoffAuthHeader {
            name: "Cookie".into(),
            value: "session=abc".into(),
        }],
    };

    let result = probe_browser_handoff_access(&url, Some(&auth)).await;
    let requests = request_handle.await.unwrap();

    assert!(result.is_ok());
    assert!(requests[0]
        .to_ascii_lowercase()
        .contains("cookie: session=abc"));
    assert!(
        !requests[1].to_ascii_lowercase().contains("cookie:"),
        "access probes should drop browser credentials after a cross-origin redirect",
    );
}

#[test]
fn authenticated_redirect_policy_identifies_same_origin_redirects() {
    assert!(redirect_keeps_origin(
        "https://chatgpt.com/backend-api/estuary/content?id=file_123",
        "https://chatgpt.com/backend-api/estuary/content?id=file_456",
    ));
    assert!(!redirect_keeps_origin(
        "https://chatgpt.com/backend-api/estuary/content?id=file_123",
        "https://cdn.example.com/file.pdf",
    ));
}

async fn spawn_authenticated_cross_origin_redirect_server(
    body: &'static [u8],
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let redirect_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect_address = redirect_listener.local_addr().unwrap();
    let target_address = target_listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut redirect_socket, _) = redirect_listener.accept().await.unwrap();
        let mut redirect_buffer = vec![0_u8; 4096];
        let redirect_read = redirect_socket.read(&mut redirect_buffer).await.unwrap();
        let redirect_request =
            String::from_utf8_lossy(&redirect_buffer[..redirect_read]).to_string();
        let redirect_response = format!(
            "HTTP/1.1 302 Found\r\nLocation: http://{target_address}/cdn.bin\r\nContent-Length: 0\r\n\r\n"
        );
        redirect_socket
            .write_all(redirect_response.as_bytes())
            .await
            .unwrap();

        let (mut target_socket, _) = target_listener.accept().await.unwrap();
        let mut target_buffer = vec![0_u8; 4096];
        let target_read = target_socket.read(&mut target_buffer).await.unwrap();
        let target_request = String::from_utf8_lossy(&target_buffer[..target_read]).to_string();
        let target_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body),
        );
        target_socket
            .write_all(target_response.as_bytes())
            .await
            .unwrap();

        vec![redirect_request, target_request]
    });

    (format!("http://{redirect_address}/download.bin"), handle)
}

#[test]
fn range_rejection_after_probe_requests_single_stream_fallback() {
    let resume_error = download_error(
        FailureCategory::Resume,
        "The server did not honor a segmented range request.".into(),
        false,
    );
    let network_error =
        download_error(FailureCategory::Network, "The network failed.".into(), true);

    assert!(segmented_error_allows_single_stream_fallback(&resume_error));
    assert!(!segmented_error_allows_single_stream_fallback(
        &network_error
    ));
}

#[tokio::test]
async fn range_request_sends_if_range_when_resume_validator_is_available() {
    let response = concat!(
        "HTTP/1.1 206 Partial Content\r\n",
        "Content-Range: bytes 4-7/12\r\n",
        "Content-Length: 4\r\n",
        "\r\n",
        "efgh"
    );
    let (url, request_handle) = spawn_one_response_server(response).await;
    let client = download_client().unwrap();
    let validators = EntityValidators {
        etag: Some("\"abc123\"".into()),
        last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
    };

    let _response = send_range_request(
        &client,
        &url,
        ByteRange { start: 4, end: 7 },
        None,
        Some(&validators),
    )
    .await
    .unwrap();
    let request = request_handle.await.unwrap();
    let request_lower = request.to_ascii_lowercase();

    assert!(request_lower.contains("range: bytes=4-7"));
    assert!(request_lower.contains("if-range: \"abc123\""));
}

#[test]
fn retry_delay_honors_retry_after_and_applies_stable_jitter() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::RETRY_AFTER,
        reqwest::header::HeaderValue::from_static("120"),
    );

    assert_eq!(
        retry_delay_for_response(
            StatusCode::TOO_MANY_REQUESTS,
            &headers,
            0,
            "job_a",
            "https://example.com/file.bin",
        ),
        Duration::from_secs(60)
    );

    let first = retry_delay_for_response(
        StatusCode::SERVICE_UNAVAILABLE,
        &reqwest::header::HeaderMap::new(),
        1,
        "job_a",
        "https://example.com/file.bin",
    );
    let second = retry_delay_for_response(
        StatusCode::SERVICE_UNAVAILABLE,
        &reqwest::header::HeaderMap::new(),
        1,
        "job_b",
        "https://example.com/file.bin",
    );

    assert!(first >= REQUEST_RETRY_DELAYS[1]);
    assert!(first <= REQUEST_RETRY_DELAYS[1] + Duration::from_millis(250));
    assert_ne!(
        first, second,
        "bulk retry jitter should be stable but de-synchronized"
    );
}

#[test]
fn worker_control_signal_maps_live_control_without_state_lookup() {
    let signal = WorkerControlSignal::default();

    assert_eq!(signal.current_outcome(), None);
    signal.store_control(WorkerControl::Paused);
    assert_eq!(signal.current_outcome(), Some(DownloadOutcome::Paused));
    signal.store_control(WorkerControl::Canceled);
    assert_eq!(signal.current_outcome(), Some(DownloadOutcome::Canceled));
    signal.store_control(WorkerControl::Missing);
    assert_eq!(signal.current_outcome(), Some(DownloadOutcome::Canceled));
    signal.store_control(WorkerControl::Continue);
    assert_eq!(signal.current_outcome(), None);
}
