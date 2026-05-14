use super::*;

pub(super) fn torrent_pause_should_release_engine_session(update: &TorrentRuntimeSnapshot) -> bool {
    update.finished
}

pub(super) fn prepare_torrent_source_for_task(
    task: &crate::state::DownloadTask,
    app_data_dir: &Path,
) -> PreparedTorrentSource {
    let cached_source = cached_torrent_metadata_source(
        app_data_dir,
        task.torrent
            .as_ref()
            .and_then(|torrent| torrent.info_hash.as_deref()),
    );
    prepare_torrent_source(cached_source.as_deref().unwrap_or(&task.url))
}

pub(super) async fn run_torrent_download_attempt<A: DownloadUi>(
    app: &A,
    state: &SharedState,
    task: &crate::state::DownloadTask,
) -> Result<DownloadOutcome, DownloadError> {
    let settings = state.settings().await;
    if !settings.torrent.enabled {
        return Err(download_error(
            FailureCategory::Torrent,
            "Torrent downloads are disabled in settings.".into(),
            false,
        ));
    }

    let mut engine = torrent_engine(state)
        .await
        .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
    if let Some(message) = engine.take_listener_fallback_message() {
        let _ = state
            .record_diagnostic_event(
                DiagnosticLevel::Warning,
                "torrent",
                message,
                Some(task.id.clone()),
            )
            .await;
    }
    let app_data_dir = state.app_data_dir();

    let mut output_folder = task.target_path.clone();
    let existing_torrent = task.torrent.as_ref();
    let stale_verified_torrent = is_stale_verified_torrent_task(task);
    if stale_verified_torrent {
        let info_hash = existing_torrent.and_then(|torrent| torrent.info_hash.clone());
        let _ = state
            .record_diagnostic_event(
                DiagnosticLevel::Warning,
                "torrent",
                stale_torrent_verified_recheck_message(),
                Some(task.id.clone()),
            )
            .await;
        if let Some(torrent) = existing_torrent {
            engine
                .forget_existing(torrent.engine_id, torrent.info_hash.as_deref())
                .await
                .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
        }
        let snapshot = state
            .reset_stale_torrent_completion_for_recheck(&task.id, info_hash)
            .await
            .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
        emit_snapshot(app, &snapshot);
    }

    let existing_torrent_for_resume = if stale_verified_torrent {
        None
    } else {
        existing_torrent
    };
    let restoring_seeding = is_torrent_seeding_restore(existing_torrent_for_resume);
    if restoring_seeding {
        match protected_restore_payload_target(
            &output_folder,
            existing_torrent_for_resume,
            &task.filename,
        ) {
            TorrentRestoreTarget::Current => {}
            TorrentRestoreTarget::Repaired(repaired) => {
                let previous_output_folder = output_folder.clone();
                if let Some(torrent) = existing_torrent_for_resume {
                    engine
                        .forget_existing(torrent.engine_id, torrent.info_hash.as_deref())
                        .await
                        .map_err(|message| {
                            download_error(FailureCategory::Torrent, message, false)
                        })?;
                }
                output_folder = repaired;
                cleanup_empty_generated_torrent_placeholder(
                    &previous_output_folder,
                    &output_folder,
                );
                let snapshot = state
                    .update_torrent_restore_target_path(&task.id, &output_folder)
                    .await
                    .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
                emit_snapshot(app, &snapshot);
            }
            TorrentRestoreTarget::Missing => {
                return Err(download_error(
                    FailureCategory::Torrent,
                    torrent_restore_payload_missing_message().into(),
                    false,
                ));
            }
        }
    }
    let mut prepared_source_for_recheck = None::<PreparedTorrentSource>;
    let mut stale_completion_recheck_attempted = stale_verified_torrent;
    let mut engine_id = match engine
        .resume_existing(
            existing_torrent_for_resume.and_then(|torrent| torrent.engine_id),
            existing_torrent_for_resume.and_then(|torrent| torrent.info_hash.as_deref()),
            settings.torrent.upload_limit_kib_per_second,
            restoring_seeding,
        )
        .await
        .map_err(|message| download_error(FailureCategory::Torrent, message, false))?
    {
        Some(engine_id) => {
            let _ = state
                .record_diagnostic_event(
                    DiagnosticLevel::Info,
                    "torrent",
                    if restoring_seeding {
                        torrent_restore_existing_seeding_session_message()
                    } else {
                        torrent_resume_existing_session_message()
                    },
                    Some(task.id.clone()),
                )
                .await;
            engine_id
        }
        None => {
            if restoring_seeding {
                let _ = state
                    .record_diagnostic_event(
                        DiagnosticLevel::Info,
                        "torrent",
                        torrent_restore_recheck_existing_files_message(),
                        Some(task.id.clone()),
                    )
                    .await;
            } else if torrent_has_resume_identity(existing_torrent_for_resume) {
                let _ = state
                    .record_diagnostic_event(
                        DiagnosticLevel::Info,
                        "torrent",
                        torrent_readd_for_verification_message(),
                        Some(task.id.clone()),
                    )
                    .await;
            }
            let prepared_source = prepare_torrent_source_for_task(task, &app_data_dir);
            let pending_cleanup_info_hash = pending_torrent_cleanup_info_hash(&prepared_source);
            prepared_source_for_recheck = Some(prepared_source.clone());
            if prepared_source.fallback_trackers_added > 0 {
                record_fallback_tracker_usage(
                    state,
                    &task.id,
                    prepared_source.fallback_trackers_added,
                    prepared_source.source_kind.label(),
                )
                .await;
            }
            let _ = state
                .record_diagnostic_event(
                    DiagnosticLevel::Info,
                    "torrent",
                    "Finding torrent metadata",
                    Some(task.id.clone()),
                )
                .await;
            let add_outcome = add_torrent_metadata_with_recovery(TorrentMetadataRecoveryRequest {
                state,
                job_id: &task.id,
                engine: &mut engine,
                prepared_source: &prepared_source,
                pending_cleanup_info_hash: pending_cleanup_info_hash.as_deref(),
                output_folder: &output_folder,
                upload_limit_kib_per_second: settings.torrent.upload_limit_kib_per_second,
                start_paused: restoring_seeding,
            })
            .await?;
            let mut add_session = match add_outcome {
                TorrentAddOutcome::Added(outcome) => outcome,
                TorrentAddOutcome::Interrupted(outcome) => {
                    return Ok(outcome);
                }
            };

            if should_readd_fresh_reused_session(
                existing_torrent_for_resume,
                &prepared_source,
                add_session,
            ) {
                let _ = state
                    .record_diagnostic_event(
                        DiagnosticLevel::Warning,
                        "torrent",
                        fresh_reused_torrent_session_recheck_message(),
                        Some(task.id.clone()),
                    )
                    .await;
                forget_stale_torrent_session(
                    engine.as_ref(),
                    add_session.engine_id,
                    pending_cleanup_info_hash.as_deref(),
                )
                .await
                .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
                stale_completion_recheck_attempted = true;

                let readd_outcome = add_prepared_torrent_with_controls(PreparedTorrentAddRequest {
                    state,
                    job_id: &task.id,
                    engine: engine.as_ref(),
                    prepared_source: &prepared_source,
                    output_folder: &output_folder,
                    upload_limit_kib_per_second: settings.torrent.upload_limit_kib_per_second,
                    start_paused: false,
                    metadata_timeout: TORRENT_METADATA_TIMEOUT,
                    tracker_first_diagnostics: None,
                })
                .await?;
                add_session = match readd_outcome {
                    TorrentAddOutcome::Added(outcome) => outcome,
                    TorrentAddOutcome::Interrupted(outcome) => return Ok(outcome),
                };
            }

            add_session.engine_id
        }
    };
    let _ = state
        .record_diagnostic_event(
            DiagnosticLevel::Info,
            "torrent",
            "Torrent metadata resolved",
            Some(task.id.clone()),
        )
        .await;

    let mut seeding_started = None::<Instant>;
    let mut persisted_seeding_started_at =
        existing_torrent.and_then(|torrent| torrent.seeding_started_at);
    let mut was_finished = persisted_seeding_started_at.is_some();
    let mut first_snapshot = true;
    let mut last_snapshot_job_state = JobState::Starting;
    let mut last_persisted_at = Instant::now();
    let mut restored_seeding_unpaused = false;
    let mut low_throughput_monitor = TorrentLowThroughputMonitor::default();
    let mut restore_watchdog =
        restoring_seeding.then(|| TorrentRestoreWatchdog::new(Instant::now()));
    let mut peer_connection_watchdog = TorrentPeerConnectionWatchdog::new(
        settings.torrent.peer_connection_watchdog_mode,
        Instant::now(),
    );
    loop {
        match state.worker_control(&task.id).await {
            WorkerControl::Paused => {
                let final_update = persist_final_torrent_snapshot_before_pause(
                    app,
                    state,
                    engine.as_ref(),
                    engine_id,
                    &task.id,
                )
                .await?;
                if final_update
                    .as_ref()
                    .is_some_and(torrent_pause_should_release_engine_session)
                {
                    if let Err(message) = engine.cache_metadata(engine_id, &app_data_dir).await {
                        let _ = state
                            .record_diagnostic_event(
                                DiagnosticLevel::Warning,
                                "torrent",
                                format!("Could not cache torrent metadata before pause: {message}"),
                                Some(task.id.clone()),
                            )
                            .await;
                    }
                    engine.forget(engine_id).await.map_err(|message| {
                        download_error(FailureCategory::Torrent, message, false)
                    })?;
                    let snapshot = state
                        .mark_torrent_engine_session_released(&task.id)
                        .await
                        .map_err(|message| {
                            download_error(FailureCategory::Torrent, message, false)
                        })?;
                    emit_snapshot(app, &snapshot);
                    return Ok(DownloadOutcome::Paused);
                }
                engine
                    .pause(engine_id)
                    .await
                    .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
                return Ok(DownloadOutcome::Paused);
            }
            WorkerControl::Canceled | WorkerControl::Missing => {
                engine
                    .forget(engine_id)
                    .await
                    .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
                return Ok(DownloadOutcome::Canceled);
            }
            WorkerControl::Continue => {}
        }

        let update = engine
            .snapshot(engine_id)
            .await
            .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
        if let Some(error) = update.error.clone() {
            return Err(download_error(FailureCategory::Torrent, error, false));
        }
        if let Some(prepared_source) = prepared_source_for_recheck.as_ref() {
            if is_stale_torrent_completion(
                prepared_source.source_kind,
                first_snapshot,
                &update,
                &output_folder,
            ) {
                let message = if stale_completion_recheck_attempted {
                    repeated_stale_torrent_completion_message()
                } else {
                    stale_torrent_completion_recheck_message()
                };
                let _ = state
                    .record_diagnostic_event(
                        DiagnosticLevel::Warning,
                        "torrent",
                        message,
                        Some(task.id.clone()),
                    )
                    .await;

                if stale_completion_recheck_attempted {
                    return Err(download_error(
                        FailureCategory::Torrent,
                        message.into(),
                        false,
                    ));
                }

                forget_stale_torrent_session(engine.as_ref(), engine_id, Some(&update.info_hash))
                    .await
                    .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
                let snapshot = state
                    .reset_stale_torrent_completion_for_recheck(
                        &task.id,
                        Some(update.info_hash.clone()),
                    )
                    .await
                    .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
                emit_snapshot(app, &snapshot);

                let add_outcome = add_prepared_torrent_with_controls(PreparedTorrentAddRequest {
                    state,
                    job_id: &task.id,
                    engine: engine.as_ref(),
                    prepared_source,
                    output_folder: &output_folder,
                    upload_limit_kib_per_second: settings.torrent.upload_limit_kib_per_second,
                    start_paused: false,
                    metadata_timeout: TORRENT_METADATA_TIMEOUT,
                    tracker_first_diagnostics: None,
                })
                .await?;
                match add_outcome {
                    TorrentAddOutcome::Added(outcome) => {
                        engine_id = outcome.engine_id;
                        stale_completion_recheck_attempted = true;
                        first_snapshot = true;
                        seeding_started = None;
                        persisted_seeding_started_at = None;
                        was_finished = false;
                        continue;
                    }
                    TorrentAddOutcome::Interrupted(outcome) => return Ok(outcome),
                }
            }
        }
        if torrent_seeding_payload_disappeared(&update, &output_folder) {
            let message = torrent_seeding_payload_disappeared_message();
            engine
                .forget(engine_id)
                .await
                .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
            cleanup_empty_torrent_output_folder(&output_folder);
            let snapshot = state
                .pause_torrent_payload_disappeared(&task.id, message)
                .await
                .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
            emit_snapshot(app, &snapshot);
            return Ok(DownloadOutcome::Paused);
        }
        let now = Instant::now();
        if restoring_seeding {
            if let Some(message) = torrent_restore_validation_failure(&update) {
                let _ = state
                    .record_diagnostic_event(
                        DiagnosticLevel::Warning,
                        "torrent",
                        message,
                        Some(task.id.clone()),
                    )
                    .await;
                return Err(download_error(
                    FailureCategory::Torrent,
                    message.into(),
                    false,
                ));
            }

            if let Some(watchdog) = restore_watchdog.as_mut() {
                match watchdog.observe(&update, now) {
                    TorrentRestoreWatchdogDecision::Continue => {}
                    TorrentRestoreWatchdogDecision::Recheck => {
                        forget_stale_torrent_session(
                            engine.as_ref(),
                            engine_id,
                            Some(&update.info_hash),
                        )
                        .await
                        .map_err(|message| {
                            download_error(FailureCategory::Torrent, message, false)
                        })?;
                        let snapshot = state
                            .reset_torrent_restore_runtime_for_recheck(
                                &task.id,
                                Some(update.info_hash.clone()),
                            )
                            .await
                            .map_err(|message| {
                                download_error(FailureCategory::Torrent, message, false)
                            })?;
                        emit_snapshot(app, &snapshot);

                        let prepared_source = prepare_torrent_source_for_task(task, &app_data_dir);
                        if prepared_source.fallback_trackers_added > 0 {
                            record_fallback_tracker_usage(
                                state,
                                &task.id,
                                prepared_source.fallback_trackers_added,
                                prepared_source.source_kind.label(),
                            )
                            .await;
                        }
                        let readd_outcome =
                            add_prepared_torrent_with_controls(PreparedTorrentAddRequest {
                                state,
                                job_id: &task.id,
                                engine: engine.as_ref(),
                                prepared_source: &prepared_source,
                                output_folder: &output_folder,
                                upload_limit_kib_per_second: settings
                                    .torrent
                                    .upload_limit_kib_per_second,
                                start_paused: false,
                                metadata_timeout: TORRENT_METADATA_TIMEOUT,
                                tracker_first_diagnostics: None,
                            })
                            .await?;
                        match readd_outcome {
                            TorrentAddOutcome::Added(outcome) => {
                                engine_id = outcome.engine_id;
                                prepared_source_for_recheck = Some(prepared_source);
                                first_snapshot = true;
                                restored_seeding_unpaused = false;
                                continue;
                            }
                            TorrentAddOutcome::Interrupted(outcome) => return Ok(outcome),
                        }
                    }
                    TorrentRestoreWatchdogDecision::Stalled => {
                        let message = torrent_restore_validation_stalled_message();
                        let _ = state
                            .record_diagnostic_event(
                                DiagnosticLevel::Warning,
                                "torrent",
                                message,
                                Some(task.id.clone()),
                            )
                            .await;
                        engine.forget(engine_id).await.map_err(|message| {
                            download_error(FailureCategory::Torrent, message, false)
                        })?;
                        return Err(download_error(
                            FailureCategory::Torrent,
                            message.into(),
                            false,
                        ));
                    }
                }
            }

            if update.finished
                && !restored_seeding_unpaused
                && matches!(update.phase, TorrentRuntimePhase::Paused)
            {
                engine
                    .unpause(engine_id)
                    .await
                    .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
                restored_seeding_unpaused = true;
            }
        }
        if low_throughput_monitor.should_report(&update, now) {
            let _ = state
                .record_diagnostic_event(
                    DiagnosticLevel::Warning,
                    "torrent",
                    torrent_low_throughput_message(&update),
                    Some(task.id.clone()),
                )
                .await;
        }
        if !restoring_seeding {
            match peer_connection_watchdog.observe(&update, now) {
                TorrentPeerConnectionWatchdogDecision::Continue => {}
                TorrentPeerConnectionWatchdogDecision::Report => {
                    let _ = state
                        .record_diagnostic_event(
                            DiagnosticLevel::Warning,
                            "torrent",
                            format!(
                                "Torrent peer watchdog diagnostic: {}",
                                torrent_low_throughput_message(&update)
                            ),
                            Some(task.id.clone()),
                        )
                        .await;
                }
                TorrentPeerConnectionWatchdogDecision::RefreshPeers => {
                    let _ = state
                        .record_diagnostic_event(
                            DiagnosticLevel::Warning,
                            "torrent",
                            format!(
                                "Recover peer watchdog refreshing peer connections: {}",
                                torrent_low_throughput_message(&update)
                            ),
                            Some(task.id.clone()),
                        )
                        .await;
                    engine.pause(engine_id).await.map_err(|message| {
                        download_error(FailureCategory::Torrent, message, false)
                    })?;
                    engine.unpause(engine_id).await.map_err(|message| {
                        download_error(FailureCategory::Torrent, message, false)
                    })?;
                    low_throughput_monitor = TorrentLowThroughputMonitor::default();
                    continue;
                }
                TorrentPeerConnectionWatchdogDecision::ReaddTorrent => {
                    let _ = state
                        .record_diagnostic_event(
                            DiagnosticLevel::Warning,
                            "torrent",
                            format!(
                                "Recover peer watchdog re-adding torrent session without deleting files: {}",
                                torrent_low_throughput_message(&update)
                            ),
                            Some(task.id.clone()),
                        )
                        .await;
                    forget_stale_torrent_session(
                        engine.as_ref(),
                        engine_id,
                        Some(&update.info_hash),
                    )
                    .await
                    .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
                    let prepared_source = prepare_torrent_source_for_task(task, &app_data_dir);
                    if prepared_source.fallback_trackers_added > 0 {
                        record_fallback_tracker_usage(
                            state,
                            &task.id,
                            prepared_source.fallback_trackers_added,
                            prepared_source.source_kind.label(),
                        )
                        .await;
                    }
                    let readd_outcome =
                        add_prepared_torrent_with_controls(PreparedTorrentAddRequest {
                            state,
                            job_id: &task.id,
                            engine: engine.as_ref(),
                            prepared_source: &prepared_source,
                            output_folder: &output_folder,
                            upload_limit_kib_per_second: settings
                                .torrent
                                .upload_limit_kib_per_second,
                            start_paused: false,
                            metadata_timeout: TORRENT_METADATA_TIMEOUT,
                            tracker_first_diagnostics: None,
                        })
                        .await?;
                    match readd_outcome {
                        TorrentAddOutcome::Added(outcome) => {
                            engine_id = outcome.engine_id;
                            prepared_source_for_recheck = Some(prepared_source);
                            first_snapshot = true;
                            low_throughput_monitor = TorrentLowThroughputMonitor::default();
                            continue;
                        }
                        TorrentAddOutcome::Interrupted(outcome) => return Ok(outcome),
                    }
                }
                TorrentPeerConnectionWatchdogDecision::ResetEngine => {
                    let reset = clear_in_memory_torrent_engine_if_no_other_work(state, &task.id)
                        .await
                        .map_err(|message| {
                            download_error(FailureCategory::Torrent, message, false)
                        })?;
                    if !reset {
                        let _ = state
                            .record_diagnostic_event(
                                DiagnosticLevel::Warning,
                                "torrent",
                                "Recover peer watchdog skipped engine reset because another torrent is active",
                                Some(task.id.clone()),
                            )
                            .await;
                        low_throughput_monitor = TorrentLowThroughputMonitor::default();
                        peer_connection_watchdog.rearm_engine_reset();
                        continue;
                    }

                    let _ = state
                        .record_diagnostic_event(
                            DiagnosticLevel::Warning,
                            "torrent",
                            format!(
                                "Recover peer watchdog resetting in-memory engine without deleting files: {}",
                                torrent_low_throughput_message(&update)
                            ),
                            Some(task.id.clone()),
                        )
                        .await;
                    engine = torrent_engine(state).await.map_err(|message| {
                        download_error(FailureCategory::Torrent, message, false)
                    })?;
                    let prepared_source = prepare_torrent_source_for_task(task, &app_data_dir);
                    if prepared_source.fallback_trackers_added > 0 {
                        record_fallback_tracker_usage(
                            state,
                            &task.id,
                            prepared_source.fallback_trackers_added,
                            prepared_source.source_kind.label(),
                        )
                        .await;
                    }
                    let readd_outcome =
                        add_prepared_torrent_with_controls(PreparedTorrentAddRequest {
                            state,
                            job_id: &task.id,
                            engine: engine.as_ref(),
                            prepared_source: &prepared_source,
                            output_folder: &output_folder,
                            upload_limit_kib_per_second: settings
                                .torrent
                                .upload_limit_kib_per_second,
                            start_paused: false,
                            metadata_timeout: TORRENT_METADATA_TIMEOUT,
                            tracker_first_diagnostics: None,
                        })
                        .await?;
                    match readd_outcome {
                        TorrentAddOutcome::Added(outcome) => {
                            engine_id = outcome.engine_id;
                            prepared_source_for_recheck = Some(prepared_source);
                            first_snapshot = true;
                            low_throughput_monitor = TorrentLowThroughputMonitor::default();
                            continue;
                        }
                        TorrentAddOutcome::Interrupted(outcome) => return Ok(outcome),
                    }
                }
            }
        }

        let started_seeding = update.finished && !was_finished;
        let should_persist = torrent_progress_should_persist(
            first_snapshot,
            started_seeding,
            false,
            last_persisted_at,
            now,
        );
        if should_persist {
            last_persisted_at = now;
        }

        let delta = state
            .update_torrent_progress_delta(&task.id, update.clone(), should_persist)
            .await
            .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
        emit_progress_delta(app, delta.clone());
        let next_snapshot_job_state = delta.job.state;
        let released_download_slot = seeding_transition_releases_download_slot(
            last_snapshot_job_state,
            next_snapshot_job_state,
        );
        last_snapshot_job_state = next_snapshot_job_state;
        if released_download_slot {
            schedule_downloads(app.clone(), state.clone());
        }
        first_snapshot = false;

        if update.finished {
            let started = seeding_started.get_or_insert_with(Instant::now);
            if persisted_seeding_started_at.is_none() {
                persisted_seeding_started_at = delta
                    .job
                    .torrent
                    .as_ref()
                    .and_then(|torrent| torrent.seeding_started_at);
            }
            was_finished = true;
            let torrent_settings = state.settings().await.torrent;
            let torrent = delta.job.torrent.as_ref();
            let ratio = torrent_seed_ratio_for_policy(
                torrent,
                update.downloaded_bytes,
                update.uploaded_bytes,
            );
            let seed_elapsed = torrent_seed_elapsed_seconds(
                persisted_seeding_started_at,
                current_unix_timestamp_millis(),
                started.elapsed(),
            );
            if should_stop_seeding(&torrent_settings, ratio, seed_elapsed) {
                engine
                    .forget(engine_id)
                    .await
                    .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
                let snapshot = state
                    .complete_torrent_job(&task.id)
                    .await
                    .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
                emit_snapshot(app, &snapshot);
                notify_download_completed(app, state, &task.target_path, false).await;
                return Ok(DownloadOutcome::Completed);
            }
        }

        tokio::time::sleep(PROGRESS_UPDATE_INTERVAL).await;
    }
}

