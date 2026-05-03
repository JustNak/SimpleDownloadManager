use super::*;

pub(super) fn performance_profile(mode: DownloadPerformanceMode) -> DownloadPerformanceProfile {
    match mode {
        DownloadPerformanceMode::Stable => DownloadPerformanceProfile {
            max_segments: 1,
            min_segmented_size: u64::MAX,
            target_segment_size: u64::MAX,
            low_speed_threshold_bytes_per_second: 4 * 1024,
            low_speed_window: Duration::from_secs(30),
            max_low_speed_retries: 2,
            speed_smoothing_alpha: 0.25,
        },
        DownloadPerformanceMode::Balanced => DownloadPerformanceProfile {
            max_segments: 6,
            min_segmented_size: BALANCED_MIN_SEGMENTED_SIZE,
            target_segment_size: BALANCED_TARGET_SEGMENT_SIZE,
            low_speed_threshold_bytes_per_second: 8 * 1024,
            low_speed_window: Duration::from_secs(20),
            max_low_speed_retries: 2,
            speed_smoothing_alpha: 0.25,
        },
        DownloadPerformanceMode::Fast => DownloadPerformanceProfile {
            max_segments: 12,
            min_segmented_size: FAST_MIN_SEGMENTED_SIZE,
            target_segment_size: FAST_TARGET_SEGMENT_SIZE,
            low_speed_threshold_bytes_per_second: 16 * 1024,
            low_speed_window: Duration::from_secs(15),
            max_low_speed_retries: 3,
            speed_smoothing_alpha: 0.25,
        },
    }
}

pub(super) fn plan_segmented_ranges(
    total_bytes: u64,
    resume_support: ResumeSupport,
    speed_limit: Option<u64>,
    profile: DownloadPerformanceProfile,
) -> Option<RangePlan> {
    if speed_limit.is_some()
        || resume_support != ResumeSupport::Supported
        || total_bytes < profile.min_segmented_size
        || profile.max_segments < 2
    {
        return None;
    }

    let target_segment_size = profile.target_segment_size.max(1);
    let segment_count = profile
        .max_segments
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
    let response = send_range_request(client, url, ByteRange { start: 0, end: 0 }, handoff_auth)
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
    }
}

pub(super) fn segmented_error_allows_single_stream_fallback(error: &DownloadError) -> bool {
    error.category == FailureCategory::Resume
}

pub(super) async fn run_segmented_download_attempt(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
    client: Client,
    plan: RangePlan,
    profile: DownloadPerformanceProfile,
) -> Result<DownloadOutcome, DownloadError> {
    let mut segment_state = load_or_create_segment_state(&task.temp_path, &plan).await?;
    refresh_segment_completion_from_disk(&task.temp_path, &mut segment_state).await;
    persist_segment_state(&task.temp_path, &segment_state).await?;
    prepare_direct_segment_file(&task.temp_path, plan.total_bytes).await?;

    let initial_segment_bytes = segment_state
        .segments
        .iter()
        .map(|segment| segment_existing_len(&task.temp_path, segment))
        .collect::<Vec<_>>();
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
    let reporter_handle = tauri::async_runtime::spawn(report_segmented_progress(
        app.clone(),
        state.clone(),
        task.id.clone(),
        plan.total_bytes,
        profile,
        progress.clone(),
        reporter_stop.clone(),
    ));
    let metadata = Arc::new(Mutex::new(segment_state));
    let worker_context = SegmentWorkerContext {
        state: state.clone(),
        client: client.clone(),
        job_id: task.id.clone(),
        url: task.url.clone(),
        handoff_auth: task.handoff_auth.clone(),
        temp_path: task.temp_path.clone(),
        total_bytes: plan.total_bytes,
        profile,
        progress: progress.clone(),
        metadata: metadata.clone(),
    };

    let mut handles = Vec::new();
    for segment in plan.segments.iter().copied().enumerate() {
        let (index, range) = segment;
        if initial_segment_bytes[index] >= range.len() {
            continue;
        }

        handles.push(tauri::async_runtime::spawn(download_segment_worker(
            worker_context.clone(),
            SegmentProgress {
                index,
                range,
                downloaded_bytes: 0,
                completed: false,
            },
        )));
    }

    let mut worker_outcome = DownloadOutcome::Completed;
    let mut worker_error = None::<DownloadError>;
    while let Some(handle) = handles.pop() {
        match handle.await {
            Ok(Ok(DownloadOutcome::Completed)) => {}
            Ok(Ok(outcome @ (DownloadOutcome::Paused | DownloadOutcome::Canceled))) => {
                worker_outcome = outcome;
                for handle in handles {
                    handle.abort();
                }
                break;
            }
            Ok(Err(error)) => {
                worker_error = Some(error);
                for handle in handles {
                    handle.abort();
                }
                break;
            }
            Err(error) => {
                worker_error = Some(download_error(
                    FailureCategory::Internal,
                    format!("Segment worker failed: {error}"),
                    true,
                ));
                for handle in handles {
                    handle.abort();
                }
                break;
            }
        }
    }

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

pub(super) async fn download_segment_worker(
    context: SegmentWorkerContext,
    segment: SegmentProgress,
) -> Result<DownloadOutcome, DownloadError> {
    let mut current_len = segment_existing_len(&context.temp_path, &segment);
    let mut low_speed_monitor = LowSpeedMonitor::new(context.profile);
    let mut file = open_direct_segment_file(&context.temp_path).await?;
    let mut last_metadata_persisted_at = Instant::now();

    while current_len < segment.range.len() {
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
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                if error.category == FailureCategory::Resume {
                    range_backoffs().record_rejection(&context.url, Instant::now());
                }
                return Err(error);
            }
        };

        if response.status() != StatusCode::PARTIAL_CONTENT {
            range_backoffs().record_rejection(&context.url, Instant::now());
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
            return Err(download_error(
                FailureCategory::Resume,
                "The server returned an unexpected Content-Range for a segment.".into(),
                false,
            ));
        }

        let mut stream = response.bytes_stream();
        let mut low_speed_bytes = 0_u64;
        let mut low_speed_started = Instant::now();

        while let Some(chunk_result) = stream.next().await {
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
                if low_speed_monitor.observe(low_speed_bytes, low_speed_started.elapsed(), false)
                    == LowSpeedDecision::Retry
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
            }
        }

        if current_len >= segment.range.len() {
            mark_segment_completed(&context.temp_path, &context.metadata, segment.index).await?;
            return Ok(DownloadOutcome::Completed);
        }

        if low_speed_monitor.retries >= context.profile.max_low_speed_retries {
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
    job_id: String,
    total_bytes: u64,
    profile: DownloadPerformanceProfile,
    progress: Arc<SegmentedProgressCounters>,
    stop: Arc<AtomicBool>,
) -> Result<(), DownloadError> {
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

        if stopping {
            break;
        }
    }

    Ok(())
}

