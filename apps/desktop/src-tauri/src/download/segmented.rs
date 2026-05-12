use super::*;

static SEGMENT_METADATA_LOCKS: OnceLock<StdMutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    OnceLock::new();
static SEGMENT_METADATA_WRITE_COUNTER: AtomicU64 = AtomicU64::new(1);
const DYNAMIC_SEGMENT_QUEUE_MULTIPLIER: usize = 2;
const DYNAMIC_SEGMENT_MIN_SPLIT_SIZE: u64 = 1024 * 1024;
const SEGMENTED_RESUME_METADATA_REQUIRED_MESSAGE: &str =
    "Resume metadata is missing or no longer matches this partial download. Use Restart to download from zero.";

struct SegmentJournal {
    temp_path: PathBuf,
}

impl SegmentJournal {
    fn new(temp_path: &Path) -> Self {
        Self {
            temp_path: temp_path.to_path_buf(),
        }
    }

    async fn load_recoverable_state(
        &self,
        plan: &RangePlan,
        validators: &EntityValidators,
    ) -> Result<Option<SegmentedDownloadState>, DownloadError> {
        if let Some(state) = self
            .load_state_from(&segment_meta_path(&self.temp_path))
            .await?
        {
            if let Some(state) = reconcile_segment_state(state, plan, validators) {
                return Ok(Some(state));
            }
        }

        let Some(state) = self
            .load_state_from(&segment_meta_backup_path(&self.temp_path))
            .await?
            .and_then(|state| reconcile_segment_state(state, plan, validators))
        else {
            return Ok(None);
        };

        persist_segment_state(&self.temp_path, &state).await?;
        Ok(Some(state))
    }

    async fn load_state_from(
        &self,
        path: &Path,
    ) -> Result<Option<SegmentedDownloadState>, DownloadError> {
        match fs::read_to_string(path).await {
            Ok(raw) => Ok(serde_json::from_str::<SegmentedDownloadState>(&raw).ok()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(disk_error(format!(
                "Could not read segment metadata sidecar: {error}"
            ))),
        }
    }
}

#[cfg(test)]
pub(super) fn plan_segmented_ranges(
    total_bytes: u64,
    resume_support: ResumeSupport,
    speed_limit: Option<u64>,
    profile: DownloadPerformanceProfile,
) -> Option<RangePlan> {
    plan_segmented_ranges_with_budget(total_bytes, resume_support, speed_limit, profile, None)
}

pub(super) fn plan_segmented_ranges_with_budget(
    total_bytes: u64,
    resume_support: ResumeSupport,
    speed_limit: Option<u64>,
    profile: DownloadPerformanceProfile,
    segment_budget: Option<usize>,
) -> Option<RangePlan> {
    let max_segments = segment_budget
        .map(|budget| profile.max_segments.min(budget))
        .unwrap_or(profile.max_segments);
    if speed_limit.is_some()
        || resume_support != ResumeSupport::Supported
        || total_bytes < profile.min_segmented_size
        || max_segments < 2
    {
        return None;
    }

    let target_segment_size = profile.target_segment_size.max(1);
    let segment_count = max_segments
        .min(total_bytes.div_ceil(target_segment_size).max(2) as usize)
        .max(2);
    let segment_size = total_bytes / segment_count as u64;
    let mut segments = Vec::with_capacity(segment_count);

    for index in 0..segment_count {
        let start = index as u64 * segment_size;
        let end = if index == segment_count - 1 {
            total_bytes - 1
        } else {
            ((index as u64 + 1) * segment_size).saturating_sub(1)
        };
        segments.push(ByteRange { start, end });
    }

    Some(RangePlan {
        total_bytes,
        segments,
    })
}

pub(super) async fn probe_range_metadata(
    client: &Client,
    url: &str,
    handoff_auth: Option<&HandoffAuth>,
) -> Option<PreflightMetadata> {
    let response = send_range_request(
        client,
        url,
        ByteRange { start: 0, end: 0 },
        handoff_auth,
        None,
    )
    .await
    .ok()?;

    if response.status() != StatusCode::PARTIAL_CONTENT {
        return None;
    }

    let (range, total_bytes) = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range)?;

    if range != (ByteRange { start: 0, end: 0 }) || total_bytes == 0 {
        return None;
    }

    Some(PreflightMetadata {
        total_bytes: Some(total_bytes),
        resume_support: ResumeSupport::Supported,
        filename: extract_filename(&response)
            .or_else(|| derive_filename_from_url(response.url().as_str())),
        validators: entity_validators_from_headers(response.headers()),
    })
}

pub(super) fn merge_preflight_metadata(
    existing: Option<PreflightMetadata>,
    probed: PreflightMetadata,
) -> PreflightMetadata {
    let Some(existing) = existing else {
        return probed;
    };

    PreflightMetadata {
        total_bytes: probed.total_bytes.or(existing.total_bytes),
        resume_support: if probed.resume_support == ResumeSupport::Supported {
            ResumeSupport::Supported
        } else {
            existing.resume_support
        },
        filename: existing.filename.or(probed.filename),
        validators: existing.validators.reconcile_with(&probed.validators),
    }
}