pub(super) async fn persist_final_torrent_snapshot_before_pause<A: DownloadUi>(
    app: &A,
    state: &SharedState,
    engine: &TorrentEngine,
    engine_id: usize,
    job_id: &str,
) -> Result<Option<TorrentRuntimeSnapshot>, DownloadError> {
    let update = match engine.snapshot(engine_id).await {
        Ok(update) => update,
        Err(message) => {
            let _ = state
                .record_diagnostic_event(
                    DiagnosticLevel::Warning,
                    "torrent",
                    format!("Could not capture final torrent snapshot before pause: {message}"),
                    Some(job_id.to_string()),
                )
                .await;
            return Ok(None);
        }
    };

    if let Some(message) = update.error.clone() {
        let _ = state
            .record_diagnostic_event(
                DiagnosticLevel::Warning,
                "torrent",
                format!("Final torrent snapshot before pause reported an engine error: {message}"),
                Some(job_id.to_string()),
            )
            .await;
    }

    let delta = state
        .update_torrent_progress_delta(job_id, update.clone(), true)
        .await
        .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
    emit_progress_delta(app, delta);
    Ok(Some(update))
}

pub(super) fn torrent_progress_should_persist(
    first_snapshot: bool,
    started_seeding: bool,
    stopping: bool,
    last_persisted_at: Instant,
    now: Instant,
) -> bool {
    first_snapshot
        || started_seeding
        || stopping
        || now.saturating_duration_since(last_persisted_at) >= PROGRESS_PERSIST_INTERVAL
}

