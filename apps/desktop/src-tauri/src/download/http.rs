use super::*;
use url::Url;

const BULK_SLOW_RECOVERY_MIN_SIZE: u64 = 32 * 1024 * 1024;
const BULK_SLOW_RECOVERY_BALANCED_THRESHOLD: u64 = 64 * 1024;
const BULK_HOSTER_FAIRNESS_RELEASE_THRESHOLD: u64 = 64 * 1024;
const NORMAL_BALANCED_TOTAL_SEGMENT_CONNECTION_BUDGET: usize = 18;
const NORMAL_BALANCED_ORIGIN_SEGMENT_CONNECTION_BUDGET: usize = 8;
const SEGMENT_BUDGET_ADMISSION_DEFER: Duration = Duration::from_secs(2);
const SEGMENT_CONNECTION_LEASE_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BulkSlowStreamRecoveryAction {
    Continue,
    Retry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum SegmentConnectionClass {
    Normal,
    DirectBulk,
    ProtectedHosterBulk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SegmentConnectionBudget {
    pub(super) total: usize,
    pub(super) per_origin: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentConnectionLease {
    class: SegmentConnectionClass,
    origin: String,
    segments: usize,
    leased_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HosterWarmupEntry {
    InFlight { started_at: Instant },
    Ready { url: String, expires_at: Instant },
    Failed { expires_at: Instant },
}

#[derive(Debug)]
pub(super) struct SegmentConnectionLeaseGuard {
    job_id: String,
}

#[derive(Debug)]
pub(super) struct SegmentConnectionLeaseController {
    guard: SegmentConnectionLeaseGuard,
    class: SegmentConnectionClass,
    effective_url: String,
    budget: SegmentConnectionBudget,
    adaptive_cap: usize,
    fair_min_segments: usize,
    fair_origin_workers: Option<usize>,
}

impl SegmentConnectionLeaseController {
    pub(super) fn current_segments(&self) -> usize {
        segment_connection_leases()
            .lock()
            .ok()
            .and_then(|leases| leases.get(&self.guard.job_id).map(|lease| lease.segments))
            .unwrap_or(0)
    }

    pub(super) fn grow_to(&self, desired_segments: usize) -> usize {
        if desired_segments < 2 {
            return self.current_segments();
        }

        let now = Instant::now();
        let desired_cap = desired_segments.min(self.adaptive_cap);
        let leases = segment_connection_leases();
        let Ok(mut leases) = leases.lock() else {
            return self.current_segments();
        };
        SegmentBudgetBroker::expire_stale_leases(&mut leases, now);
        let current_segments = leases
            .get(&self.guard.job_id)
            .map(|lease| lease.segments)
            .unwrap_or(0);
        let Some(available_segments) = segment_budget_from_leases_locked(
            &leases,
            SegmentBudgetRequest {
                class: self.class,
                job_id: &self.guard.job_id,
                effective_url: &self.effective_url,
                budget: self.budget,
                policy_cap: desired_cap,
                fair_min_segments: self.fair_min_segments,
                fair_origin_workers: self.fair_origin_workers,
            },
        ) else {
            return current_segments;
        };
        let next_segments = current_segments.max(available_segments.min(desired_cap));
        if let Some(lease) = leases.get_mut(&self.guard.job_id) {
            lease.segments = next_segments;
            lease.leased_at = now;
        }
        next_segments
    }

    pub(super) fn shrink_to(&self, desired_segments: usize) -> usize {
        if desired_segments < 2 {
            return self.current_segments();
        }

        let now = Instant::now();
        let desired_cap = desired_segments.min(self.adaptive_cap).max(1);
        let leases = segment_connection_leases();
        let Ok(mut leases) = leases.lock() else {
            return self.current_segments();
        };
        let Some(lease) = leases.get_mut(&self.guard.job_id) else {
            return 0;
        };
        lease.segments = lease.segments.min(desired_cap);
        lease.leased_at = now;
        lease.segments
    }
}

struct SegmentBudgetBroker;

struct SegmentBudgetRequest<'a> {
    class: SegmentConnectionClass,
    job_id: &'a str,
    effective_url: &'a str,
    budget: SegmentConnectionBudget,
    policy_cap: usize,
    fair_min_segments: usize,
    fair_origin_workers: Option<usize>,
}

impl SegmentBudgetBroker {
    fn reserve_segments(
        task: &crate::state::DownloadTask,
        effective_url: &str,
        class: SegmentConnectionClass,
        budget: SegmentConnectionBudget,
        policy_cap: usize,
        fair_min_segments: usize,
        fair_origin_workers: Option<usize>,
    ) -> Option<(usize, SegmentConnectionLeaseGuard, bool)> {
        let now = Instant::now();
        let leases = segment_connection_leases();
        let mut leases = leases
            .lock()
            .expect("segment connection lease registry should not be poisoned");
        Self::expire_stale_leases(&mut leases, now);

        let target_origin = segment_connection_origin_key(effective_url)
            .unwrap_or_else(|| effective_url.to_string());
        let secondary_hoster_worker = class == SegmentConnectionClass::ProtectedHosterBulk
            && fair_origin_workers.is_some()
            && leases.iter().any(|(lease_job_id, lease)| {
                lease_job_id != &task.id && lease.class == class && lease.origin == target_origin
            });
        let segment_budget = segment_budget_from_leases_locked(
            &leases,
            SegmentBudgetRequest {
                class,
                job_id: &task.id,
                effective_url,
                budget,
                policy_cap,
                fair_min_segments,
                fair_origin_workers,
            },
        )?;
        leases.insert(
            task.id.clone(),
            SegmentConnectionLease {
                class,
                origin: target_origin,
                segments: segment_budget,
                leased_at: now,
            },
        );

        Some((
            segment_budget,
            SegmentConnectionLeaseGuard {
                job_id: task.id.clone(),
            },
            secondary_hoster_worker,
        ))
    }

    fn expire_stale_leases(leases: &mut HashMap<String, SegmentConnectionLease>, now: Instant) {
        leases.retain(|_, lease| {
            now.saturating_duration_since(lease.leased_at) < SEGMENT_CONNECTION_LEASE_TTL
        });
    }
}

static SEGMENT_CONNECTION_LEASES: OnceLock<StdMutex<HashMap<String, SegmentConnectionLease>>> =
    OnceLock::new();
static HOSTER_WARMUP_CACHE: OnceLock<StdMutex<HashMap<String, HosterWarmupEntry>>> =
    OnceLock::new();
const HOSTER_WARMUP_TTL: Duration = Duration::from_secs(5 * 60);
pub(super) const HOSTER_WARMUP_INFLIGHT_TTL: Duration = Duration::from_secs(2 * 60);
const HOSTER_WARMUP_FAILURE_BACKOFF: Duration = Duration::from_secs(30);

impl Drop for SegmentConnectionLeaseGuard {
    fn drop(&mut self) {
        if let Some(leases) = SEGMENT_CONNECTION_LEASES.get() {
            if let Ok(mut leases) = leases.lock() {
                leases.remove(&self.job_id);
            }
        }
    }
}

pub(super) async fn run_http_download_attempt<A: DownloadUi>(
    app: &A,
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
                        if error.category == FailureCategory::Resume
                            && metadata_len(&task.temp_path).await.unwrap_or(0) > 0
                        {
                            return Err(segmented_resume_metadata_required_error());
                        }
                        current_url = url;
                    }
                    Ok(None) | Err(_) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    }
}

async fn run_http_download_attempt_for_url<A: DownloadUi>(
    app: &A,
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
    let segment_attempt = segment_attempt_context_for_task(state, task, effective_url).await;
    let segment_pressure_key = segment_attempt
        .as_ref()
        .map(|attempt| segment_pressure_key_for_task(task, effective_url, attempt));
    let profile = segment_attempt
        .as_ref()
        .map(|attempt| {
            profile_for_segment_admission_at(
                effective_url,
                segment_pressure_key.as_deref(),
                attempt.admission,
                Instant::now(),
            )
        })
        .unwrap_or_else(|| {
            profile_for_effective_http_url_with_pressure_key_at(
                effective_url,
                segment_pressure_key.as_deref(),
                Instant::now(),
            )
        });
    let segmented_client = client.clone();
    record_download_diagnostic(
        state,
        DiagnosticLevel::Info,
        task,
        format!(
            "HTTP transfer policy general selected (initial segments: {}, soft cap: {}, max segments: {}, target segment size: {} MiB).",
            profile.initial_segments,
            profile.soft_max_segments,
            profile.max_segments,
            profile.target_segment_size / (1024 * 1024)
        ),
    )
    .await;
    let mut preflight_metadata =
        preflight_download(&client, effective_url, request_auth.as_ref()).await;

    let has_segment_state = segment_meta_path(&task.temp_path).exists();
    if existing_bytes > 0
        && !has_segment_state
        && segment_attempt.is_some()
        && preflight_metadata
            .as_ref()
            .and_then(|metadata| metadata.total_bytes)
            .is_some_and(|total_bytes| {
                existing_bytes >= total_bytes && total_bytes >= profile.min_segmented_size
            })
    {
        return Err(segmented_resume_metadata_required_error());
    }
    let can_try_segmented = segment_attempt.is_some()
        && (existing_bytes == 0 || has_segment_state)
        && profile.max_segments >= 2
        && !segment_attempt
            .as_ref()
            .is_some_and(|attempt| attempt.is_backed_off(effective_url, Instant::now()));

    let mut range_probe_full_response = None;

    if can_try_segmented {
        let segment_attempt = segment_attempt
            .as_ref()
            .expect("segment attempt context exists when segmented download can start");
        let probe_response =
            probe_range_metadata_response(&segmented_client, effective_url, request_auth.as_ref())
                .await;
        let mut range_probe_supported = false;
        let mut probed_transport_label = "unknown";
        match probe_response {
            RangeProbeOutcome::PartialContent(metadata, segmented_transport_label) => {
                range_probe_supported = true;
                probed_transport_label = segmented_transport_label;
                preflight_metadata = Some(merge_preflight_metadata(preflight_metadata, metadata));
            }
            RangeProbeOutcome::FullResponse(response) if existing_bytes == 0 => {
                segment_attempt.record_rejection(effective_url, Instant::now());
                record_download_diagnostic(
                    state,
                    DiagnosticLevel::Info,
                    task,
                    "Range probe was ignored by the server; reusing the open response as the single-stream download."
                        .into(),
                )
                .await;
                range_probe_full_response = Some(response);
            }
            RangeProbeOutcome::FullResponse(_) | RangeProbeOutcome::Unsupported => {
                segment_attempt.record_rejection(effective_url, Instant::now());
                record_download_diagnostic(
                    state,
                    DiagnosticLevel::Warning,
                    task,
                    "Range probe failed; using single-stream fallback for this hoster link.".into(),
                )
                .await;
                if has_segment_state && metadata_len(&task.temp_path).await.unwrap_or(0) > 0 {
                    if let Some(outcome) = try_low_cap_segmented_recovery_after_probe_failure(
                        app,
                        state,
                        task,
                        &segmented_client,
                        effective_url,
                        request_auth.clone(),
                        segment_attempt,
                        profile,
                        segment_pressure_key
                            .clone()
                            .unwrap_or_else(|| segment_pressure_fallback_key(effective_url)),
                    )
                    .await?
                    {
                        return Ok(outcome);
                    }
                }
            }
        }

        if range_probe_supported {
            if let Some(metadata) = preflight_metadata.as_ref() {
                if let Some(total_bytes) = metadata.total_bytes {
                    if let Some((plan, segment_lease, secondary_hoster_worker)) =
                        reserve_segmented_plan_for_attempt(
                            task,
                            effective_url,
                            segment_attempt,
                            total_bytes,
                            metadata.resume_support,
                            speed_limit,
                            profile,
                        )
                    {
                        if secondary_hoster_worker {
                            record_download_diagnostic(
                                state,
                                DiagnosticLevel::Info,
                                task,
                                format!(
                                    "DataNodes priority capped secondary segmented worker to {} segments.",
                                    plan.segments.len()
                                ),
                            )
                            .await;
                        }
                        let planned_segment_count = plan_segmented_ranges_with_budget(
                            total_bytes,
                            metadata.resume_support,
                            speed_limit,
                            profile,
                            (segment_attempt.adaptive_cap != usize::MAX)
                                .then_some(segment_attempt.adaptive_cap),
                        )
                        .map(|planned| planned.segments.len())
                        .unwrap_or(plan.segments.len());
                        let adaptive_segment_cap =
                            segment_attempt.adaptive_cap.min(profile.max_segments);
                        record_download_diagnostic(
                            state,
                            DiagnosticLevel::Info,
                            task,
                            format!(
                                "Starting segmented HTTP download: planned segments {}, admitted segments {}, adaptive max {}, transport {}.",
                                planned_segment_count,
                                plan.segments.len(),
                                adaptive_segment_cap,
                                probed_transport_label
                            ),
                        )
                        .await;
                        match run_segmented_download_attempt(
                            app,
                            state,
                            task,
                            segmented_client.clone(),
                            effective_url.to_string(),
                            request_auth.clone(),
                            plan,
                            profile,
                            metadata.validators.clone(),
                            segment_lease,
                            segment_pressure_key
                                .clone()
                                .unwrap_or_else(|| segment_pressure_fallback_key(effective_url)),
                        )
                        .await
                        {
                            Ok(outcome) => return Ok(outcome),
                            Err(error) if segmented_error_allows_single_stream_fallback(&error) => {
                                if has_segment_state
                                    || metadata_len(&task.temp_path).await.unwrap_or(0) > 0
                                {
                                    return Err(segmented_resume_metadata_required_error());
                                }
                                segment_attempt.record_rejection(effective_url, Instant::now());
                                cleanup_partial_artifacts(&task.temp_path).await;
                                existing_bytes = 0;
                            }
                            Err(error) => return Err(error),
                        }
                    } else if segmented_plan_would_fit_without_active_budget(
                        total_bytes,
                        metadata.resume_support,
                        speed_limit,
                        profile,
                        segment_attempt.initial_cap,
                    ) {
                        match segment_attempt
                            .admission
                            .segment_budget_wait_action(has_segment_state)
                        {
                            SegmentBudgetWaitAction::Defer => {
                                let requested_chunks =
                                    segment_attempt.adaptive_cap.min(profile.max_segments);
                                let host_key = segment_attempt
                                    .backoff_key
                                    .clone()
                                    .or_else(|| segment_connection_origin_key(effective_url))
                                    .unwrap_or_else(|| effective_url.to_string());
                                let reason = format!(
                                    "Segment connection budget is full for {} admission; host key: {host_key}; class: {:?}; requested chunks: {}; retrying after {} seconds without occupying a worker slot.",
                                    segment_attempt.admission.label(),
                                    segment_attempt.connection_class,
                                    requested_chunks,
                                    SEGMENT_BUDGET_ADMISSION_DEFER.as_secs()
                                );
                                record_download_diagnostic(
                                    state,
                                    DiagnosticLevel::Warning,
                                    task,
                                    reason.clone(),
                                )
                                .await;
                                let snapshot = state
                                    .defer_active_job(
                                        &task.id,
                                        reason,
                                        SEGMENT_BUDGET_ADMISSION_DEFER,
                                    )
                                    .await?;
                                emit_download_update(app, &snapshot, &task.id);
                                return Ok(DownloadOutcome::Deferred(
                                    SEGMENT_BUDGET_ADMISSION_DEFER,
                                ));
                            }
                        }
                    }
                }
            }
        }

        if has_segment_state {
            if metadata_len(&task.temp_path).await.unwrap_or(0) > 0 {
                return Err(segmented_resume_metadata_required_error());
            }
            record_download_diagnostic(
                state,
                DiagnosticLevel::Warning,
                task,
                "Cleaning incompatible segmented state before single-stream fallback.".into(),
            )
            .await;
            cleanup_partial_artifacts(&task.temp_path).await;
            existing_bytes = 0;
        }
    } else if has_segment_state {
        if metadata_len(&task.temp_path).await.unwrap_or(0) > 0 {
            return Err(segmented_resume_metadata_required_error());
        }
        record_download_diagnostic(
            state,
            DiagnosticLevel::Warning,
            task,
            "Cleaning segmented state because this attempt cannot use segmented downloading."
                .into(),
        )
        .await;
        cleanup_partial_artifacts(&task.temp_path).await;
        existing_bytes = 0;
    } else if segment_attempt.is_some() {
        let fallback_reason = if existing_bytes > 0 && !has_segment_state {
            "partial file has no segmented metadata"
        } else if profile.max_segments < 2 {
            "selected policy uses a single stream"
        } else if segment_attempt
            .as_ref()
            .is_some_and(|attempt| attempt.is_backed_off(effective_url, Instant::now()))
        {
            "range requests are temporarily backed off"
        } else {
            "attempt is not eligible"
        };
        record_download_diagnostic(
            state,
            DiagnosticLevel::Info,
            task,
            format!("Skipping segmented HTTP download: {fallback_reason}."),
        )
        .await;
    }

    let response = match range_probe_full_response {
        Some(response) => response,
        None => {
            send_request(
                &client,
                effective_url,
                existing_bytes,
                request_auth.as_ref(),
                preflight_metadata
                    .as_ref()
                    .map(|metadata| &metadata.validators),
            )
            .await?
        }
    };
    reject_hoster_html_response(task, &response)?;
    let supports_resume = response.status() == StatusCode::PARTIAL_CONTENT;
    ensure_single_stream_resume_supported(&task.temp_path, existing_bytes, supports_resume)?;

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
    let stall_timeout = protected_bulk_hoster_stall_timeout(task, profile);
    let mut downloaded_bytes = existing_bytes;
    let attempt_started = Instant::now();
    let mut attempt_transferred_bytes = 0_u64;
    let mut sample_bytes = 0_u64;
    let mut sample_started = Instant::now();
    let mut displayed_speed = RollingSpeed::with_alpha(profile.speed_smoothing_alpha);
    let mut low_speed_monitor = LowSpeedMonitor::new(profile);
    let mut low_speed_bytes = 0_u64;
    let mut low_speed_started = Instant::now();
    let mut priority_throttle_limited = false;
    let mut last_emitted_bytes = existing_bytes;
    let mut last_persisted_at = Instant::now();
    let priority_throttle = Mutex::new(DynamicThrottleState::default());
    let control_signal = WorkerControlSignal::default();
    let _control_poller =
        WorkerControlPoller::spawn(state.clone(), task.id.clone(), control_signal.clone());
    let mut stream_controller = SignalStreamController::new(control_signal.clone(), stall_timeout);

    loop {
        let chunk_result = match stream_controller.next(stream.next()).await {
            StreamItemWait::Item(result) => result,
            StreamItemWait::Interrupted(DownloadOutcome::Paused) => {
                file.flush().await.ok();
                return Ok(DownloadOutcome::Paused);
            }
            StreamItemWait::Interrupted(DownloadOutcome::Canceled) => {
                file.flush().await.ok();
                return Ok(DownloadOutcome::Canceled);
            }
            StreamItemWait::Interrupted(DownloadOutcome::Completed) => unreachable!(
                "stream control cannot produce a completed outcome while waiting for chunks"
            ),
            StreamItemWait::Interrupted(DownloadOutcome::Deferred(_)) => unreachable!(
                "stream control cannot produce a deferred outcome while waiting for chunks"
            ),
            StreamItemWait::Stalled => {
                let timeout = stall_timeout.expect("stall wait can only stall when configured");
                file.flush().await.ok();
                record_download_diagnostic(
                    state,
                    DiagnosticLevel::Warning,
                    task,
                    format!(
                        "Protected hoster stream received no data for {} seconds; retrying.",
                        timeout.as_secs()
                    ),
                )
                .await;
                return Err(bulk_hoster_stall_error(timeout));
            }
        };
        let Some(chunk_result) = chunk_result else {
            break;
        };

        match control_signal.current_outcome() {
            Some(DownloadOutcome::Paused) => {
                file.flush().await.ok();
                return Ok(DownloadOutcome::Paused);
            }
            Some(DownloadOutcome::Canceled) => {
                file.flush().await.ok();
                return Ok(DownloadOutcome::Canceled);
            }
            Some(DownloadOutcome::Completed) | Some(DownloadOutcome::Deferred(_)) | None => {}
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
        let priority_throttle_enabled = task.is_bulk_member && task.resolved_from_url.is_some();
        if priority_throttle_enabled {
            if let Some(decision) = state.hoster_priority_throttle_decision(&task.id).await {
                priority_throttle_limited = true;
                match throttle_download_with_dynamic_limit(
                    state,
                    &task.id,
                    &priority_throttle,
                    decision.cap_bytes_per_second,
                    chunk_len,
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
            } else {
                clear_dynamic_throttle(&priority_throttle).await;
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
                speed_limit.or(priority_throttle_limited.then_some(1)),
                profile,
                bulk_slow_recovery_state,
            ) {
                BulkSlowStreamRecoveryAction::Retry => {
                    file.flush().await.ok();
                    drop(file);
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
                        speed_limit.is_some() || priority_throttle_limited,
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
            priority_throttle_limited = false;
        }

        if elapsed >= PROGRESS_UPDATE_INTERVAL {
            let speed = displayed_speed.record_sample(sample_bytes, elapsed);
            let should_persist = last_persisted_at.elapsed() >= PROGRESS_PERSIST_INTERVAL;
            let delta = state
                .update_job_progress_delta(
                    &task.id,
                    downloaded_bytes,
                    total_bytes,
                    speed,
                    should_persist,
                )
                .await?;
            emit_progress_delta(app, delta);
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
        let delta = state
            .update_job_progress_delta(&task.id, downloaded_bytes, total_bytes, 0, should_persist)
            .await?;
        emit_progress_delta(app, delta);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SegmentBudgetWaitAction {
    Defer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadAdmissionKind {
    Normal,
    DirectBulk,
    ProtectedHosterBulk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DownloadAdmission {
    kind: DownloadAdmissionKind,
}

impl DownloadAdmission {
    pub(super) fn normal() -> Self {
        Self {
            kind: DownloadAdmissionKind::Normal,
        }
    }

    pub(super) fn direct_bulk() -> Self {
        Self {
            kind: DownloadAdmissionKind::DirectBulk,
        }
    }

    pub(super) fn protected_hoster_bulk() -> Self {
        Self {
            kind: DownloadAdmissionKind::ProtectedHosterBulk,
        }
    }

    fn label(self) -> &'static str {
        match self.kind {
            DownloadAdmissionKind::Normal => "normal",
            DownloadAdmissionKind::DirectBulk => "direct bulk",
            DownloadAdmissionKind::ProtectedHosterBulk => "protected hoster bulk",
        }
    }

    pub(super) fn segment_budget_wait_action(
        self,
        _has_segment_state: bool,
    ) -> SegmentBudgetWaitAction {
        SegmentBudgetWaitAction::Defer
    }
}

pub(super) fn profile_for_segment_admission_at(
    effective_url: &str,
    pressure_key: Option<&str>,
    _admission: DownloadAdmission,
    now: Instant,
) -> DownloadPerformanceProfile {
    profile_for_effective_http_url_with_pressure_key_at(effective_url, pressure_key, now)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentAttemptContext {
    admission: DownloadAdmission,
    backoff_key: Option<String>,
    connection_class: Option<SegmentConnectionClass>,
    connection_budget: Option<SegmentConnectionBudget>,
    initial_cap: usize,
    adaptive_cap: usize,
    fair_min_segments: usize,
    fair_origin_workers: Option<usize>,
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

fn segment_pressure_key_for_task(
    task: &crate::state::DownloadTask,
    effective_url: &str,
    attempt: &SegmentAttemptContext,
) -> String {
    if let Some(key) = attempt.backoff_key.as_deref() {
        return key.to_string();
    }
    if let Some(key) = task
        .resolved_from_url
        .as_deref()
        .and_then(segment_connection_origin_key)
    {
        return key;
    }
    if let Some(source) = task.source.as_ref() {
        if let Some(key) = source
            .page_url
            .as_deref()
            .and_then(segment_connection_origin_key)
            .or_else(|| {
                source
                    .referrer
                    .as_deref()
                    .and_then(segment_connection_origin_key)
            })
        {
            return key;
        }
    }

    segment_pressure_fallback_key(effective_url)
}

fn segment_pressure_fallback_key(effective_url: &str) -> String {
    segment_connection_origin_key(effective_url).unwrap_or_else(|| effective_url.to_string())
}

async fn segment_attempt_context_for_task(
    state: &SharedState,
    task: &crate::state::DownloadTask,
    effective_url: &str,
) -> Option<SegmentAttemptContext> {
    if task.is_bulk_member && task.resolved_from_url.is_none() {
        return Some(SegmentAttemptContext {
            admission: DownloadAdmission::direct_bulk(),
            backoff_key: None,
            connection_class: Some(SegmentConnectionClass::DirectBulk),
            connection_budget: direct_bulk_segment_budget(),
            initial_cap: usize::MAX,
            adaptive_cap: usize::MAX,
            fair_min_segments: 2,
            fair_origin_workers: None,
        });
    }

    if task.is_bulk_member && task.resolved_from_url.is_some() {
        if state.bulk_hoster_acceleration_mode().await == BulkHosterAccelerationMode::Off {
            return None;
        }
        let source_url = task.resolved_from_url.as_deref()?;
        let policy = crate::hosters::hoster_acceleration_policy(source_url, effective_url)?;
        let initial_cap = hoster_initial_segment_cap(&policy);
        let adaptive_cap = hoster_adaptive_segment_cap(&policy);
        let connection_budget = hoster_segment_budget()?;
        if initial_cap < 2 || adaptive_cap < 2 {
            return None;
        }
        return Some(SegmentAttemptContext {
            admission: DownloadAdmission::protected_hoster_bulk(),
            backoff_key: Some(policy.backoff_key),
            connection_class: Some(SegmentConnectionClass::ProtectedHosterBulk),
            connection_budget: Some(connection_budget),
            initial_cap,
            adaptive_cap,
            fair_min_segments: 2,
            fair_origin_workers: accelerated_hoster_fair_origin_workers(),
        });
    }

    Some(SegmentAttemptContext {
        admission: DownloadAdmission::normal(),
        backoff_key: None,
        connection_class: Some(SegmentConnectionClass::Normal),
        connection_budget: normal_segment_budget(),
        initial_cap: usize::MAX,
        adaptive_cap: usize::MAX,
        fair_min_segments: 2,
        fair_origin_workers: None,
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

pub(super) fn hoster_initial_segment_cap(
    policy: &crate::hosters::HosterAccelerationPolicy,
) -> usize {
    policy.balanced_initial_segments
}

pub(super) fn hoster_adaptive_segment_cap(
    policy: &crate::hosters::HosterAccelerationPolicy,
) -> usize {
    policy.balanced_max_segments
}

pub(super) fn segmented_transport_label_for_version(version: reqwest::Version) -> &'static str {
    match version {
        reqwest::Version::HTTP_09 => "http/0.9",
        reqwest::Version::HTTP_10 => "http/1.0",
        reqwest::Version::HTTP_11 => "http/1.1",
        reqwest::Version::HTTP_2 => "h2",
        reqwest::Version::HTTP_3 => "h3",
        _ => "unknown",
    }
}

pub(super) fn hoster_segment_budget() -> Option<SegmentConnectionBudget> {
    Some(SegmentConnectionBudget {
        total: HOSTER_BULK_BALANCED_TOTAL_SEGMENT_CONNECTION_BUDGET,
        per_origin: HOSTER_BULK_BALANCED_ORIGIN_SEGMENT_CONNECTION_BUDGET,
    })
}

pub(super) fn normal_segment_budget() -> Option<SegmentConnectionBudget> {
    Some(SegmentConnectionBudget {
        total: NORMAL_BALANCED_TOTAL_SEGMENT_CONNECTION_BUDGET,
        per_origin: NORMAL_BALANCED_ORIGIN_SEGMENT_CONNECTION_BUDGET,
    })
}

pub(super) fn direct_bulk_segment_budget() -> Option<SegmentConnectionBudget> {
    normal_segment_budget()
}

fn reserve_segmented_plan_for_attempt(
    task: &crate::state::DownloadTask,
    effective_url: &str,
    attempt: &SegmentAttemptContext,
    total_bytes: u64,
    resume_support: ResumeSupport,
    speed_limit: Option<u64>,
    profile: DownloadPerformanceProfile,
) -> Option<(RangePlan, Option<SegmentConnectionLeaseController>, bool)> {
    let Some(class) = attempt.connection_class else {
        return plan_segmented_ranges_with_budget(
            total_bytes,
            resume_support,
            speed_limit,
            profile,
            None,
        )
        .map(|plan| (plan, None, false));
    };

    let budget = attempt.connection_budget?;
    let (segment_budget, lease, secondary_hoster_worker) = SegmentBudgetBroker::reserve_segments(
        task,
        effective_url,
        class,
        budget,
        attempt.initial_cap,
        attempt.fair_min_segments,
        attempt.fair_origin_workers,
    )?;
    let plan = plan_segmented_ranges_with_budget(
        total_bytes,
        resume_support,
        speed_limit,
        profile,
        Some(segment_budget),
    )?;
    if let Ok(mut leases) = segment_connection_leases().lock() {
        if let Some(lease) = leases.get_mut(&task.id) {
            lease.segments = plan.segments.len();
        }
    }

    Some((
        plan,
        Some(SegmentConnectionLeaseController {
            guard: lease,
            class,
            effective_url: effective_url.to_string(),
            budget,
            adaptive_cap: attempt.adaptive_cap,
            fair_min_segments: attempt.fair_min_segments,
            fair_origin_workers: attempt.fair_origin_workers,
        }),
        secondary_hoster_worker,
    ))
}

#[allow(clippy::too_many_arguments)]
async fn try_low_cap_segmented_recovery_after_probe_failure<A: DownloadUi>(
    app: &A,
    state: &SharedState,
    task: &crate::state::DownloadTask,
    segmented_client: &Client,
    effective_url: &str,
    request_auth: Option<HandoffAuth>,
    attempt: &SegmentAttemptContext,
    profile: DownloadPerformanceProfile,
    segment_pressure_key: String,
) -> Result<Option<DownloadOutcome>, DownloadError> {
    let Some(existing_state) = load_existing_segment_state(&task.temp_path).await? else {
        return Ok(None);
    };
    if existing_state.total_bytes == 0 || existing_state.segments.is_empty() {
        return Ok(None);
    }

    let recovery_profile = low_cap_segmented_recovery_profile(profile);
    let Some((plan, segment_lease, _secondary_hoster_worker)) = reserve_segmented_plan_for_attempt(
        task,
        effective_url,
        attempt,
        existing_state.total_bytes,
        ResumeSupport::Supported,
        None,
        recovery_profile,
    ) else {
        return Ok(None);
    };

    record_download_diagnostic(
        state,
        DiagnosticLevel::Warning,
        task,
        format!(
            "Range probe failed with segmented partial state; attempting low-cap segmented recovery with {} workers.",
            plan.segments.len()
        ),
    )
    .await;

    match run_segmented_download_attempt(
        app,
        state,
        task,
        segmented_client.clone(),
        effective_url.to_string(),
        request_auth,
        plan,
        recovery_profile,
        existing_state.validators,
        segment_lease,
        segment_pressure_key,
    )
    .await
    {
        Ok(outcome) => Ok(Some(outcome)),
        Err(error) if segmented_error_allows_single_stream_fallback(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn low_cap_segmented_recovery_profile(
    mut profile: DownloadPerformanceProfile,
) -> DownloadPerformanceProfile {
    profile.initial_segments = profile.initial_segments.clamp(2, 8);
    profile.soft_max_segments = profile.soft_max_segments.min(profile.initial_segments);
    profile.max_segments = profile.max_segments.min(profile.initial_segments);
    profile.adaptive_ramp_step = 0;
    profile
}

#[cfg(test)]
pub(super) fn segment_budget_from_test_leases(
    class: SegmentConnectionClass,
    job_id: &str,
    effective_url: &str,
    budget: SegmentConnectionBudget,
    policy_cap: usize,
    leases: &[(&str, SegmentConnectionClass, &str, usize)],
) -> Option<usize> {
    let leases = leases
        .iter()
        .map(|(lease_job_id, lease_class, lease_url, segments)| {
            let origin = segment_connection_origin_key(lease_url)
                .unwrap_or_else(|| (*lease_url).to_string());
            (
                (*lease_job_id).to_string(),
                SegmentConnectionLease {
                    class: *lease_class,
                    origin,
                    segments: *segments,
                    leased_at: Instant::now(),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    segment_budget_from_leases_locked(
        &leases,
        SegmentBudgetRequest {
            class,
            job_id,
            effective_url,
            budget,
            policy_cap,
            fair_min_segments: 2,
            fair_origin_workers: datanodes_fair_origin_workers_for_budget(budget, policy_cap),
        },
    )
}

fn segmented_plan_would_fit_without_active_budget(
    total_bytes: u64,
    resume_support: ResumeSupport,
    speed_limit: Option<u64>,
    profile: DownloadPerformanceProfile,
    policy_cap: usize,
) -> bool {
    plan_segmented_ranges_with_budget(
        total_bytes,
        resume_support,
        speed_limit,
        profile,
        Some(policy_cap),
    )
    .is_some()
}

fn segment_budget_from_leases_locked(
    leases: &HashMap<String, SegmentConnectionLease>,
    request: SegmentBudgetRequest<'_>,
) -> Option<usize> {
    if request.policy_cap < 2 {
        return None;
    }

    let target_origin = segment_connection_origin_key(request.effective_url)
        .unwrap_or_else(|| request.effective_url.to_string());
    let mut used_total = 0_usize;
    let mut used_origin = 0_usize;
    let mut used_origin_workers = 0_usize;

    for (lease_job_id, lease) in leases {
        if lease_job_id == request.job_id || lease.class != request.class {
            continue;
        }
        used_total = used_total.saturating_add(lease.segments);
        if lease.origin == target_origin {
            used_origin = used_origin.saturating_add(lease.segments);
            used_origin_workers = used_origin_workers.saturating_add(1);
        }
    }

    let available_total = request.budget.total.saturating_sub(used_total);
    let available_origin = request.budget.per_origin.saturating_sub(used_origin);
    let mut segment_budget = request
        .policy_cap
        .min(available_total)
        .min(available_origin);
    if request.class == SegmentConnectionClass::ProtectedHosterBulk {
        if let Some(fair_origin_workers) = request.fair_origin_workers {
            let active_origin_workers = used_origin_workers.saturating_add(1);
            let future_origin_workers = fair_origin_workers.saturating_sub(active_origin_workers);
            let reserved_for_future =
                future_origin_workers.saturating_mul(request.fair_min_segments.max(1));
            segment_budget =
                segment_budget.min(available_origin.saturating_sub(reserved_for_future));
            if used_origin_workers > 0 {
                segment_budget = segment_budget.min(request.fair_min_segments.max(1));
            }
        }
    }
    (segment_budget >= 2).then_some(segment_budget)
}

#[cfg(test)]
fn datanodes_fair_origin_workers_for_budget(
    budget: SegmentConnectionBudget,
    policy_cap: usize,
) -> Option<usize> {
    match (budget.total, budget.per_origin, policy_cap) {
        (
            HOSTER_BULK_BALANCED_TOTAL_SEGMENT_CONNECTION_BUDGET,
            HOSTER_BULK_BALANCED_ORIGIN_SEGMENT_CONNECTION_BUDGET,
            4,
        ) => Some(4),
        _ => None,
    }
}

fn segment_connection_origin_key(raw_url: &str) -> Option<String> {
    let parsed = Url::parse(raw_url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    Some(format!(
        "{}://{}:{}",
        parsed.scheme(),
        host,
        parsed.port_or_known_default().unwrap_or(0)
    ))
}

fn segment_connection_leases() -> &'static StdMutex<HashMap<String, SegmentConnectionLease>> {
    SEGMENT_CONNECTION_LEASES.get_or_init(|| StdMutex::new(HashMap::new()))
}

pub(super) fn spawn_datanodes_hoster_warmups<A: DownloadUi>(
    app: A,
    state: SharedState,
    candidates: Vec<crate::state::HosterWarmupCandidate>,
) {
    for candidate in candidates {
        let key = hoster_warmup_key(&candidate.job_id, &candidate.source_url);
        if !mark_hoster_warmup_inflight(&key, Instant::now()) {
            continue;
        }
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            match crate::hosters::refresh_resolved_hoster_link(&candidate.source_url).await {
                Ok(outcome) => {
                    store_warmed_hoster_url(&key, outcome.url, Instant::now() + HOSTER_WARMUP_TTL);
                }
                Err(_) => {
                    store_failed_hoster_warmup(
                        &key,
                        Instant::now() + HOSTER_WARMUP_FAILURE_BACKOFF,
                    );
                }
            }
            schedule_downloads(app.clone(), state.clone());
        });
    }
}

fn take_warmed_hoster_url(job_id: &str, source_url: &str, now: Instant) -> Option<String> {
    let key = hoster_warmup_key(job_id, source_url);
    let cache = hoster_warmup_cache();
    let mut cache = cache
        .lock()
        .expect("hoster warmup cache should not be poisoned");
    match cache.remove(&key) {
        Some(HosterWarmupEntry::Ready { url, expires_at }) if expires_at > now => Some(url),
        Some(HosterWarmupEntry::InFlight { started_at })
            if now.saturating_duration_since(started_at) < HOSTER_WARMUP_INFLIGHT_TTL =>
        {
            cache.insert(key, HosterWarmupEntry::InFlight { started_at });
            None
        }
        Some(HosterWarmupEntry::Failed { expires_at }) if expires_at > now => {
            cache.insert(key, HosterWarmupEntry::Failed { expires_at });
            None
        }
        _ => None,
    }
}

fn mark_hoster_warmup_inflight(key: &str, now: Instant) -> bool {
    let cache = hoster_warmup_cache();
    let mut cache = cache
        .lock()
        .expect("hoster warmup cache should not be poisoned");
    match cache.get(key) {
        Some(HosterWarmupEntry::InFlight { started_at })
            if now.saturating_duration_since(*started_at) < HOSTER_WARMUP_INFLIGHT_TTL =>
        {
            false
        }
        Some(HosterWarmupEntry::Ready { expires_at, .. }) if *expires_at > now => false,
        Some(HosterWarmupEntry::Failed { expires_at }) if *expires_at > now => false,
        _ => {
            cache.insert(
                key.to_string(),
                HosterWarmupEntry::InFlight { started_at: now },
            );
            true
        }
    }
}

fn store_warmed_hoster_url(key: &str, url: String, expires_at: Instant) {
    hoster_warmup_cache()
        .lock()
        .expect("hoster warmup cache should not be poisoned")
        .insert(
            key.to_string(),
            HosterWarmupEntry::Ready { url, expires_at },
        );
}

fn store_failed_hoster_warmup(key: &str, expires_at: Instant) {
    hoster_warmup_cache()
        .lock()
        .expect("hoster warmup cache should not be poisoned")
        .insert(key.to_string(), HosterWarmupEntry::Failed { expires_at });
}

fn hoster_warmup_cache() -> &'static StdMutex<HashMap<String, HosterWarmupEntry>> {
    HOSTER_WARMUP_CACHE.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn hoster_warmup_key(job_id: &str, source_url: &str) -> String {
    format!("{job_id}\n{}", source_url.trim())
}

#[cfg(test)]
pub(super) fn hoster_warmup_key_for_tests(job_id: &str, source_url: &str) -> String {
    hoster_warmup_key(job_id, source_url)
}

#[cfg(test)]
pub(super) fn mark_hoster_warmup_inflight_for_tests(key: &str, now: Instant) -> bool {
    mark_hoster_warmup_inflight(key, now)
}

#[cfg(test)]
pub(super) fn clear_hoster_warmup_cache_for_tests() {
    hoster_warmup_cache()
        .lock()
        .expect("hoster warmup cache should not be poisoned")
        .clear();
}

#[cfg(test)]
pub(super) fn put_hoster_warmup_for_tests(
    job_id: &str,
    source_url: &str,
    resolved_url: &str,
    expires_at: Instant,
) {
    let key = hoster_warmup_key(job_id, source_url);
    store_warmed_hoster_url(&key, resolved_url.to_string(), expires_at);
}

#[cfg(test)]
pub(super) fn take_warmed_hoster_url_for_tests(job_id: &str, source_url: &str) -> Option<String> {
    take_warmed_hoster_url(job_id, source_url, Instant::now())
}

pub(super) fn task_releases_bulk_hoster_fairness(
    task: &crate::state::DownloadTask,
    speed: u64,
) -> bool {
    task.is_bulk_member
        && task.resolved_from_url.is_some()
        && speed >= BULK_HOSTER_FAIRNESS_RELEASE_THRESHOLD
}

pub(super) fn protected_bulk_hoster_stall_timeout(
    task: &crate::state::DownloadTask,
    profile: DownloadPerformanceProfile,
) -> Option<Duration> {
    (task.is_bulk_member && task.resolved_from_url.is_some())
        .then_some(profile.bulk_hoster_stall_timeout)
}

pub(super) fn bulk_hoster_stall_error(timeout: Duration) -> DownloadError {
    download_error(
        FailureCategory::Network,
        format!(
            "Protected hoster stream received no data for {} seconds; retrying the stream.",
            timeout.as_secs()
        ),
        true,
    )
}

async fn record_download_diagnostic(
    state: &SharedState,
    level: DiagnosticLevel,
    task: &crate::state::DownloadTask,
    message: String,
) {
    let _ = state
        .record_diagnostic_event(level, "download", message, Some(task.id.clone()))
        .await;
}

async fn refresh_hoster_url_before_attempt(
    state: &SharedState,
    task: &crate::state::DownloadTask,
) -> Result<Option<String>, DownloadError> {
    if task.is_bulk_member && task.resolved_from_url.is_some() {
        if let Some(source_url) = task.resolved_from_url.as_deref() {
            if let Some(warmed_url) = take_warmed_hoster_url(&task.id, source_url, Instant::now()) {
                record_download_diagnostic(
                    state,
                    DiagnosticLevel::Info,
                    task,
                    "Using warmed DataNodes direct link.".into(),
                )
                .await;
                return Ok(Some(warmed_url));
            }
        }
        record_download_diagnostic(
            state,
            DiagnosticLevel::Info,
            task,
            "Resolving protected hoster direct link.".into(),
        )
        .await;
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
    let browser_handoff = task
        .source
        .as_ref()
        .is_some_and(|source| source.entry_point == "browser_download");
    if let Some(error) = unusable_download_response_error_for_parts(
        response.url().as_str(),
        response.headers(),
        browser_handoff,
        task.resolved_from_url.is_some(),
    ) {
        return Err(error);
    }

    Ok(())
}

pub(super) async fn complete_http_download<A: DownloadUi>(
    app: &A,
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
    notify_download_completed(app, state, final_path, task.is_bulk_member).await;
    Ok(())
}

pub(super) async fn handle_bulk_archive_after_completion<A: DownloadUi>(
    app: &A,
    state: &SharedState,
    job_id: &str,
) -> Result<(), String> {
    if let Some(archive) = state.bulk_archive_ready_for_job(job_id).await? {
        let _ = create_bulk_archive_from_ready(app, state, archive, Some(job_id.into())).await;
    }

    Ok(())
}

pub(super) fn ensure_single_stream_resume_supported(
    temp_path: &Path,
    existing_bytes: u64,
    supports_resume: bool,
) -> Result<(), DownloadError> {
    if existing_bytes == 0 || supports_resume {
        return Ok(());
    }

    Err(download_error(
        FailureCategory::Resume,
        format!(
            "The server refused to resume this partial download after {existing_bytes} bytes; the partial file was preserved at {}. Use Restart to download from zero.",
            temp_path.display()
        ),
        false,
    ))
}

pub(super) fn bulk_slow_stream_recovery_action(
    sample_bytes: u64,
    elapsed: Duration,
    total_bytes: Option<u64>,
    _downloaded_bytes: u64,
    speed_limit: Option<u64>,
    profile: DownloadPerformanceProfile,
    recovery_state: Option<BulkMemberSlowRecoveryState>,
) -> BulkSlowStreamRecoveryAction {
    let Some(_recovery_state) = recovery_state else {
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

    BulkSlowStreamRecoveryAction::Retry
}

fn accelerated_hoster_fair_origin_workers() -> Option<usize> {
    Some(4)
}

fn bulk_slow_recovery_threshold(profile: DownloadPerformanceProfile) -> u64 {
    let _ = profile;
    BULK_SLOW_RECOVERY_BALANCED_THRESHOLD
}

pub(super) async fn retry_bulk_archive_creation<A: DownloadUi>(
    app: &A,
    state: &SharedState,
    archive_id: &str,
) -> Result<(), String> {
    let archive = state.bulk_archive_ready_for_retry(archive_id).await?;
    create_bulk_archive_from_ready(app, state, archive, None).await
}

struct BulkFinalizeProgressChannel {
    sender: mpsc::UnboundedSender<BulkFinalizeProgressUpdate>,
}

impl BulkFinalizeProgressListener for BulkFinalizeProgressChannel {
    fn report(&self, update: BulkFinalizeProgressUpdate) {
        let _ = self.sender.send(update);
    }
}

struct BulkFinalizeProgressRuntime {
    reporter: BulkFinalizeProgressReporter,
    status: Arc<StdMutex<BulkArchiveStatus>>,
    handle: tauri::async_runtime::JoinHandle<()>,
}

impl BulkFinalizeProgressRuntime {
    fn set_status(&self, status: BulkArchiveStatus) {
        *self
            .status
            .lock()
            .expect("bulk finalize progress status lock poisoned") = status;
    }

    async fn stop(self) {
        drop(self.reporter);
        let _ = self.handle.await;
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_bulk_finalize_progress_runtime<A: DownloadUi>(
    app: A,
    state: SharedState,
    archive_id: String,
    initial_status: BulkArchiveStatus,
    requires_extraction: bool,
    output_path: String,
    warning: Option<String>,
    finalize_mode: BulkFinalizeMode,
    initial_total_bytes: u64,
) -> BulkFinalizeProgressRuntime {
    let (sender, mut receiver) = mpsc::unbounded_channel::<BulkFinalizeProgressUpdate>();
    let listener = Arc::new(BulkFinalizeProgressChannel { sender });
    let reporter = BulkFinalizeProgressReporter::new(initial_total_bytes, listener);
    let status = Arc::new(StdMutex::new(initial_status));
    let status_for_task = status.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let mut latest = None;
        let mut last_emit = Instant::now() - Duration::from_millis(500);
        while let Some(update) = receiver.recv().await {
            latest = Some(update);
            let should_emit = last_emit.elapsed() >= Duration::from_millis(250)
                || (update.total_bytes > 0 && update.processed_bytes >= update.total_bytes);
            if !should_emit {
                continue;
            }
            if let Some(update) = latest.take() {
                emit_bulk_finalize_progress_update(
                    &app,
                    &state,
                    &archive_id,
                    &status_for_task,
                    requires_extraction,
                    &output_path,
                    warning.clone(),
                    finalize_mode,
                    update,
                )
                .await;
                last_emit = Instant::now();
            }
        }
        if let Some(update) = latest {
            emit_bulk_finalize_progress_update(
                &app,
                &state,
                &archive_id,
                &status_for_task,
                requires_extraction,
                &output_path,
                warning,
                finalize_mode,
                update,
            )
            .await;
        }
    });

    BulkFinalizeProgressRuntime {
        reporter,
        status,
        handle,
    }
}

#[allow(clippy::too_many_arguments)]
async fn emit_bulk_finalize_progress_update<A: DownloadUi>(
    app: &A,
    state: &SharedState,
    archive_id: &str,
    status: &Arc<StdMutex<BulkArchiveStatus>>,
    requires_extraction: bool,
    output_path: &str,
    warning: Option<String>,
    finalize_mode: BulkFinalizeMode,
    update: BulkFinalizeProgressUpdate,
) {
    let archive_status = *status
        .lock()
        .expect("bulk finalize progress status lock poisoned");
    match state
        .mark_bulk_archive_status(
            archive_id,
            archive_status,
            Some(requires_extraction),
            Some(output_path.to_string()),
            None,
            warning,
            Some(finalize_mode),
            Some(update.total_bytes),
            Some(update.processed_bytes),
        )
        .await
    {
        Ok(snapshot) => emit_snapshot(app, &snapshot),
        Err(error) => eprintln!("failed to persist bulk finalization progress: {error}"),
    }
}

async fn create_bulk_archive_from_ready<A: DownloadUi>(
    app: &A,
    state: &SharedState,
    archive: BulkArchiveReady,
    diagnostic_job_id: Option<String>,
) -> Result<(), String> {
    let archive_id = archive.archive_id.clone();
    let plan = match plan_bulk_archive_finalization(archive.clone()).await {
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

    let progress_runtime = spawn_bulk_finalize_progress_runtime(
        app.clone(),
        state.clone(),
        archive_id.clone(),
        initial_status,
        requires_extraction,
        archive_output_path.clone(),
        plan.warning.clone(),
        plan.finalize_mode,
        plan.total_completed_bytes,
    );
    let _finalization_io_permit = acquire_bulk_finalization_io_permit().await;

    let prepared = match prepare_bulk_archive_sources_with_progress(
        archive,
        seven_zip_path,
        progress_runtime.reporter.clone(),
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(error) => {
            progress_runtime.stop().await;
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
        progress_runtime.set_status(BulkArchiveStatus::Combining);
        let update = progress_runtime.reporter.current();
        let snapshot = state
            .mark_bulk_archive_status(
                &archive_id,
                BulkArchiveStatus::Combining,
                Some(requires_extraction),
                Some(archive_output_path.clone()),
                None,
                plan.warning.clone(),
                Some(plan.finalize_mode),
                Some(update.total_bytes),
                Some(update.processed_bytes),
            )
            .await?;
        emit_snapshot(app, &snapshot);
    }

    match finish_prepared_bulk_archive_with_progress(prepared, progress_runtime.reporter.clone())
        .await
    {
        Ok(outcome) => {
            let final_progress = progress_runtime.reporter.current();
            progress_runtime.stop().await;
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
                    Some(final_progress.total_bytes),
                    Some(final_progress.total_bytes),
                )
                .await?;
            emit_snapshot(app, &snapshot);
            notify_bulk_archive_completed(app, state, &outcome.output_path).await;
            Ok(())
        }
        Err(error) => {
            progress_runtime.stop().await;
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

async fn mark_bulk_archive_create_failed<A: DownloadUi>(
    app: &A,
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