fn dynamic_segment_queue_depth(worker_count: usize) -> usize {
    worker_count
        .saturating_mul(DYNAMIC_SEGMENT_QUEUE_MULTIPLIER)
        .max(worker_count)
        .max(1)
}

fn dynamic_segment_min_split_size(profile: DownloadPerformanceProfile) -> u64 {
    (profile.target_segment_size / 4)
        .max(DYNAMIC_SEGMENT_MIN_SPLIT_SIZE)
        .max(1)
}

fn segment_remaining_bytes(segment: &SegmentProgress) -> u64 {
    if segment.completed {
        return 0;
    }

    segment.range.len().saturating_sub(segment.downloaded_bytes)
}

fn pending_segment_count(state: &SegmentedDownloadState, active: &HashSet<usize>) -> usize {
    state
        .segments
        .iter()
        .filter(|segment| !active.contains(&segment.index) && segment_remaining_bytes(segment) > 0)
        .count()
}

fn largest_pending_segment_position(
    state: &SegmentedDownloadState,
    active: &HashSet<usize>,
    min_remaining_bytes: u64,
) -> Option<usize> {
    let mut best: Option<(usize, u64, u64)> = None;
    for (position, segment) in state.segments.iter().enumerate() {
        if active.contains(&segment.index) {
            continue;
        }
        let remaining = segment_remaining_bytes(segment);
        if remaining < min_remaining_bytes {
            continue;
        }
        let key = (remaining, u64::MAX.saturating_sub(segment.range.start));
        if best
            .map(|(_, best_remaining, best_start_key)| {
                key.0 > best_remaining || (key.0 == best_remaining && key.1 > best_start_key)
            })
            .unwrap_or(true)
        {
            best = Some((position, key.0, key.1));
        }
    }

    best.map(|(position, _, _)| position)
}

fn split_largest_pending_segment(
    state: &mut SegmentedDownloadState,
    active: &HashSet<usize>,
    min_split_size: u64,
) -> bool {
    let min_split_size = min_split_size.max(1);
    let Some(position) =
        largest_pending_segment_position(state, active, min_split_size.saturating_mul(2))
    else {
        return false;
    };

    let segment = state.segments[position].clone();
    let remaining_start = segment
        .range
        .start
        .saturating_add(segment.downloaded_bytes.min(segment.range.len()));
    let remaining = segment_remaining_bytes(&segment);
    let first_remaining = remaining / 2;
    if first_remaining < min_split_size
        || remaining.saturating_sub(first_remaining) < min_split_size
    {
        return false;
    }

    let split_end = remaining_start
        .saturating_add(first_remaining)
        .saturating_sub(1);
    if split_end >= segment.range.end {
        return false;
    }

    let next_index = state
        .segments
        .iter()
        .map(|segment| segment.index)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    state.segments[position].range.end = split_end;
    state.segments[position].completed = false;
    state.segments.push(SegmentProgress {
        index: next_index,
        range: ByteRange {
            start: split_end.saturating_add(1),
            end: segment.range.end,
        },
        downloaded_bytes: 0,
        completed: false,
    });
    state.segments.sort_by_key(|segment| segment.range.start);
    state.retry_generation = state.retry_generation.saturating_add(1);
    true
}

fn fill_dynamic_segment_queue(
    state: &mut SegmentedDownloadState,
    active: &HashSet<usize>,
    target_depth: usize,
    min_split_size: u64,
) -> bool {
    let mut changed = false;
    while pending_segment_count(state, active) < target_depth {
        if !split_largest_pending_segment(state, active, min_split_size) {
            break;
        }
        changed = true;
    }
    changed
}

fn claim_largest_dynamic_segment(
    state: &mut SegmentedDownloadState,
    active: &mut HashSet<usize>,
    target_depth: usize,
    min_split_size: u64,
) -> (Option<SegmentProgress>, bool) {
    let changed = fill_dynamic_segment_queue(state, active, target_depth, min_split_size);
    let Some(position) = largest_pending_segment_position(state, active, 1) else {
        return (None, changed);
    };

    let segment = state.segments[position].clone();
    active.insert(segment.index);
    (Some(segment), changed)
}

#[cfg(test)]
pub(super) fn claim_largest_dynamic_segment_for_tests(
    state: &mut SegmentedDownloadState,
    active: &mut HashSet<usize>,
    target_depth: usize,
    min_split_size: u64,
) -> Option<SegmentProgress> {
    claim_largest_dynamic_segment(state, active, target_depth, min_split_size).0
}

async fn claim_dynamic_segment_work(
    temp_path: &Path,
    metadata: &Arc<Mutex<SegmentedDownloadState>>,
    active: &Arc<Mutex<HashSet<usize>>>,
    target_depth: usize,
    min_split_size: u64,
) -> Result<Option<SegmentProgress>, DownloadError> {
    let mut active = active.lock().await;
    let mut metadata = metadata.lock().await;
    let (segment, changed) =
        claim_largest_dynamic_segment(&mut metadata, &mut active, target_depth, min_split_size);
    if changed {
        persist_segment_state(temp_path, &metadata).await?;
    }

    Ok(segment)
}

async fn release_dynamic_segment_work(active: &Arc<Mutex<HashSet<usize>>>, segment_index: usize) {
    active.lock().await.remove(&segment_index);
}