pub(super) fn seeding_transition_releases_download_slot(
    previous: JobState,
    next: JobState,
) -> bool {
    next == JobState::Seeding && previous != JobState::Seeding
}

pub(super) fn torrent_seed_elapsed_seconds(
    persisted_started_at_millis: Option<u64>,
    now_millis: u64,
    local_elapsed: Duration,
) -> u64 {
    persisted_started_at_millis
        .map(|started_at| now_millis.saturating_sub(started_at) / 1000)
        .unwrap_or_else(|| local_elapsed.as_secs())
}

pub(super) fn torrent_seed_ratio_for_policy(
    torrent: Option<&TorrentInfo>,
    downloaded_bytes: u64,
    runtime_uploaded_bytes: u64,
) -> f64 {
    torrent
        .map(|torrent| torrent.ratio)
        .filter(|ratio| ratio.is_finite())
        .unwrap_or_else(|| {
            if downloaded_bytes == 0 {
                0.0
            } else {
                runtime_uploaded_bytes as f64 / downloaded_bytes as f64
            }
        })
}

pub(super) fn current_unix_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) async fn add_torrent_with_controls<F>(
    state: &SharedState,
    job_id: &str,
    add_torrent: F,
    metadata_timeout: Duration,
    control_interval: Duration,
) -> Result<TorrentAddOutcome, DownloadError>
where
    F: Future<Output = Result<TorrentAddSessionOutcome, String>>,
{
    tokio::pin!(add_torrent);
    let timeout = tokio::time::sleep(metadata_timeout);
    tokio::pin!(timeout);
    let mut control_tick = tokio::time::interval(control_interval);

    loop {
        tokio::select! {
            result = &mut add_torrent => {
                return match result {
                    Ok(outcome) => Ok(TorrentAddOutcome::Added(outcome)),
                    Err(message) => {
                        let _ = state
                            .record_diagnostic_event(
                                DiagnosticLevel::Error,
                                "torrent",
                                format!("Torrent add failed: {message}"),
                                Some(job_id.to_string()),
                            )
                            .await;
                        Err(download_error(FailureCategory::Torrent, message, false))
                    }
                };
            }
            _ = &mut timeout => {
                let message = torrent_metadata_timeout_message();
                let _ = state
                    .record_diagnostic_event(
                        DiagnosticLevel::Error,
                        "torrent",
                        message.clone(),
                        Some(job_id.to_string()),
                    )
                    .await;
                return Err(download_error(FailureCategory::Torrent, message, true));
            }
            _ = control_tick.tick() => {
                match state.worker_control(job_id).await {
                    WorkerControl::Paused => {
                        let _ = state
                            .record_diagnostic_event(
                                DiagnosticLevel::Info,
                                "torrent",
                                "Torrent metadata lookup paused",
                                Some(job_id.to_string()),
                            )
                            .await;
                        return Ok(TorrentAddOutcome::Interrupted(DownloadOutcome::Paused));
                    }
                    WorkerControl::Canceled | WorkerControl::Missing => {
                        let _ = state
                            .record_diagnostic_event(
                                DiagnosticLevel::Info,
                                "torrent",
                                "Torrent metadata lookup canceled",
                                Some(job_id.to_string()),
                            )
                            .await;
                        return Ok(TorrentAddOutcome::Interrupted(DownloadOutcome::Canceled));
                    }
                    WorkerControl::Continue => {}
                }
            }
        }
    }
}

