use super::*;

const BULK_SLOW_RECOVERY_MIN_SIZE: u64 = 32 * 1024 * 1024;
const BULK_SLOW_RECOVERY_RESET_PARTIAL_MAX_BYTES: u64 = 16 * 1024 * 1024;
const BULK_SLOW_RECOVERY_BALANCED_THRESHOLD: u64 = 64 * 1024;
const BULK_SLOW_RECOVERY_FAST_THRESHOLD: u64 = 128 * 1024;
const BULK_HOSTER_FAIRNESS_RELEASE_THRESHOLD: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BulkSlowStreamRecoveryAction {
    Continue,
    Retry { reset_partial: bool },
}

pub(super) async fn run_http_download_attempt(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
) -> Result<DownloadOutcome, DownloadError> {
    let mut current_url = match refresh_hoster_url_before_attempt(state, task).await {
        Ok(Some(url)) => url,
        Ok(None) => task.url.clone(),
        Err(error) => return Err(error),
    };
    let mut refreshed_after_failure = false;

    loop {
        match run_http_download_attempt_for_url(app, state, task, &current_url).await {
            Ok(outcome) => return Ok(outcome),
            Err(error)
                if !refreshed_after_failure
                    && hoster_refresh_error_allows_retry(&error)
                    && task.resolved_from_url.is_some() =>
            {
                refreshed_after_failure = true;
                match refresh_hoster_url_after_failure(state, task, &error).await {
                    Ok(Some(url)) => {
                        cleanup_partial_artifacts(&task.temp_path).await;
                        let snapshot = state.sync_downloaded_bytes(&task.id, 0).await?;
                        emit_download_update(app, &snapshot, &task.id);
                        current_url = url;
                    }
                    Ok(None) | Err(_) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    }
}

async fn run_http_download_attempt_for_url(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
    effective_url: &str,
) -> Result<DownloadOutcome, DownloadError> {
    ensure_parent_directory(&task.target_path)
        .await
        .map_err(disk_error)?;

    let mut existing_bytes = metadata_len(&task.temp_path).await.unwrap_or(0);
    let client = download_client()?;
    let request_auth = request_auth_for_task_url(task, effective_url);
    let speed_limit = state
        .speed_limit_bytes_per_second_for_task(task.is_bulk_member)
        .await;
    let performance_mode = state
        .download_performance_mode_for_task(task.is_bulk_member)
        .await;
    let profile = performance_profile(performance_mode);
    let segment_attempt =
        segment_attempt_context_for_task(state, task, effective_url, performance_mode).await;

    let mut preflight_metadata =
        preflight_download(&client, effective_url, request_auth.as_ref()).await;

    let has_segment_state = segment_meta_path(&task.temp_path).exists();
    let can_try_segmented = segment_attempt.is_some()
        && (existing_bytes == 0 || has_segment_state)
        && speed_limit.is_none()
        && profile.max_segments >= 2
        && !segment_attempt
            .as_ref()
            .is_some_and(|attempt| attempt.is_backed_off(effective_url, Instant::now()));

    if can_try_segmented {
        let segment_attempt = segment_attempt
            .as_ref()
            .expect("segment attempt context exists when segmented download can start");
        let probe_metadata =
            probe_range_metadata(&client, effective_url, request_auth.as_ref()).await;
        let mut range_probe_supported = false;
        match probe_metadata {
            Some(metadata) => {
                range_probe_supported = true;
                preflight_metadata = Some(merge_preflight_metadata(preflight_metadata, metadata));
            }
            None => {
                segment_attempt.record_rejection(effective_url, Instant::now());
            }
        }

        if range_probe_supported {
            if let Some(metadata) = preflight_metadata.as_ref() {
                if let Some(total_bytes) = metadata.total_bytes {
                    if let Some(plan) = plan_segmented_ranges_with_budget(
                        total_bytes,
                        metadata.resume_support,
                        speed_limit,
                        profile,
                        segment_attempt.segment_budget,
                    ) {
                        match run_segmented_download_attempt(
                            app,
                            state,
                            task,
                            client.clone(),
                            effective_url.to_string(),
                            request_auth.clone(),
                            plan,
                            profile,
                            metadata.validators.clone(),
                        )
                        .await
                        {
                            Ok(outcome) => return Ok(outcome),
                            Err(error) if segmented_error_allows_single_stream_fallback(&error) => {
                                segment_attempt.record_rejection(effective_url, Instant::now());
                                cleanup_partial_artifacts(&task.temp_path).await;
                                existing_bytes = 0;
                            }
                            Err(error) => return Err(error),
                        }
                    }
                }
            }
        }

        if has_segment_state {
            cleanup_partial_artifacts(&task.temp_path).await;
            existing_bytes = 0;
        }
    } else if has_segment_state {
        cleanup_partial_artifacts(&task.temp_path).await;
        existing_bytes = 0;
    }

    let mut response = send_request(
        &client,
        effective_url,
        existing_bytes,
        request_auth.as_ref(),
        preflight_metadata
            .as_ref()
            .map(|metadata| &metadata.validators),
    )
    .await?;
    reject_hoster_html_response(task, &response)?;
    let supports_resume = response.status() == StatusCode::PARTIAL_CONTENT;

    if existing_bytes > 0 && !supports_resume {
        truncate_file(&task.temp_path).await.map_err(disk_error)?;
        existing_bytes = 0;
        let snapshot = state
            .mark_job_downloading(
                &task.id,
                0,
                response.content_length(),
                ResumeSupport::Unsupported,
                extract_filename(&response),
            )
            .await?;
        emit_snapshot(app, &snapshot);
        response = send_request(&client, effective_url, 0, request_auth.as_ref(), None).await?;
        reject_hoster_html_response(task, &response)?;
    }

    let total_bytes = derive_total_bytes(&response, existing_bytes).or_else(|| {
        preflight_metadata
            .as_ref()
            .and_then(|metadata| metadata.total_bytes)
    });
    let resume_support = derive_resume_support(&response, existing_bytes);
    let display_filename = extract_filename(&response)
        .or_else(|| {
            preflight_metadata
                .as_ref()
                .and_then(|metadata| metadata.filename.clone())
        })
        .or_else(|| derive_filename_from_url(response.url().as_str()));
    let target_path = derive_target_path(&task.target_path, &response);
    let snapshot = state
        .mark_job_downloading(
            &task.id,
            existing_bytes,
            total_bytes,
            resume_support,
            display_filename,
        )
        .await?;
    emit_snapshot(app, &snapshot);
    let bulk_slow_recovery_state = state
        .bulk_member_slow_recovery_state(&task.id)
        .await
        .map_err(|error| {
            download_error(
                FailureCategory::Internal,
                format!("Could not inspect bulk slow-recovery state: {error}"),
                false,
            )
        })?;

    let file = if existing_bytes > 0 {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&task.temp_path)
            .await
            .map_err(|error| disk_error(format!("Could not open partial download file: {error}")))?
    } else {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&task.temp_path)
            .await
            .map_err(|error| disk_error(format!("Could not create download file: {error}")))?
    };
    let mut file = BufWriter::with_capacity(DOWNLOAD_BUFFER_SIZE, file);

    let mut stream = response.bytes_stream();
    let mut downloaded_bytes = existing_bytes;
    let attempt_started = Instant::now();
    let mut attempt_transferred_bytes = 0_u64;
    let mut sample_bytes = 0_u64;
    let mut sample_started = Instant::now();
    let mut displayed_speed = RollingSpeed::with_alpha(profile.speed_smoothing_alpha);
    let mut low_speed_monitor = LowSpeedMonitor::new(profile);
    let mut low_speed_bytes = 0_u64;
    let mut low_speed_started = Instant::now();
    let mut last_emitted_bytes = existing_bytes;
    let mut last_persisted_at = Instant::now();

    while let Some(chunk_result) = stream.next().await {
        match state.worker_control(&task.id).await {
            WorkerControl::Paused => {
                file.flush().await.ok();
                return Ok(DownloadOutcome::Paused);
            }
            WorkerControl::Canceled => {
                file.flush().await.ok();
                return Ok(DownloadOutcome::Canceled);
            }
            WorkerControl::Missing => {
                file.flush().await.ok();
                return Ok(DownloadOutcome::Canceled);
            }
            WorkerControl::Continue => {}
        }

        let chunk = match chunk_result {
            Ok(chunk) => chunk,
            Err(error) => {
                file.flush().await.ok();
                return Err(download_stream_error(error));
            }
        };
        let chunk_len = chunk.len() as u64;
        file.write_all(&chunk)
            .await
            .map_err(|error| disk_error(format!("Could not write download chunk: {error}")))?;

        downloaded_bytes = downloaded_bytes.saturating_add(chunk_len);
        attempt_transferred_bytes = attempt_transferred_bytes.saturating_add(chunk_len);
        sample_bytes = sample_bytes.saturating_add(chunk_len);

        if let Some(limit) = speed_limit {
            match throttle_download(
                state,
                &task.id,
                limit,
                attempt_transferred_bytes,
                attempt_started,
            )
            .await
            {
                WorkerControl::Paused => {
                    file.flush().await.ok();
                    return Ok(DownloadOutcome::Paused);
                }
                WorkerControl::Canceled | WorkerControl::Missing => {
                    file.flush().await.ok();
                    return Ok(DownloadOutcome::Canceled);
                }
                WorkerControl::Continue => {}
            }
        }

        let elapsed = sample_started.elapsed();

        low_speed_bytes = low_speed_bytes.saturating_add(chunk_len);
        let low_speed_elapsed = low_speed_started.elapsed();
        if low_speed_elapsed >= profile.low_speed_window {
            match bulk_slow_stream_recovery_action(
                low_speed_bytes,
                low_speed_elapsed,
                total_bytes,
                downloaded_bytes,
                speed_limit,
                profile,
                bulk_slow_recovery_state,
            ) {
                BulkSlowStreamRecoveryAction::Retry { reset_partial } => {
                    file.flush().await.ok();
                    drop(file);
                    if reset_partial {
                        cleanup_partial_artifacts(&task.temp_path).await;
                        let snapshot = state.sync_downloaded_bytes(&task.id, 0).await?;
                        emit_download_update(app, &snapshot, &task.id);
                    }
                    return Err(download_error(
                        FailureCategory::Network,
                        "Bulk member download speed stayed below the recovery threshold; retrying the stream."
                            .into(),
                        true,
                    ));
                }
                BulkSlowStreamRecoveryAction::Continue => {
                    if low_speed_monitor.observe(
                        low_speed_bytes,
                        low_speed_elapsed,
                        speed_limit.is_some(),
                    ) == LowSpeedDecision::Retry
                    {
                        file.flush().await.ok();
                        return Err(download_error(
                            FailureCategory::Network,
                            "Download speed stayed below the recovery threshold; retrying the stream."
                                .into(),
                            true,
                        ));
                    }
                }
            }
            low_speed_bytes = 0;
            low_speed_started = Instant::now();
        }

        if elapsed >= PROGRESS_UPDATE_INTERVAL {
            let speed = displayed_speed.record_sample(sample_bytes, elapsed);
            let should_persist = last_persisted_at.elapsed() >= PROGRESS_PERSIST_INTERVAL;
            let snapshot = state
                .update_job_progress(
                    &task.id,
                    downloaded_bytes,
                    total_bytes,
                    speed,
                    should_persist,
                )
                .await?;
            emit_download_update(app, &snapshot, &task.id);
            if task_releases_bulk_hoster_fairness(task, speed) {
                schedule_downloads(app.clone(), state.clone());
            }
            last_emitted_bytes = downloaded_bytes;
            if should_persist {
                last_persisted_at = Instant::now();
            }
            sample_bytes = 0;
            sample_started = Instant::now();
        }
    }

    file.flush()
        .await
        .map_err(|error| disk_error(format!("Could not flush download file: {error}")))?;
    file.get_mut()
        .sync_all()
        .await
        .map_err(|error| disk_error(format!("Could not sync download file: {error}")))?;
    drop(file);

    if let Some(total_bytes) = total_bytes {
        if downloaded_bytes < total_bytes {
            return Err(download_error(
                FailureCategory::Network,
                format!(
                    "Download ended early. Received {downloaded_bytes} of {total_bytes} bytes."
                ),
                true,
            ));
        }
    }

    if downloaded_bytes != last_emitted_bytes {
        let should_persist = last_persisted_at.elapsed() >= PROGRESS_PERSIST_INTERVAL;
        let snapshot = state
            .update_job_progress(&task.id, downloaded_bytes, total_bytes, 0, should_persist)
            .await?;
        emit_download_update(app, &snapshot, &task.id);
    }

    let final_path = move_to_final_path(&task.temp_path, &target_path)
        .await
        .map_err(disk_error)?;
    complete_http_download(app, state, task, downloaded_bytes, &final_path).await?;
    Ok(DownloadOutcome::Completed)
}

fn request_auth_for_task_url(task: &crate::state::DownloadTask, url: &str) -> Option<HandoffAuth> {
    let mut auth = task.handoff_auth.clone().unwrap_or(HandoffAuth {
        headers: Vec::new(),
    });
    if let Some(context) = crate::hosters::hoster_download_context_for_resolved_url(
        url,
        task.resolved_from_url.as_deref(),
    ) {
        for header in context.headers {
            if auth
                .headers
                .iter()
                .any(|existing| existing.name.eq_ignore_ascii_case(&header.name))
            {
                continue;
            }
            auth.headers.push(header);
        }
    }

    (!auth.headers.is_empty()).then_some(auth)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentAttemptContext {
    backoff_key: Option<String>,
    segment_budget: Option<usize>,
}

impl SegmentAttemptContext {
    fn is_backed_off(&self, effective_url: &str, now: Instant) -> bool {
        self.backoff_key
            .as_deref()
            .map(|key| range_backoffs().is_key_backed_off(key, now))
            .unwrap_or_else(|| range_backoffs().is_backed_off(effective_url, now))
    }

    fn record_rejection(&self, effective_url: &str, now: Instant) {
        if let Some(key) = self.backoff_key.as_deref() {
            range_backoffs().record_key_rejection(key, now);
        } else {
            range_backoffs().record_rejection(effective_url, now);
        }
    }
}

async fn segment_attempt_context_for_task(
    state: &SharedState,
    task: &crate::state::DownloadTask,
    effective_url: &str,
    performance_mode: DownloadPerformanceMode,
) -> Option<SegmentAttemptContext> {
    if task.is_bulk_member && task.resolved_from_url.is_none() {
        let budget = direct_bulk_segment_budget_for_task(state, task, effective_url).await?;
        return Some(SegmentAttemptContext {
            backoff_key: None,
            segment_budget: Some(budget),
        });
    }

    if task.is_bulk_member && task.resolved_from_url.is_some() {
        if state.bulk_hoster_acceleration_mode().await == BulkHosterAccelerationMode::Off {
            return None;
        }
        let source_url = task.resolved_from_url.as_deref()?;
        let policy = crate::hosters::hoster_acceleration_policy(source_url, effective_url)?;
        let policy_cap = hoster_segment_cap_for_mode(&policy, performance_mode);
        let budget =
            protected_hoster_bulk_segment_budget_for_task(state, task, effective_url, policy_cap)
                .await?;
        return Some(SegmentAttemptContext {
            backoff_key: Some(policy.backoff_key),
            segment_budget: Some(budget),
        });
    }

    Some(SegmentAttemptContext {
        backoff_key: None,
        segment_budget: None,
    })
}

#[cfg(test)]
pub(super) fn task_allows_segmented_download(task: &crate::state::DownloadTask) -> bool {
    task_allows_segmented_download_with_mode(task, BulkHosterAccelerationMode::Safe)
}

#[cfg(test)]
pub(super) fn task_allows_segmented_download_with_mode(
    task: &crate::state::DownloadTask,
    hoster_acceleration_mode: BulkHosterAccelerationMode,
) -> bool {
    if !(task.is_bulk_member && task.resolved_from_url.is_some()) {
        return true;
    }

    if hoster_acceleration_mode == BulkHosterAccelerationMode::Off {
        return false;
    }

    let Some(source_url) = task.resolved_from_url.as_deref() else {
        return false;
    };
    crate::hosters::hoster_acceleration_policy(source_url, &task.url).is_some()
}

pub(super) fn hoster_segment_cap_for_mode(
    policy: &crate::hosters::HosterAccelerationPolicy,
    performance_mode: DownloadPerformanceMode,
) -> usize {
    match performance_mode {
        DownloadPerformanceMode::Stable => 1,
        DownloadPerformanceMode::Balanced => policy.max_balanced_segments,
        DownloadPerformanceMode::Fast => policy.max_fast_segments,
    }
}

async fn direct_bulk_segment_budget_for_task(
    state: &SharedState,
    task: &crate::state::DownloadTask,
    effective_url: &str,
) -> Option<usize> {
    if !(task.is_bulk_member && task.resolved_from_url.is_none()) {
        return None;
    }

    let (active_direct_bulk_workers, active_same_origin_workers) = state
        .active_direct_bulk_worker_counts(&task.id, effective_url)
        .await;
    segment_budget_from_counts(
        DIRECT_BULK_TOTAL_SEGMENT_CONNECTION_BUDGET,
        DIRECT_BULK_ORIGIN_SEGMENT_CONNECTION_BUDGET,
        active_direct_bulk_workers,
        active_same_origin_workers,
        usize::MAX,
    )
}

async fn protected_hoster_bulk_segment_budget_for_task(
    state: &SharedState,
    task: &crate::state::DownloadTask,
    effective_url: &str,
    policy_cap: usize,
) -> Option<usize> {
    if !(task.is_bulk_member && task.resolved_from_url.is_some()) || policy_cap < 2 {
        return None;
    }

    let (active_hoster_workers, active_same_origin_workers) = state
        .active_protected_hoster_bulk_worker_counts(&task.id, effective_url)
        .await;
    segment_budget_from_counts(
        HOSTER_BULK_TOTAL_SEGMENT_CONNECTION_BUDGET,
        HOSTER_BULK_ORIGIN_SEGMENT_CONNECTION_BUDGET,
        active_hoster_workers,
        active_same_origin_workers,
        policy_cap,
    )
}

pub(super) fn segment_budget_from_counts(
    total_budget: usize,
    origin_budget: usize,
    active_workers: usize,
    active_same_origin_workers: usize,
    policy_cap: usize,
) -> Option<usize> {
    let total_share = total_budget / active_workers.max(1);
    let origin_share = origin_budget / active_same_origin_workers.max(1);
    let budget = policy_cap.min(total_share).min(origin_share);
    (budget >= 2).then_some(budget)
}

pub(super) fn task_releases_bulk_hoster_fairness(
    task: &crate::state::DownloadTask,
    speed: u64,
) -> bool {
    task.is_bulk_member
        && task.resolved_from_url.is_some()
        && speed >= BULK_HOSTER_FAIRNESS_RELEASE_THRESHOLD
}

async fn refresh_hoster_url_before_attempt(
    state: &SharedState,
    task: &crate::state::DownloadTask,
) -> Result<Option<String>, DownloadError> {
    if task.is_bulk_member && task.resolved_from_url.is_some() {
        state.mark_bulk_hoster_resolving(&task.id).await;
    }
    let result = refresh_hoster_url_for_task(task).await;
    if let Err(error) = &result {
        let _ = state
            .record_diagnostic_event(
                DiagnosticLevel::Warning,
                "download",
                format!(
                    "Could not refresh hoster link before download: {}",
                    error.message
                ),
                Some(task.id.clone()),
            )
            .await;
    }
    result
}

async fn refresh_hoster_url_after_failure(
    state: &SharedState,
    task: &crate::state::DownloadTask,
    failure: &DownloadError,
) -> Result<Option<String>, DownloadError> {
    let result = refresh_hoster_url_for_task(task).await;
    if let Err(error) = &result {
        let _ = state
            .record_diagnostic_event(
                DiagnosticLevel::Warning,
                "download",
                format!(
                    "Could not refresh hoster link after download failure ({}): {}",
                    failure.message, error.message
                ),
                Some(task.id.clone()),
            )
            .await;
    }
    result
}

async fn refresh_hoster_url_for_task(
    task: &crate::state::DownloadTask,
) -> Result<Option<String>, DownloadError> {
    let Some(source_url) = task.resolved_from_url.as_deref() else {
        return Ok(None);
    };
    let refreshed = crate::hosters::refresh_resolved_hoster_link(source_url)
        .await
        .map_err(hoster_resolution_download_error)?;
    Ok(Some(refreshed.url))
}

pub(super) fn hoster_resolution_download_error(
    error: crate::hosters::HosterResolutionError,
) -> DownloadError {
    download_error(
        FailureCategory::Http,
        format!("Could not refresh hoster link: {}", error.message),
        error.retryable,
    )
}

pub(super) fn hoster_refresh_error_allows_retry(error: &DownloadError) -> bool {
    match error.category {
        FailureCategory::Resume => true,
        FailureCategory::Http => {
            error.message.contains("403")
                || error.message.contains("404")
                || error.message.contains("410")
                || error.message.contains("416")
                || error.message.to_ascii_lowercase().contains("html")
        }
        FailureCategory::Network => error.message.to_ascii_lowercase().contains("ended early"),
        _ => false,
    }
}

fn reject_hoster_html_response(
    task: &crate::state::DownloadTask,
    response: &reqwest::Response,
) -> Result<(), DownloadError> {
    if task.resolved_from_url.is_none() {
        return Ok(());
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.contains("text/html") || content_type.contains("application/xhtml") {
        return Err(download_error(
            FailureCategory::Http,
            "Hoster direct link returned HTML instead of file content.".into(),
            true,
        ));
    }

    Ok(())
}

pub(super) async fn complete_http_download(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
    total_bytes: u64,
    final_path: &Path,
) -> Result<(), DownloadError> {
    let actual_sha256 = if state.job_requires_sha256(&task.id).await {
        Some(compute_sha256(final_path).await.map_err(disk_error)?)
    } else {
        None
    };
    let snapshot = state
        .complete_job_with_integrity(&task.id, total_bytes, final_path, actual_sha256)
        .await?;
    let failed_integrity = snapshot.jobs.iter().any(|job| {
        job.id == task.id
            && job.state == crate::storage::JobState::Failed
            && job.failure_category == Some(FailureCategory::Integrity)
    });
    emit_snapshot(app, &snapshot);

    if failed_integrity {
        notify_download_failure(
            app,
            state,
            task,
            snapshot
                .jobs
                .iter()
                .find(|job| job.id == task.id)
                .and_then(|job| job.error.as_deref()),
        )
        .await;
        return Ok(());
    }

    handle_bulk_archive_after_completion(app, state, &task.id).await?;
    notify_download_completed(app, state, final_path).await;
    Ok(())
}

pub(super) async fn handle_bulk_archive_after_completion(
    app: &AppHandle,
    state: &SharedState,
    job_id: &str,
) -> Result<(), String> {
    if let Some(archive) = state.bulk_archive_ready_for_job(job_id).await? {
        let _ = create_bulk_archive_from_ready(app, state, archive, Some(job_id.into())).await;
    }

    Ok(())
}

pub(super) fn bulk_slow_stream_recovery_action(
    sample_bytes: u64,
    elapsed: Duration,
    total_bytes: Option<u64>,
    downloaded_bytes: u64,
    speed_limit: Option<u64>,
    profile: DownloadPerformanceProfile,
    recovery_state: Option<BulkMemberSlowRecoveryState>,
) -> BulkSlowStreamRecoveryAction {
    let Some(recovery_state) = recovery_state else {
        return BulkSlowStreamRecoveryAction::Continue;
    };
    let Some(total_bytes) = total_bytes else {
        return BulkSlowStreamRecoveryAction::Continue;
    };
    if speed_limit.is_some()
        || total_bytes < BULK_SLOW_RECOVERY_MIN_SIZE
        || elapsed < profile.low_speed_window
        || elapsed.is_zero()
    {
        return BulkSlowStreamRecoveryAction::Continue;
    }

    let sample_speed = (sample_bytes as f64 / elapsed.as_secs_f64()) as u64;
    if sample_speed >= bulk_slow_recovery_threshold(profile) {
        return BulkSlowStreamRecoveryAction::Continue;
    }

    if recovery_state.retry_attempts >= recovery_state.max_retry_attempts {
        return BulkSlowStreamRecoveryAction::Retry {
            reset_partial: false,
        };
    }

    BulkSlowStreamRecoveryAction::Retry {
        reset_partial: downloaded_bytes < BULK_SLOW_RECOVERY_RESET_PARTIAL_MAX_BYTES,
    }
}

fn bulk_slow_recovery_threshold(profile: DownloadPerformanceProfile) -> u64 {
    if profile.max_segments >= 12 {
        BULK_SLOW_RECOVERY_FAST_THRESHOLD
    } else {
        BULK_SLOW_RECOVERY_BALANCED_THRESHOLD
    }
}

pub(super) async fn retry_bulk_archive_creation(
    app: &AppHandle,
    state: &SharedState,
    archive_id: &str,
) -> Result<(), String> {
    let archive = state.bulk_archive_ready_for_retry(archive_id).await?;
    create_bulk_archive_from_ready(app, state, archive, None).await
}

async fn create_bulk_archive_from_ready(
    app: &AppHandle,
    state: &SharedState,
    archive: BulkArchiveReady,
    diagnostic_job_id: Option<String>,
) -> Result<(), String> {
    let archive_id = archive.archive_id.clone();
    let plan = match bulk_finalization_plan(&archive) {
        Ok(plan) => plan,
        Err(error) => {
            let snapshot = state
                .mark_bulk_archive_status(
                    &archive_id,
                    BulkArchiveStatus::Failed,
                    None,
                    Some(archive.output_path.display().to_string()),
                    Some(error.clone()),
                    None,
                    None,
                    None,
                    None,
                )
                .await?;
            emit_snapshot(app, &snapshot);
            return Err(error);
        }
    };
    let archive = BulkArchiveReady {
        output_kind: plan.output_kind,
        ..archive
    };
    let archive_output_path = archive.output_path.display().to_string();
    let requires_extraction = plan.requires_extraction;
    let seven_zip_path = if requires_extraction {
        match crate::sidecars::resolve_seven_zip_binary_path() {
            Ok(path) => Some(path),
            Err(error) => {
                let snapshot = state
                    .mark_bulk_archive_status(
                        &archive_id,
                        BulkArchiveStatus::Failed,
                        Some(requires_extraction),
                        Some(archive_output_path.clone()),
                        Some(error.clone()),
                        None,
                        Some(plan.finalize_mode),
                        Some(plan.total_completed_bytes),
                        Some(0),
                    )
                    .await?;
                emit_snapshot(app, &snapshot);
                return Err(error);
            }
        }
    } else {
        None
    };
    let initial_status = if requires_extraction {
        BulkArchiveStatus::Extracting
    } else {
        BulkArchiveStatus::Combining
    };
    let snapshot = state
        .mark_bulk_archive_status(
            &archive_id,
            initial_status,
            Some(requires_extraction),
            Some(archive_output_path.clone()),
            None,
            plan.warning.clone(),
            Some(plan.finalize_mode),
            Some(plan.total_completed_bytes),
            Some(0),
        )
        .await?;
    emit_snapshot(app, &snapshot);

    let prepared = match prepare_bulk_archive_sources(archive, seven_zip_path).await {
        Ok(prepared) => prepared,
        Err(error) => {
            mark_bulk_archive_create_failed(
                app,
                state,
                &archive_id,
                archive_output_path,
                Some(requires_extraction),
                error.clone(),
                diagnostic_job_id,
            )
            .await?;
            return Err(error);
        }
    };

    if requires_extraction {
        let snapshot = state
            .mark_bulk_archive_status(
                &archive_id,
                BulkArchiveStatus::Combining,
                Some(requires_extraction),
                Some(archive_output_path.clone()),
                None,
                plan.warning.clone(),
                Some(plan.finalize_mode),
                Some(plan.total_completed_bytes),
                Some(0),
            )
            .await?;
        emit_snapshot(app, &snapshot);
    }

    match finish_prepared_bulk_archive(prepared).await {
        Ok(outcome) => {
            let cleanup_warning = cleanup_warning_message(&outcome.cleanup_warnings);
            if let Some(warning) = &cleanup_warning {
                let _ = state
                    .record_diagnostic_event(
                        DiagnosticLevel::Warning,
                        "bulk_archive",
                        warning.clone(),
                        diagnostic_job_id.clone(),
                    )
                    .await;
            }
            let snapshot = state
                .mark_bulk_archive_status(
                    &archive_id,
                    BulkArchiveStatus::Completed,
                    None,
                    Some(outcome.output_path.display().to_string()),
                    None,
                    cleanup_warning,
                    Some(plan.finalize_mode),
                    Some(plan.total_completed_bytes),
                    Some(plan.total_completed_bytes),
                )
                .await?;
            emit_snapshot(app, &snapshot);
            notify_bulk_archive_completed(app, state, &outcome.output_path).await;
            Ok(())
        }
        Err(error) => {
            mark_bulk_archive_create_failed(
                app,
                state,
                &archive_id,
                archive_output_path,
                Some(requires_extraction),
                error.clone(),
                diagnostic_job_id,
            )
            .await?;
            Err(error)
        }
    }
}

async fn mark_bulk_archive_create_failed(
    app: &AppHandle,
    state: &SharedState,
    archive_id: &str,
    archive_output_path: String,
    requires_extraction: Option<bool>,
    error: String,
    diagnostic_job_id: Option<String>,
) -> Result<(), String> {
    let _ = state
        .record_diagnostic_event(
            DiagnosticLevel::Error,
            "bulk_archive",
            format!("Bulk archive failed: {error}"),
            diagnostic_job_id,
        )
        .await;
    let snapshot = state
        .mark_bulk_archive_status(
            archive_id,
            BulkArchiveStatus::Failed,
            requires_extraction,
            Some(archive_output_path),
            Some(error.clone()),
            None,
            None,
            None,
            None,
        )
        .await?;
    emit_snapshot(app, &snapshot);
    eprintln!("failed to create bulk archive: {error}");
    Ok(())
}

fn cleanup_warning_message(warnings: &[String]) -> Option<String> {
    match warnings.len() {
        0 => None,
        1 => warnings.first().cloned(),
        count => Some(format!(
            "Bulk archive was created, but {count} downloaded archive parts could not be deleted."
        )),
    }
}