fn update_segment_recovery_metadata(
    state: &mut SegmentedDownloadState,
    effective_url: &str,
    target_path: &Path,
    temp_path: &Path,
) {
    state.schema_version = default_segment_state_schema_version();
    state.effective_url = Some(effective_url.to_string());
    state.target_path = Some(target_path.display().to_string());
    state.temp_path = Some(temp_path.display().to_string());
}

pub(super) fn segmented_error_allows_single_stream_fallback(error: &DownloadError) -> bool {
    error.category == FailureCategory::Resume
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_segmented_download_attempt(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
    client: Client,
    effective_url: String,
    request_auth: Option<HandoffAuth>,
    plan: RangePlan,
    profile: DownloadPerformanceProfile,
    validators: EntityValidators,
) -> Result<DownloadOutcome, DownloadError> {
    let mut segment_state =
        load_or_create_segment_state(&task.temp_path, &plan, &validators).await?;
    update_segment_recovery_metadata(
        &mut segment_state,
        &effective_url,
        &task.target_path,
        &task.temp_path,
    );
    refresh_segment_completion_from_disk(&task.temp_path, &mut segment_state).await;
    fill_dynamic_segment_queue(
        &mut segment_state,
        &HashSet::new(),
        dynamic_segment_queue_depth(plan.segments.len()),
        dynamic_segment_min_split_size(profile),
    );
    persist_segment_state(&task.temp_path, &segment_state).await?;
    prepare_direct_segment_file(&task.temp_path, plan.total_bytes).await?;

    let initial_segment_bytes =
        segment_existing_lengths_by_index(&task.temp_path, &segment_state.segments);
    let initial_downloaded = initial_segment_bytes.iter().sum::<u64>();

    let snapshot = state
        .mark_job_downloading(
            &task.id,
            initial_downloaded,
            Some(plan.total_bytes),
            ResumeSupport::Supported,
            None,
        )
        .await?;
    emit_snapshot(app, &snapshot);

    let progress = Arc::new(SegmentedProgressCounters::new(
        initial_segment_bytes.clone(),
    ));
    let reporter_stop = Arc::new(AtomicBool::new(false));
    let metadata = Arc::new(Mutex::new(segment_state));
    let worker_stop = Arc::new(AtomicBool::new(false));
    let active_segments = Arc::new(Mutex::new(HashSet::new()));
    let priority_throttle = Arc::new(Mutex::new(DynamicThrottleState::default()));
    let reporter_handle = tauri::async_runtime::spawn(report_segmented_progress(
        app.clone(),
        state.clone(),
        task.clone(),
        plan.total_bytes,
        profile,
        progress.clone(),
        reporter_stop.clone(),
    ));
    let worker_context = SegmentWorkerContext {
        state: state.clone(),
        client: client.clone(),
        job_id: task.id.clone(),
        url: effective_url,
        handoff_auth: request_auth,
        temp_path: task.temp_path.clone(),
        total_bytes: plan.total_bytes,
        profile,
        validators: validators.clone(),
        progress: progress.clone(),
        metadata: metadata.clone(),
        stop: worker_stop.clone(),
        priority_throttle,
        stall_timeout: protected_bulk_hoster_stall_timeout(task, profile),
    };

    let mut handles = tokio::task::JoinSet::new();
    let worker_count = plan.segments.len().max(1);
    let queue_depth = dynamic_segment_queue_depth(worker_count);
    let min_split_size = dynamic_segment_min_split_size(profile);
    for _ in 0..worker_count {
        handles.spawn(download_dynamic_segment_worker(
            worker_context.clone(),
            active_segments.clone(),
            queue_depth,
            min_split_size,
        ));
    }

    let (worker_outcome, mut worker_error) =
        await_segment_workers_with_stop(handles, worker_stop).await;

    reporter_stop.store(true, Ordering::Relaxed);
    match reporter_handle.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) if worker_error.is_none() => worker_error = Some(error),
        Ok(Err(_)) => {}
        Err(error) if worker_error.is_none() => {
            worker_error = Some(download_error(
                FailureCategory::Internal,
                format!("Segment progress reporter failed: {error}"),
                true,
            ));
        }
        Err(_) => {}
    }

    if let Some(error) = worker_error {
        return Err(error);
    }

    if worker_outcome != DownloadOutcome::Completed {
        return Ok(worker_outcome);
    }

    let final_state = metadata.lock().await.clone();
    if !final_state.segments.iter().all(|segment| segment.completed) {
        return Err(download_error(
            FailureCategory::Network,
            "Segmented download ended before every segment completed.".into(),
            true,
        ));
    }

    sync_direct_segment_file(&task.temp_path).await?;
    cleanup_segment_artifacts(&task.temp_path, final_state.segments.len()).await;

    let snapshot = state
        .update_job_progress(&task.id, plan.total_bytes, Some(plan.total_bytes), 0, true)
        .await?;
    emit_download_update(app, &snapshot, &task.id);

    let final_path = move_to_final_path(&task.temp_path, &task.target_path)
        .await
        .map_err(disk_error)?;
    complete_http_download(app, state, task, plan.total_bytes, &final_path).await?;
    Ok(DownloadOutcome::Completed)
}