pub(super) struct TorrentMetadataRecoveryRequest<'a> {
    state: &'a SharedState,
    job_id: &'a str,
    engine: &'a mut Arc<TorrentEngine>,
    prepared_source: &'a PreparedTorrentSource,
    pending_cleanup_info_hash: Option<&'a str>,
    output_folder: &'a Path,
    upload_limit_kib_per_second: u32,
    start_paused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TorrentMetadataRecoveryStage {
    pub(super) use_tracker_first: bool,
    pub(super) reset_engine_before_retry: bool,
    pub(super) timeout: Duration,
    pub(super) is_final_failure: bool,
}

pub(super) fn torrent_metadata_recovery_stage(attempt: usize) -> TorrentMetadataRecoveryStage {
    match attempt {
        0 => TorrentMetadataRecoveryStage {
            use_tracker_first: true,
            reset_engine_before_retry: false,
            timeout: TORRENT_METADATA_TIMEOUT,
            is_final_failure: false,
        },
        1 => TorrentMetadataRecoveryStage {
            use_tracker_first: false,
            reset_engine_before_retry: false,
            timeout: TORRENT_METADATA_TIMEOUT,
            is_final_failure: false,
        },
        2 => TorrentMetadataRecoveryStage {
            use_tracker_first: false,
            reset_engine_before_retry: true,
            timeout: TORRENT_METADATA_TIMEOUT,
            is_final_failure: false,
        },
        _ => TorrentMetadataRecoveryStage {
            use_tracker_first: false,
            reset_engine_before_retry: false,
            timeout: TORRENT_METADATA_TIMEOUT,
            is_final_failure: true,
        },
    }
}

