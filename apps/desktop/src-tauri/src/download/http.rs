use super::*;

pub(super) async fn run_http_download_attempt(
    app: &AppHandle,
    state: &SharedState,
    task: &crate::state::DownloadTask,
) -> Result<DownloadOutcome, DownloadError> {
    ensure_parent_directory(&task.target_path)
        .await
        .map_err(disk_error)?;

    let mut existing_bytes = metadata_len(&task.temp_path).await.unwrap_or(0);
    let client = download_client()?;
    let speed_limit = state.speed_limit_bytes_per_second().await;
    let profile = performance_profile(state.download_performance_mode().await);

    let mut preflight_metadata =
        preflight_download(&client, &task.url, task.handoff_auth.as_ref()).await;

    let has_segment_state = segment_meta_path(&task.temp_path).exists();
    let can_try_segmented = (existing_bytes == 0 || has_segment_state)
        && speed_limit.is_none()
        && profile.max_segments >= 2
        && !range_backoffs().is_backed_off(&task.url, Instant::now());

    if can_try_segmented {
        let probe_metadata =
            probe_range_metadata(&client, &task.url, task.handoff_auth.as_ref()).await;
        let mut range_probe_supported = false;
        match probe_metadata {
            Some(metadata) => {
                range_probe_supported = true;
                preflight_metadata = Some(merge_preflight_metadata(preflight_metadata, metadata));
            }
            None => {
                range_backoffs().record_rejection(&task.url, Instant::now());
            }
        }

        if range_probe_supported {
            if let Some(metadata) = preflight_metadata.as_ref() {
                if let Some(total_bytes) = metadata.total_bytes {
                    if let Some(plan) = plan_segmented_ranges(
                        total_bytes,
                        metadata.resume_support,
                        speed_limit,
                        profile,
                    ) {
                        match run_segmented_download_attempt(
                            app,
                            state,
                            task,
                            client.clone(),
                            plan,
                            profile,
                            metadata.validators.clone(),
                        )
                        .await
                        {
                            Ok(outcome) => return Ok(outcome),
                            Err(error) if segmented_error_allows_single_stream_fallback(&error) => {
                                range_backoffs().record_rejection(&task.url, Instant::now());
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
        &task.url,
        existing_bytes,
        task.handoff_auth.as_ref(),
        preflight_metadata
            .as_ref()
            .map(|metadata| &metadata.validators),
    )
    .await?;
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
        response = send_request(&client, &task.url, 0, task.handoff_auth.as_ref(), None).await?;
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

        let chunk = chunk_result.map_err(download_stream_error)?;
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
        if low_speed_started.elapsed() >= profile.low_speed_window {
            if low_speed_monitor.observe(
                low_speed_bytes,
                low_speed_started.elapsed(),
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
    let output_kind = archive.output_kind;
    let archive_output_path = archive.output_path.display().to_string();
    let requires_extraction = match build_bulk_archive_source_plan(&archive.entries) {
        Ok(plan) => !plan.archive_sets.is_empty(),
        Err(error) => {
            let snapshot = state
                .mark_bulk_archive_status(
                    &archive_id,
                    BulkArchiveStatus::Failed,
                    None,
                    Some(archive_output_path.clone()),
                    Some(error.clone()),
                    None,
                )
                .await?;
            emit_snapshot(app, &snapshot);
            return Err(error);
        }
    };
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
            None,
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
                None,
            )
            .await?;
        emit_snapshot(app, &snapshot);
    }

    if output_kind == BulkArchiveOutputKind::Archive {
        let snapshot = state
            .mark_bulk_archive_status(
                &archive_id,
                BulkArchiveStatus::Compressing,
                Some(requires_extraction),
                Some(archive_output_path.clone()),
                None,
                None,
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