pub(super) async fn download_dynamic_segment_worker(
    context: SegmentWorkerContext,
    active_segments: Arc<Mutex<HashSet<usize>>>,
    queue_depth: usize,
    min_split_size: u64,
) -> Result<DownloadOutcome, DownloadError> {
    loop {
        if context.stop.load(Ordering::Relaxed) {
            return Ok(DownloadOutcome::Paused);
        }

        let Some(segment) = claim_dynamic_segment_work(
            &context.temp_path,
            &context.metadata,
            &active_segments,
            queue_depth,
            min_split_size,
        )
        .await?
        else {
            return Ok(DownloadOutcome::Completed);
        };
        let segment_index = segment.index;
        let outcome = download_segment_worker(context.clone(), segment).await;

        if !matches!(&outcome, Ok(DownloadOutcome::Completed)) {
            context.stop.store(true, Ordering::Relaxed);
        }
        release_dynamic_segment_work(&active_segments, segment_index).await;

        match outcome? {
            DownloadOutcome::Completed => {}
            outcome => return Ok(outcome),
        }
    }
}

pub(super) async fn download_segment_worker(
    context: SegmentWorkerContext,
    segment: SegmentProgress,
) -> Result<DownloadOutcome, DownloadError> {
    let mut current_len = segment_existing_len(&context.temp_path, &segment);
    let mut low_speed_monitor = LowSpeedMonitor::new(context.profile);
    let mut file = open_direct_segment_file(&context.temp_path).await?;
    let mut last_metadata_persisted_at = Instant::now();

    while current_len < segment.range.len() {
        if context.stop.load(Ordering::Relaxed) {
            record_segment_progress(
                &context.temp_path,
                &context.metadata,
                segment.index,
                current_len,
                false,
                true,
            )
            .await?;
            return Ok(DownloadOutcome::Paused);
        }

        match context.state.worker_control(&context.job_id).await {
            WorkerControl::Paused => {
                record_segment_progress(
                    &context.temp_path,
                    &context.metadata,
                    segment.index,
                    current_len,
                    false,
                    true,
                )
                .await?;
                return Ok(DownloadOutcome::Paused);
            }
            WorkerControl::Canceled | WorkerControl::Missing => {
                record_segment_progress(
                    &context.temp_path,
                    &context.metadata,
                    segment.index,
                    current_len,
                    false,
                    true,
                )
                .await?;
                return Ok(DownloadOutcome::Canceled);
            }
            WorkerControl::Continue => {}
        }

        let requested = ByteRange {
            start: segment.range.start + current_len,
            end: segment.range.end,
        };
        let response = match send_range_request(
            &context.client,
            &context.url,
            requested,
            context.handoff_auth.as_ref(),
            Some(&context.validators),
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                if error.category == FailureCategory::Resume {
                    range_backoffs().record_rejection(&context.url, Instant::now());
                }
                record_segment_progress(
                    &context.temp_path,
                    &context.metadata,
                    segment.index,
                    current_len,
                    false,
                    true,
                )
                .await?;
                return Err(error);
            }
        };

        if response.status() != StatusCode::PARTIAL_CONTENT {
            range_backoffs().record_rejection(&context.url, Instant::now());
            record_segment_progress(
                &context.temp_path,
                &context.metadata,
                segment.index,
                current_len,
                false,
                true,
            )
            .await?;
            return Err(download_error(
                FailureCategory::Resume,
                "The server did not honor a segmented range request.".into(),
                false,
            ));
        }

        let range_ok = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .map(|value| content_range_matches(value, requested, context.total_bytes))
            .unwrap_or(false);

        if !range_ok {
            range_backoffs().record_rejection(&context.url, Instant::now());
            record_segment_progress(
                &context.temp_path,
                &context.metadata,
                segment.index,
                current_len,
                false,
                true,
            )
            .await?;
            return Err(download_error(
                FailureCategory::Resume,
                "The server returned an unexpected Content-Range for a segment.".into(),
                false,
            ));
        }

        let mut stream = response.bytes_stream();
        let mut low_speed_bytes = 0_u64;
        let mut low_speed_started = Instant::now();
        let mut priority_throttle_limited = false;

        loop {
            let chunk_result = match next_stream_item_with_control(
                &context.state,
                &context.job_id,
                context.stall_timeout,
                stream.next(),
            )
            .await
            {
                StreamItemWait::Item(result) => result,
                StreamItemWait::Interrupted(DownloadOutcome::Paused) => {
                    record_segment_progress(
                        &context.temp_path,
                        &context.metadata,
                        segment.index,
                        current_len,
                        false,
                        true,
                    )
                    .await?;
                    return Ok(DownloadOutcome::Paused);
                }
                StreamItemWait::Interrupted(DownloadOutcome::Canceled) => {
                    record_segment_progress(
                        &context.temp_path,
                        &context.metadata,
                        segment.index,
                        current_len,
                        false,
                        true,
                    )
                    .await?;
                    return Ok(DownloadOutcome::Canceled);
                }
                StreamItemWait::Interrupted(DownloadOutcome::Completed) => unreachable!(
                    "stream control cannot produce a completed outcome while waiting for chunks"
                ),
                StreamItemWait::Interrupted(DownloadOutcome::Deferred(_)) => unreachable!(
                    "stream control cannot produce a deferred outcome while waiting for chunks"
                ),
                StreamItemWait::Stalled => {
                    let timeout = context
                        .stall_timeout
                        .expect("stall wait can only stall when configured");
                    record_segment_progress(
                        &context.temp_path,
                        &context.metadata,
                        segment.index,
                        current_len,
                        false,
                        true,
                    )
                    .await?;
                    return Err(bulk_hoster_stall_error(timeout));
                }
            };
            let Some(chunk_result) = chunk_result else {
                break;
            };

            if context.stop.load(Ordering::Relaxed) {
                record_segment_progress(
                    &context.temp_path,
                    &context.metadata,
                    segment.index,
                    current_len,
                    false,
                    true,
                )
                .await?;
                return Ok(DownloadOutcome::Paused);
            }

            match context.state.worker_control(&context.job_id).await {
                WorkerControl::Paused => {
                    record_segment_progress(
                        &context.temp_path,
                        &context.metadata,
                        segment.index,
                        current_len,
                        false,
                        true,
                    )
                    .await?;
                    return Ok(DownloadOutcome::Paused);
                }
                WorkerControl::Canceled | WorkerControl::Missing => {
                    record_segment_progress(
                        &context.temp_path,
                        &context.metadata,
                        segment.index,
                        current_len,
                        false,
                        true,
                    )
                    .await?;
                    return Ok(DownloadOutcome::Canceled);
                }
                WorkerControl::Continue => {}
            }

            let chunk = match chunk_result {
                Ok(chunk) => chunk,
                Err(error) => {
                    record_segment_progress(
                        &context.temp_path,
                        &context.metadata,
                        segment.index,
                        current_len,
                        false,
                        true,
                    )
                    .await?;
                    return Err(download_stream_error(error));
                }
            };
            let chunk_len = chunk.len() as u64;
            if chunk_len > segment.range.len().saturating_sub(current_len) {
                range_backoffs().record_rejection(&context.url, Instant::now());
                record_segment_progress(
                    &context.temp_path,
                    &context.metadata,
                    segment.index,
                    current_len,
                    false,
                    true,
                )
                .await?;
                return Err(download_error(
                    FailureCategory::Resume,
                    "The server returned more bytes than the requested segment range.".into(),
                    false,
                ));
            }

            write_segment_chunk_to(&mut file, segment.range.start + current_len, &chunk).await?;

            current_len = current_len
                .saturating_add(chunk_len)
                .min(segment.range.len());
            low_speed_bytes = low_speed_bytes.saturating_add(chunk_len);
            context
                .progress
                .store_segment_bytes(segment.index, current_len);
            context.progress.add_sample_bytes(chunk_len);
            if let Some(decision) = context
                .state
                .hoster_priority_throttle_decision(&context.job_id)
                .await
            {
                priority_throttle_limited = true;
                match throttle_download_with_dynamic_limit(
                    &context.state,
                    &context.job_id,
                    &context.priority_throttle,
                    decision.cap_bytes_per_second,
                    chunk_len,
                )
                .await
                {
                    WorkerControl::Paused => {
                        record_segment_progress(
                            &context.temp_path,
                            &context.metadata,
                            segment.index,
                            current_len,
                            false,
                            true,
                        )
                        .await?;
                        return Ok(DownloadOutcome::Paused);
                    }
                    WorkerControl::Canceled | WorkerControl::Missing => {
                        record_segment_progress(
                            &context.temp_path,
                            &context.metadata,
                            segment.index,
                            current_len,
                            false,
                            true,
                        )
                        .await?;
                        return Ok(DownloadOutcome::Canceled);
                    }
                    WorkerControl::Continue => {}
                }
            } else {
                clear_dynamic_throttle(&context.priority_throttle).await;
            }

            let should_persist_metadata =
                last_metadata_persisted_at.elapsed() >= PROGRESS_PERSIST_INTERVAL;
            record_segment_progress(
                &context.temp_path,
                &context.metadata,
                segment.index,
                current_len,
                false,
                should_persist_metadata,
            )
            .await?;
            if should_persist_metadata {
                last_metadata_persisted_at = Instant::now();
            }

            if low_speed_started.elapsed() >= context.profile.low_speed_window {
                if low_speed_monitor.observe(
                    low_speed_bytes,
                    low_speed_started.elapsed(),
                    priority_throttle_limited,
                ) == LowSpeedDecision::Retry
                {
                    record_segment_progress(
                        &context.temp_path,
                        &context.metadata,
                        segment.index,
                        current_len,
                        false,
                        true,
                    )
                    .await?;
                    tokio::time::sleep(retry_delay_for_attempt(
                        low_speed_monitor.retries.saturating_sub(1) as usize,
                    ))
                    .await;
                    break;
                }
                low_speed_bytes = 0;
                low_speed_started = Instant::now();
                priority_throttle_limited = false;
            }
        }

        if current_len >= segment.range.len() {
            mark_segment_completed(&context.temp_path, &context.metadata, segment.index).await?;
            return Ok(DownloadOutcome::Completed);
        }

        if low_speed_monitor.retries >= context.profile.max_low_speed_retries {
            record_segment_progress(
                &context.temp_path,
                &context.metadata,
                segment.index,
                current_len,
                false,
                true,
            )
            .await?;
            return Err(download_error(
                FailureCategory::Network,
                "A segment stayed below the recovery speed threshold.".into(),
                true,
            ));
        }
    }

    mark_segment_completed(&context.temp_path, &context.metadata, segment.index).await?;
    Ok(DownloadOutcome::Completed)
}