pub(super) async fn add_torrent_metadata_with_recovery(
    request: TorrentMetadataRecoveryRequest<'_>,
) -> Result<TorrentAddOutcome, DownloadError> {
    let mut engine_reset_attempted = false;

    for attempt in 0.. {
        let stage = torrent_metadata_recovery_stage(attempt);
        if stage.is_final_failure {
            let error = torrent_metadata_recovery_failure_error(engine_reset_attempted);
            let message = error.message.clone();
            let _ = request
                .state
                .record_diagnostic_event(
                    DiagnosticLevel::Error,
                    "torrent",
                    message.clone(),
                    Some(request.job_id.to_string()),
                )
                .await;
            return Err(error);
        }

        if stage.reset_engine_before_retry {
            cleanup_pending_torrent_metadata(
                request.engine.as_ref(),
                request.state,
                request.job_id,
                request.pending_cleanup_info_hash,
            )
            .await;
            let reset =
                clear_in_memory_torrent_engine_if_no_other_work(request.state, request.job_id)
                    .await
                    .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
            if reset {
                engine_reset_attempted = true;
                let _ = request
                    .state
                    .record_diagnostic_event(
                        DiagnosticLevel::Warning,
                        "torrent",
                        "Resetting in-memory torrent engine before metadata retry",
                        Some(request.job_id.to_string()),
                    )
                    .await;
                *request.engine = torrent_engine(request.state)
                    .await
                    .map_err(|message| download_error(FailureCategory::Torrent, message, false))?;
            } else {
                let _ = request
                    .state
                    .record_diagnostic_event(
                        DiagnosticLevel::Warning,
                        "torrent",
                        "Skipped torrent engine reset before metadata retry because another torrent is active",
                        Some(request.job_id.to_string()),
                    )
                    .await;
            }
        }

        let mut staged_source = request.prepared_source.clone();
        staged_source.tracker_first_metadata =
            request.prepared_source.tracker_first_metadata && stage.use_tracker_first;
        let tracker_first_diagnostics = staged_source.tracker_first_metadata.then(|| {
            spawn_tracker_first_metadata_diagnostics(
                (*request.state).clone(),
                request.job_id.to_string(),
            )
        });
        if attempt > 0 {
            let _ = request
                .state
                .record_diagnostic_event(
                    DiagnosticLevel::Info,
                    "torrent",
                    format!(
                        "Retrying torrent metadata lookup after staged recovery attempt {attempt}"
                    ),
                    Some(request.job_id.to_string()),
                )
                .await;
        }

        let add_outcome = add_prepared_torrent_with_controls(PreparedTorrentAddRequest {
            state: request.state,
            job_id: request.job_id,
            engine: request.engine.as_ref(),
            prepared_source: &staged_source,
            output_folder: request.output_folder,
            upload_limit_kib_per_second: request.upload_limit_kib_per_second,
            start_paused: request.start_paused,
            metadata_timeout: stage.timeout,
            tracker_first_diagnostics,
        })
        .await;

        match add_outcome {
            Ok(TorrentAddOutcome::Interrupted(DownloadOutcome::Canceled)) => {
                cleanup_pending_torrent_metadata(
                    request.engine.as_ref(),
                    request.state,
                    request.job_id,
                    request.pending_cleanup_info_hash,
                )
                .await;
                return Ok(TorrentAddOutcome::Interrupted(DownloadOutcome::Canceled));
            }
            Ok(outcome) => return Ok(outcome),
            Err(error) if is_torrent_metadata_timeout_error(&error) => {
                cleanup_pending_torrent_metadata(
                    request.engine.as_ref(),
                    request.state,
                    request.job_id,
                    request.pending_cleanup_info_hash,
                )
                .await;
            }
            Err(error) => return Err(error),
        }
    }

    unreachable!("metadata recovery loop should return on final failure stage")
}