pub(super) async fn load_or_create_segment_state(
    temp_path: &Path,
    plan: &RangePlan,
) -> Result<SegmentedDownloadState, DownloadError> {
    let meta_path = segment_meta_path(temp_path);
    if let Ok(raw) = fs::read_to_string(&meta_path).await {
        if let Ok(state) = serde_json::from_str::<SegmentedDownloadState>(&raw) {
            let same_plan = state.total_bytes == plan.total_bytes
                && state.segments.len() == plan.segments.len()
                && state
                    .segments
                    .iter()
                    .zip(plan.segments.iter())
                    .all(|(stored, planned)| stored.range == *planned);
            if same_plan {
                return Ok(state);
            }
        }
    }

    cleanup_partial_artifacts(temp_path).await;
    Ok(SegmentedDownloadState {
        total_bytes: plan.total_bytes,
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

pub(super) async fn prepare_direct_segment_file(
    temp_path: &Path,
    total_bytes: u64,
) -> Result<(), DownloadError> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(temp_path)
        .await
        .map_err(|error| disk_error(format!("Could not create segmented partial file: {error}")))?;
    file.set_len(total_bytes)
        .await
        .map_err(|error| disk_error(format!("Could not size segmented partial file: {error}")))?;
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
    let state = {
        let mut metadata = metadata.lock().await;
        if let Some(segment) = metadata
            .segments
            .iter_mut()
            .find(|segment| segment.index == segment_index)
        {
            segment.downloaded_bytes = downloaded_bytes.min(segment.range.len());
            segment.completed = completed || segment.downloaded_bytes == segment.range.len();
        }
        metadata.clone()
    };

    if persist {
        persist_segment_state(temp_path, &state).await?;
    }

    Ok(())
}

pub(super) async fn persist_segment_state(
    temp_path: &Path,
    state: &SegmentedDownloadState,
) -> Result<(), DownloadError> {
    let serialized = serde_json::to_string_pretty(state)
        .map_err(|error| format!("Could not serialize segment metadata: {error}"))?;
    fs::write(segment_meta_path(temp_path), serialized)
        .await
        .map_err(|error| disk_error(format!("Could not write segment metadata: {error}")))
}

pub(super) async fn cleanup_segment_artifacts(temp_path: &Path, segment_count: usize) {
    let _ = fs::remove_file(segment_meta_path(temp_path)).await;
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

pub(super) fn segment_path(temp_path: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.seg{index}", temp_path.display()))
}