pub(super) async fn report_segmented_progress(
    app: AppHandle,
    state: SharedState,
    task: crate::state::DownloadTask,
    total_bytes: u64,
    profile: DownloadPerformanceProfile,
    progress: Arc<SegmentedProgressCounters>,
    stop: Arc<AtomicBool>,
) -> Result<(), DownloadError> {
    let job_id = task.id.clone();
    let mut rolling_speed = RollingSpeed::with_alpha(profile.speed_smoothing_alpha);
    let mut sample_started = Instant::now();
    let mut last_persisted_at = Instant::now();
    let mut interval = tokio::time::interval(PROGRESS_UPDATE_INTERVAL);

    loop {
        interval.tick().await;

        let stopping = stop.load(Ordering::Relaxed);
        let sample_bytes = progress.drain_sample_bytes();
        if sample_bytes == 0 && !stopping {
            continue;
        }

        let elapsed = sample_started.elapsed();
        let speed = if elapsed.as_secs_f64() > 0.0 {
            rolling_speed.record_sample(sample_bytes, elapsed)
        } else {
            0
        };
        sample_started = Instant::now();

        let downloaded_bytes = progress.total_downloaded();
        let should_persist = stopping || last_persisted_at.elapsed() >= PROGRESS_PERSIST_INTERVAL;
        if should_persist {
            last_persisted_at = Instant::now();
        }

        let snapshot = match state.worker_control(&job_id).await {
            WorkerControl::Continue => {
                state
                    .update_job_progress(
                        &job_id,
                        downloaded_bytes,
                        Some(total_bytes),
                        speed,
                        should_persist,
                    )
                    .await?
            }
            WorkerControl::Paused | WorkerControl::Canceled | WorkerControl::Missing => {
                state
                    .sync_downloaded_bytes(&job_id, downloaded_bytes)
                    .await?
            }
        };
        emit_download_update(&app, &snapshot, &job_id);
        if task_releases_bulk_hoster_fairness(&task, speed) {
            schedule_downloads(app.clone(), state.clone());
        }

        if stopping {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
pub(super) async fn await_segment_workers(
    handles: tokio::task::JoinSet<Result<DownloadOutcome, DownloadError>>,
) -> (DownloadOutcome, Option<DownloadError>) {
    await_segment_workers_with_stop(handles, Arc::new(AtomicBool::new(false))).await
}

pub(super) async fn await_segment_workers_with_stop(
    mut handles: tokio::task::JoinSet<Result<DownloadOutcome, DownloadError>>,
    stop: Arc<AtomicBool>,
) -> (DownloadOutcome, Option<DownloadError>) {
    while let Some(result) = handles.join_next().await {
        match result {
            Ok(Ok(DownloadOutcome::Completed)) => {}
            Ok(Ok(
                outcome @ (DownloadOutcome::Paused
                | DownloadOutcome::Canceled
                | DownloadOutcome::Deferred(_)),
            )) => {
                stop.store(true, Ordering::Relaxed);
                handles.abort_all();
                return (outcome, None);
            }
            Ok(Err(error)) => {
                stop.store(true, Ordering::Relaxed);
                drain_segment_workers_after_stop(&mut handles).await;
                return (DownloadOutcome::Completed, Some(error));
            }
            Err(error) => {
                stop.store(true, Ordering::Relaxed);
                drain_segment_workers_after_stop(&mut handles).await;
                return (
                    DownloadOutcome::Completed,
                    Some(download_error(
                        FailureCategory::Internal,
                        format!("Segment worker failed: {error}"),
                        true,
                    )),
                );
            }
        }
    }

    (DownloadOutcome::Completed, None)
}

async fn drain_segment_workers_after_stop(
    handles: &mut tokio::task::JoinSet<Result<DownloadOutcome, DownloadError>>,
) {
    let drain = async { while handles.join_next().await.is_some() {} };
    if tokio::time::timeout(SEGMENT_WORKER_STOP_GRACE, drain)
        .await
        .is_err()
    {
        handles.abort_all();
    }
}

pub(super) async fn load_or_create_segment_state(
    temp_path: &Path,
    plan: &RangePlan,
    validators: &EntityValidators,
) -> Result<SegmentedDownloadState, DownloadError> {
    let partial_exists = temp_path.exists();
    let journal = SegmentJournal::new(temp_path);
    if let Some(state) = journal.load_recoverable_state(plan, validators).await? {
        return Ok(state);
    }
    if partial_exists {
        return Err(segmented_resume_metadata_required_error());
    }

    cleanup_partial_artifacts(temp_path).await;
    Ok(SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: plan.total_bytes,
        validators: validators.clone(),
        effective_url: None,
        target_path: None,
        temp_path: Some(temp_path.display().to_string()),
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: plan
            .segments
            .iter()
            .copied()
            .enumerate()
            .map(|(index, range)| SegmentProgress {
                index,
                range,
                downloaded_bytes: 0,
                completed: false,
            })
            .collect(),
    })
}

fn reconcile_segment_state(
    mut state: SegmentedDownloadState,
    plan: &RangePlan,
    validators: &EntityValidators,
) -> Option<SegmentedDownloadState> {
    if !segment_state_compatible_with_plan(&state, plan)
        || state.validators.conflicts_with(validators)
    {
        return None;
    }

    state.schema_version = default_segment_state_schema_version();
    state.validators = state.validators.reconcile_with(validators);
    Some(state)
}

pub(super) fn segmented_resume_metadata_required_error() -> DownloadError {
    download_error(
        FailureCategory::Resume,
        SEGMENTED_RESUME_METADATA_REQUIRED_MESSAGE.into(),
        false,
    )
}

fn segment_state_compatible_with_plan(state: &SegmentedDownloadState, plan: &RangePlan) -> bool {
    state.total_bytes == plan.total_bytes
        && !state.segments.is_empty()
        && segments_cover_total_bytes(&state.segments, plan.total_bytes)
}

fn segments_cover_total_bytes(segments: &[SegmentProgress], total_bytes: u64) -> bool {
    let mut ranges = segments
        .iter()
        .map(|segment| segment.range)
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);

    let mut next_start = 0_u64;
    for range in ranges {
        if range.start != next_start || range.end < range.start || range.end >= total_bytes {
            return false;
        }
        next_start = range.end.saturating_add(1);
    }

    next_start == total_bytes
}

pub(super) async fn prepare_direct_segment_file(
    temp_path: &Path,
    total_bytes: u64,
) -> Result<(), DownloadError> {
    let current_len = metadata_len(temp_path).await;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(temp_path)
        .await
        .map_err(|error| disk_error(format!("Could not create segmented partial file: {error}")))?;
    if current_len != Some(total_bytes) {
        file.set_len(total_bytes).await.map_err(|error| {
            disk_error(format!("Could not size segmented partial file: {error}"))
        })?;
    }
    Ok(())
}

pub(super) async fn open_direct_segment_file(temp_path: &Path) -> Result<fs::File, DownloadError> {
    OpenOptions::new()
        .write(true)
        .open(temp_path)
        .await
        .map_err(|error| disk_error(format!("Could not open segmented partial file: {error}")))
}

pub(super) async fn write_segment_chunk_to(
    file: &mut fs::File,
    offset: u64,
    chunk: &[u8],
) -> Result<(), DownloadError> {
    file.seek(SeekFrom::Start(offset))
        .await
        .map_err(|error| disk_error(format!("Could not seek segmented partial file: {error}")))?;
    file.write_all(chunk)
        .await
        .map_err(|error| disk_error(format!("Could not write segment chunk: {error}")))
}

pub(super) async fn sync_direct_segment_file(temp_path: &Path) -> Result<(), DownloadError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(temp_path)
        .await
        .map_err(|error| disk_error(format!("Could not open segmented partial file: {error}")))?;
    file.sync_all()
        .await
        .map_err(|error| disk_error(format!("Could not sync segmented partial file: {error}")))
}