pub(super) fn torrent_metadata_recovery_failure_message(engine_reset_attempted: bool) -> String {
    if engine_reset_attempted {
        return "Torrent metadata lookup stalled after tracker, DHT, cleanup, and engine-reset recovery attempts. Add more trackers, import a .torrent file, or retry later."
            .into();
    }

    "Torrent metadata lookup stalled after tracker, DHT, and cleanup recovery attempts; the app could not reset the torrent engine because another torrent was active. Pause other torrents, add more trackers, import a .torrent file, or retry later."
        .into()
}

pub(super) fn torrent_metadata_recovery_failure_error(
    engine_reset_attempted: bool,
) -> DownloadError {
    download_error(
        FailureCategory::Torrent,
        torrent_metadata_recovery_failure_message(engine_reset_attempted),
        false,
    )
}

pub(super) struct PreparedTorrentAddRequest<'a> {
    state: &'a SharedState,
    job_id: &'a str,
    engine: &'a TorrentEngine,
    prepared_source: &'a PreparedTorrentSource,
    output_folder: &'a Path,
    upload_limit_kib_per_second: u32,
    start_paused: bool,
    metadata_timeout: Duration,
    tracker_first_diagnostics: Option<mpsc::UnboundedSender<TrackerFirstMetadataOutcome>>,
}

pub(super) async fn add_prepared_torrent_with_controls(
    request: PreparedTorrentAddRequest<'_>,
) -> Result<TorrentAddOutcome, DownloadError> {
    add_torrent_with_controls(
        request.state,
        request.job_id,
        request.engine.add_source(
            request.prepared_source,
            request.output_folder,
            request.upload_limit_kib_per_second,
            request.start_paused,
            request.tracker_first_diagnostics,
        ),
        request.metadata_timeout,
        TORRENT_METADATA_CONTROL_INTERVAL,
    )
    .await
}