pub(super) async fn refresh_segment_completion_from_disk(
    temp_path: &Path,
    state: &mut SegmentedDownloadState,
) {
    let partial_exists = temp_path.exists();
    state.last_verified_file_len = metadata_len(temp_path).await.unwrap_or(0);
    for segment in &mut state.segments {
        let expected_len = segment.range.len();
        if !partial_exists || segment.downloaded_bytes > expected_len {
            segment.downloaded_bytes = 0;
            segment.completed = false;
            continue;
        }

        if segment.completed || segment.downloaded_bytes == expected_len {
            segment.downloaded_bytes = expected_len;
            segment.completed = true;
        }
    }

    cleanup_legacy_segment_files(temp_path, state.segments.len()).await;
}

pub(super) fn segment_existing_len(_temp_path: &Path, segment: &SegmentProgress) -> u64 {
    segment.downloaded_bytes.min(segment.range.len())
}

pub(super) fn segment_existing_lengths_by_index(
    temp_path: &Path,
    segments: &[SegmentProgress],
) -> Vec<u64> {
    let mut lengths = vec![
        0;
        segments
            .iter()
            .map(|segment| segment.index)
            .max()
            .map(|index| index.saturating_add(1))
            .unwrap_or(0)
    ];
    for segment in segments {
        lengths[segment.index] = segment_existing_len(temp_path, segment);
    }
    lengths
}