pub(super) async fn record_fallback_tracker_usage(
    state: &SharedState,
    job_id: &str,
    count: usize,
    source_kind: &str,
) {
    let _ = state
        .record_diagnostic_event(
            DiagnosticLevel::Info,
            "torrent",
            format!("Added {count} fallback trackers for {source_kind} metadata lookup"),
            Some(job_id.to_string()),
        )
        .await;
}

pub(super) fn spawn_tracker_first_metadata_diagnostics(
    state: SharedState,
    job_id: String,
) -> mpsc::UnboundedSender<TrackerFirstMetadataOutcome> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tauri::async_runtime::spawn(async move {
        while let Some(outcome) = rx.recv().await {
            record_tracker_first_metadata_outcome(&state, &job_id, &outcome).await;
        }
    });
    tx
}

pub(super) async fn record_tracker_first_metadata_outcome(
    state: &SharedState,
    job_id: &str,
    outcome: &TrackerFirstMetadataOutcome,
) {
    let _ = state
        .record_diagnostic_event(
            DiagnosticLevel::Info,
            "torrent",
            tracker_first_metadata_diagnostic_message(outcome),
            Some(job_id.to_string()),
        )
        .await;
}

pub(super) fn tracker_first_metadata_diagnostic_message(
    outcome: &TrackerFirstMetadataOutcome,
) -> String {
    match outcome {
        TrackerFirstMetadataOutcome::Resolved => "Tracker-first torrent metadata resolved".into(),
        TrackerFirstMetadataOutcome::TimedOut => format!(
            "Tracker-first torrent metadata timed out after {} seconds; falling back to the main DHT session",
            crate::torrent::TORRENT_TRACKER_FIRST_METADATA_TIMEOUT.as_secs()
        ),
        TrackerFirstMetadataOutcome::Failed(message) => {
            format!(
                "Tracker-first torrent metadata failed; falling back to the main DHT session: {message}"
            )
        }
    }
}

pub(super) fn torrent_resume_existing_session_message() -> &'static str {
    "Resumed torrent from saved session"
}

pub(super) fn torrent_restore_existing_seeding_session_message() -> &'static str {
    "Restored torrent seeding from saved session"
}

pub(super) fn torrent_readd_for_verification_message() -> &'static str {
    "No saved torrent session found; re-adding torrent for piece verification"
}

pub(super) fn torrent_restore_recheck_existing_files_message() -> &'static str {
    "No saved seeding session found; rechecking existing files before seeding"
}

pub(super) fn fresh_reused_torrent_session_recheck_message() -> &'static str {
    "Fresh torrent matched an existing engine session; clearing stale verification and rechecking files"
}

pub(super) fn stale_torrent_completion_recheck_message() -> &'static str {
    "Torrent reported complete but the target folder is empty; clearing stale verification and rechecking files"
}

pub(super) fn stale_torrent_verified_recheck_message() -> &'static str {
    "Existing torrent seeding state has no payload files; clearing stale verification and rechecking files"
}

pub(super) fn repeated_stale_torrent_completion_message() -> &'static str {
    "Torrent verification still reports complete, but the target folder is empty after recheck. Clear the torrent and add it again, or choose a folder containing the files."
}

pub(super) fn torrent_restore_peer_download_blocked_message() -> &'static str {
    "Seeding restore started downloading from peers before local files validated complete; pausing to avoid an unintended redownload."
}

pub(super) fn torrent_restore_incomplete_payload_message() -> &'static str {
    "Seeding restore found incomplete local files; pausing instead of downloading them again. Use restart if you want to download this torrent again."
}

pub(super) fn torrent_restore_validation_stalled_message() -> &'static str {
    "Seeding restore validation made no progress after an automatic recheck; pausing instead of staying active forever."
}

pub(super) fn torrent_restore_payload_missing_message() -> &'static str {
    "Seeding restore could not find local payload files; pausing instead of downloading them again. Choose the folder containing the files or restart the torrent."
}

pub(super) fn torrent_seeding_payload_disappeared_message() -> &'static str {
    "Torrent payload files disappeared while seeding; stopping the torrent session so the folder is not recreated. Use restart if you want to download it again."
}

pub(super) fn torrent_has_resume_identity(torrent: Option<&TorrentInfo>) -> bool {
    torrent.is_some_and(|torrent| torrent.engine_id.is_some() || torrent.info_hash.is_some())
}

pub(super) fn is_torrent_seeding_restore(torrent: Option<&TorrentInfo>) -> bool {
    torrent.is_some_and(|torrent| torrent.seeding_started_at.is_some())
}

pub(super) fn is_torrent_seeding_restore_task(task: &crate::state::DownloadTask) -> bool {
    task.transfer_kind == TransferKind::Torrent
        && is_torrent_seeding_restore(task.torrent.as_ref())
        && !is_stale_verified_torrent_task(task)
}

pub(super) fn is_stale_verified_torrent_task(task: &crate::state::DownloadTask) -> bool {
    if task.transfer_kind != TransferKind::Torrent {
        return false;
    }
    if !task
        .url
        .get(..task.url.len().min("magnet:".len()))
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("magnet:"))
    {
        return false;
    }
    let Some(torrent) = task.torrent.as_ref() else {
        return false;
    };
    if torrent.seeding_started_at.is_none()
        || torrent.fetched_bytes > 0
        || torrent.uploaded_bytes > 0
    {
        return false;
    }

    target_payload_appears_empty(&task.target_path)
}

pub(super) fn should_readd_fresh_reused_session(
    torrent: Option<&TorrentInfo>,
    prepared_source: &PreparedTorrentSource,
    add_session: TorrentAddSessionOutcome,
) -> bool {
    prepared_source.source_kind == TorrentSourceKind::Magnet
        && add_session.reused_existing_session
        && !is_torrent_seeding_restore(torrent)
}