pub(super) async fn mark_segment_completed(
    temp_path: &Path,
    metadata: &Arc<Mutex<SegmentedDownloadState>>,
    segment_index: usize,
) -> Result<(), DownloadError> {
    let range_len = {
        let metadata = metadata.lock().await;
        metadata
            .segments
            .iter()
            .find(|segment| segment.index == segment_index)
            .map(|segment| segment.range.len())
            .unwrap_or(0)
    };

    record_segment_progress(temp_path, metadata, segment_index, range_len, true, true).await
}

pub(super) async fn record_segment_progress(
    temp_path: &Path,
    metadata: &Arc<Mutex<SegmentedDownloadState>>,
    segment_index: usize,
    downloaded_bytes: u64,
    completed: bool,
    persist: bool,
) -> Result<(), DownloadError> {
    let mut metadata = metadata.lock().await;
    if let Some(segment) = metadata
        .segments
        .iter_mut()
        .find(|segment| segment.index == segment_index)
    {
        segment.downloaded_bytes = downloaded_bytes.min(segment.range.len());
        segment.completed = completed || segment.downloaded_bytes == segment.range.len();
    }
    if persist {
        persist_segment_state(temp_path, &metadata).await?;
    }

    Ok(())
}

pub(super) async fn persist_segment_state(
    temp_path: &Path,
    state: &SegmentedDownloadState,
) -> Result<(), DownloadError> {
    let serialized = serde_json::to_string_pretty(state)
        .map_err(|error| format!("Could not serialize segment metadata: {error}"))?;
    let lock = segment_metadata_lock(temp_path);
    let _guard = lock.lock().await;
    let meta_path = segment_meta_path(temp_path);
    let temp_meta_path = unique_segment_meta_temp_path(temp_path);
    let backup_meta_path = segment_meta_backup_path(temp_path);
    fs::write(&temp_meta_path, serialized)
        .await
        .map_err(|error| {
            disk_error(format!("Could not write segment metadata sidecar: {error}"))
        })?;

    let _ = fs::remove_file(&backup_meta_path).await;
    let had_existing_metadata = match fs::rename(&meta_path, &backup_meta_path).await {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            let _ = fs::remove_file(&temp_meta_path).await;
            return Err(disk_error(format!(
                "Could not stage segment metadata replacement: {error}"
            )));
        }
    };

    if let Err(error) = fs::rename(&temp_meta_path, &meta_path).await {
        if had_existing_metadata {
            let _ = fs::rename(&backup_meta_path, &meta_path).await;
        }
        let _ = fs::remove_file(&temp_meta_path).await;
        return Err(disk_error(format!(
            "Could not replace segment metadata sidecar: {error}"
        )));
    }

    let _ = fs::remove_file(&backup_meta_path).await;
    cleanup_stale_segment_metadata_temp_files(temp_path).await;
    Ok(())
}