pub(super) fn is_stale_torrent_completion(
    source_kind: TorrentSourceKind,
    first_snapshot: bool,
    update: &crate::state::TorrentRuntimeSnapshot,
    target_path: &Path,
) -> bool {
    source_kind == TorrentSourceKind::Magnet
        && first_snapshot
        && update.finished
        && update.total_bytes > 0
        && update.downloaded_bytes >= update.total_bytes
        && update.fetched_bytes == 0
        && target_payload_appears_empty(target_path)
}

pub(super) fn torrent_restore_validation_failure(
    update: &crate::state::TorrentRuntimeSnapshot,
) -> Option<&'static str> {
    if update.finished {
        return None;
    }

    if update.fetched_bytes > 0 || update.download_speed > 0 {
        return Some(torrent_restore_peer_download_blocked_message());
    }

    if matches!(update.phase, TorrentRuntimePhase::Paused) && update.total_bytes > 0 {
        return Some(torrent_restore_incomplete_payload_message());
    }

    None
}

pub(super) fn torrent_seeding_payload_disappeared(
    update: &crate::state::TorrentRuntimeSnapshot,
    target_path: &Path,
) -> bool {
    update.finished
        && matches!(update.phase, TorrentRuntimePhase::Live)
        && update.total_bytes > 0
        && target_payload_appears_empty(target_path)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum TorrentRestoreTarget {
    Current,
    Repaired(PathBuf),
    Missing,
}

pub(super) fn protected_restore_payload_target(
    current_target: &Path,
    torrent: Option<&TorrentInfo>,
    fallback_name: &str,
) -> TorrentRestoreTarget {
    if !target_payload_appears_empty(current_target) {
        return TorrentRestoreTarget::Current;
    }

    let Some(parent) = current_target.parent() else {
        return TorrentRestoreTarget::Missing;
    };

    for name in torrent_restore_payload_candidate_names(torrent, fallback_name) {
        let candidate = parent.join(name);
        if candidate == current_target {
            continue;
        }
        if !target_payload_appears_empty(&candidate) {
            return TorrentRestoreTarget::Repaired(candidate);
        }
    }

    TorrentRestoreTarget::Missing
}

pub(super) fn cleanup_empty_generated_torrent_placeholder(
    previous_target: &Path,
    repaired_target: &Path,
) {
    if previous_target == repaired_target || !is_generated_torrent_placeholder(previous_target) {
        return;
    }

    cleanup_empty_torrent_output_folder(previous_target);
}

pub(super) fn cleanup_empty_torrent_output_folder(target_path: &Path) {
    let Ok(metadata) = std::fs::metadata(target_path) else {
        return;
    };
    if !metadata.is_dir() || !target_payload_appears_empty(target_path) {
        return;
    }

    let _ = std::fs::remove_dir(target_path);
}

pub(super) fn is_generated_torrent_placeholder(target_path: &Path) -> bool {
    let Some(name) = target_path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(hash) = name.strip_prefix("torrent-") else {
        return false;
    };

    !hash.is_empty() && hash.chars().all(|character| character.is_ascii_hexdigit())
}

pub(super) fn torrent_restore_payload_candidate_names(
    torrent: Option<&TorrentInfo>,
    fallback_name: &str,
) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = torrent.and_then(|torrent| torrent.name.as_deref()) {
        let candidate = sanitize_torrent_payload_name(name);
        if !candidate.is_empty() {
            names.push(candidate);
        }
    }

    let fallback = sanitize_torrent_payload_name(fallback_name);
    if !fallback.is_empty() && !names.iter().any(|name| name == &fallback) {
        names.push(fallback);
    }

    names
}

pub(super) fn sanitize_torrent_payload_name(input: &str) -> String {
    input
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            character if character.is_control() => '_',
            _ => character,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .trim()
        .to_string()
}

pub(super) fn target_payload_appears_empty(target_path: &Path) -> bool {
    let metadata = match std::fs::metadata(target_path) {
        Ok(metadata) => metadata,
        Err(error) => return error.kind() == std::io::ErrorKind::NotFound,
    };

    if metadata.is_file() {
        return metadata.len() == 0;
    }
    if !metadata.is_dir() {
        return true;
    }

    let mut pending = vec![target_path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            return false;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                return false;
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() && metadata.len() > 0 {
                return false;
            }
        }
    }

    true
}

pub(super) async fn forget_stale_torrent_session(
    engine: &TorrentEngine,
    engine_id: usize,
    info_hash: Option<&str>,
) -> Result<(), String> {
    if let Some(info_hash) = info_hash {
        if engine.forget_by_info_hash(info_hash).await? {
            return Ok(());
        }
    }

    engine.forget(engine_id).await
}

pub(super) fn torrent_metadata_timeout_message() -> String {
    format!(
        "Torrent metadata lookup timed out after {} seconds. Add trackers or retry later.",
        TORRENT_METADATA_TIMEOUT.as_secs()
    )
}

pub(super) fn is_torrent_metadata_timeout_error(error: &DownloadError) -> bool {
    error.category == FailureCategory::Torrent
        && error
            .message
            .starts_with("Torrent metadata lookup timed out after ")
}

pub(super) async fn cleanup_pending_torrent_metadata(
    engine: &TorrentEngine,
    state: &SharedState,
    job_id: &str,
    info_hash: Option<&str>,
) {
    let Some(info_hash) = info_hash else {
        return;
    };

    match engine.forget_by_info_hash(info_hash).await {
        Ok(true) => {
            let _ = state
                .record_diagnostic_event(
                    DiagnosticLevel::Info,
                    "torrent",
                    "Cleaned up pending torrent metadata session",
                    Some(job_id.to_string()),
                )
                .await;
        }
        Ok(false) => {}
        Err(message) => {
            let _ = state
                .record_diagnostic_event(
                    DiagnosticLevel::Warning,
                    "torrent",
                    format!("Could not clean up pending torrent metadata session: {message}"),
                    Some(job_id.to_string()),
                )
                .await;
        }
    }
}

pub(super) async fn torrent_engine(state: &SharedState) -> Result<Arc<TorrentEngine>, String> {
    managed_torrent_engine(state).await
}