pub(super) async fn cleanup_segment_artifacts(temp_path: &Path, segment_count: usize) {
    let _ = fs::remove_file(segment_meta_path(temp_path)).await;
    let _ = fs::remove_file(segment_meta_temp_path(temp_path)).await;
    let _ = fs::remove_file(segment_meta_backup_path(temp_path)).await;
    cleanup_stale_segment_metadata_temp_files(temp_path).await;
    cleanup_legacy_segment_files(temp_path, segment_count).await;
}

pub(super) async fn cleanup_legacy_segment_files(temp_path: &Path, segment_count: usize) {
    for index in 0..segment_count {
        let _ = fs::remove_file(segment_path(temp_path, index)).await;
    }
}

pub(super) async fn cleanup_partial_artifacts(temp_path: &Path) {
    let _ = fs::remove_file(temp_path).await;
    let _ = fs::remove_file(segment_meta_path(temp_path)).await;
    let _ = fs::remove_file(segment_meta_temp_path(temp_path)).await;
    let _ = fs::remove_file(segment_meta_backup_path(temp_path)).await;
    cleanup_stale_segment_metadata_temp_files(temp_path).await;

    let Some(parent) = temp_path.parent() else {
        return;
    };
    let Some(file_name) = temp_path.file_name().and_then(|value| value.to_str()) else {
        return;
    };
    let segment_prefix = format!("{file_name}.seg");

    let Ok(mut entries) = fs::read_dir(parent).await else {
        return;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let should_remove = entry
            .file_name()
            .to_str()
            .map(|name| name.starts_with(&segment_prefix))
            .unwrap_or(false);
        if should_remove {
            let _ = fs::remove_file(entry.path()).await;
        }
    }
}

pub(super) fn segment_meta_path(temp_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.meta", temp_path.display()))
}

pub(super) fn segment_meta_temp_path(temp_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.meta.tmp", temp_path.display()))
}

fn unique_segment_meta_temp_path(temp_path: &Path) -> PathBuf {
    let counter = SEGMENT_METADATA_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
    PathBuf::from(format!("{}.meta.{counter}.tmp", temp_path.display()))
}

fn segment_meta_backup_path(temp_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.meta.bak", temp_path.display()))
}

pub(super) fn segment_path(temp_path: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.seg{index}", temp_path.display()))
}

fn segment_metadata_lock(temp_path: &Path) -> Arc<Mutex<()>> {
    let key = temp_path.to_path_buf();
    let locks = SEGMENT_METADATA_LOCKS.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .expect("segment metadata lock registry should not be poisoned");
    locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

async fn cleanup_stale_segment_metadata_temp_files(temp_path: &Path) {
    let Some(parent) = temp_path.parent() else {
        return;
    };
    let Some(file_name) = temp_path.file_name().and_then(|value| value.to_str()) else {
        return;
    };
    let temp_prefix = format!("{file_name}.meta.");

    let Ok(mut entries) = fs::read_dir(parent).await else {
        return;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let should_remove = entry
            .file_name()
            .to_str()
            .map(|name| name.starts_with(&temp_prefix) && name.ends_with(".tmp"))
            .unwrap_or(false);
        if should_remove {
            let _ = fs::remove_file(entry.path()).await;
        }
    }
}
