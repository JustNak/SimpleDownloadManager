use super::*;

#[test]
fn balanced_range_plan_uses_target_size_and_caps_at_six_segments() {
    let profile = performance_profile(DownloadPerformanceMode::Balanced);
    let minimum_plan =
        plan_segmented_ranges(32 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("balanced mode should segment range-capable files at 32 MiB");
    let capped_plan =
        plan_segmented_ranges(512 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("large range-capable files should use segmented downloading");
    let plan = plan_segmented_ranges(256 * 1024 * 1024, ResumeSupport::Supported, None, profile)
        .expect("large range-capable files should use segmented downloading");

    assert_eq!(minimum_plan.segments.len(), 2);
    assert_eq!(plan.segments.len(), 4);
    assert_eq!(capped_plan.segments.len(), 6);
    assert_eq!(
        plan.segments[0],
        ByteRange {
            start: 0,
            end: 67_108_863
        }
    );
    assert_eq!(
        plan.segments[3],
        ByteRange {
            start: 201_326_592,
            end: 268_435_455,
        }
    );
}

#[test]
fn fast_profile_uses_fast_plus_initial_and_adaptive_segment_caps() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    assert_eq!(profile.initial_segments, 16);
    assert_eq!(profile.soft_max_segments, 32);
    assert_eq!(profile.max_segments, 64);
    assert_eq!(profile.target_segment_size, 8 * 1024 * 1024);
    assert_eq!(profile.adaptive_ramp_step, 4);
    assert_eq!(profile.adaptive_ramp_interval, Duration::from_secs(2));
}

#[test]
fn fast_range_plan_uses_fast_plus_initial_fanout() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let minimum_plan =
        plan_segmented_ranges(16 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("fast mode should segment range-capable files at 16 MiB");
    let ramp_plan =
        plan_segmented_ranges(256 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("large fast downloads should immediately fan out across the fast initial cap");
    let capped_plan =
        plan_segmented_ranges(1024 * 1024 * 1024, ResumeSupport::Supported, None, profile)
            .expect("large fast downloads should use capped segmented downloading");

    assert_eq!(minimum_plan.segments.len(), 2);
    assert_eq!(ramp_plan.segments.len(), 16);
    assert_eq!(capped_plan.segments.len(), 16);
}

#[test]
fn fast_dynamic_queue_depth_keeps_ranges_long_lived_past_mid_download() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);

    assert_eq!(dynamic_segment_queue_depth(profile), 80);
}

#[test]
fn fast_tail_lease_size_uses_remaining_byte_buckets() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let mib = 1024 * 1024;
    let gib = 1024 * mib;

    assert_eq!(
        dynamic_segment_tail_lease_size(2 * gib, profile),
        Some(32 * mib)
    );
    assert_eq!(dynamic_segment_tail_lease_size(gib, profile), Some(8 * mib));
    assert_eq!(
        dynamic_segment_tail_lease_size(256 * mib, profile),
        Some(8 * mib)
    );
    assert_eq!(
        dynamic_segment_tail_lease_size(255 * mib, profile),
        Some(mib)
    );
    assert_eq!(
        dynamic_segment_tail_lease_size(
            2 * gib,
            performance_profile(DownloadPerformanceMode::Balanced)
        ),
        None
    );
}

#[test]
fn capped_fast_profiles_keep_tail_leasing_above_rescue_floor() {
    let mut capped_fast = performance_profile(DownloadPerformanceMode::Fast);
    capped_fast.max_segments = 24;
    capped_fast.soft_max_segments = 24;

    let mut heavily_capped_fast = capped_fast;
    heavily_capped_fast.max_segments = 16;
    heavily_capped_fast.soft_max_segments = 16;

    assert_eq!(
        dynamic_segment_tail_lease_size(2 * 1024 * 1024 * 1024, capped_fast),
        Some(32 * 1024 * 1024)
    );
    assert_eq!(
        dynamic_segment_tail_lease_size(2 * 1024 * 1024 * 1024, heavily_capped_fast),
        None
    );
    assert_eq!(
        dynamic_segment_tail_lease_size(
            2 * 1024 * 1024 * 1024,
            performance_profile(DownloadPerformanceMode::Balanced)
        ),
        None
    );
}

#[test]
fn fast_tail_leasing_splits_clean_ranges_into_fixed_leases() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let mib = 1024 * 1024;
    let gib = 1024 * mib;
    let total_bytes = 2 * gib;
    let mut state = segmented_state_for_test(total_bytes, vec![(0, total_bytes - 1, 0, false)]);
    let mut active = HashSet::new();

    let claimed = claim_largest_dynamic_segment_for_profile_tests(
        &mut state,
        &mut active,
        dynamic_segment_queue_depth(profile),
        profile,
    )
    .expect("tail leasing should claim a clean lease");

    assert_eq!(
        claimed.range,
        ByteRange {
            start: 0,
            end: 32 * mib - 1
        }
    );
    assert_eq!(
        state
            .segments
            .iter()
            .map(|segment| segment.range.len())
            .sum::<u64>(),
        total_bytes
    );
    assert!(state
        .segments
        .iter()
        .all(|segment| segment.range.len() <= 32 * mib));
}

#[test]
fn fast_tail_leasing_splits_partial_pending_remainders_without_losing_progress() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let mib = 1024 * 1024;
    let gib = 1024 * mib;
    let total_bytes = 2 * gib;
    let mut state =
        segmented_state_for_test(total_bytes, vec![(0, total_bytes - 1, 4 * mib, false)]);
    let mut active = HashSet::new();

    let claimed = claim_largest_dynamic_segment_for_profile_tests(
        &mut state,
        &mut active,
        dynamic_segment_queue_depth(profile),
        profile,
    )
    .expect("partial pending remainder should be leased safely");

    assert_eq!(claimed.downloaded_bytes, 4 * mib);
    assert_eq!(
        claimed.range,
        ByteRange {
            start: 0,
            end: 36 * mib - 1
        }
    );
    assert_eq!(
        state
            .segments
            .iter()
            .map(|segment| segment.range.len())
            .sum::<u64>(),
        total_bytes
    );
    assert_eq!(state.segments.first().unwrap().downloaded_bytes, 4 * mib);
    assert!(state
        .segments
        .windows(2)
        .all(|pair| pair[0].range.end.saturating_add(1) == pair[1].range.start));
}

#[test]
fn fast_tail_leasing_does_not_split_active_partial_ranges() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let mib = 1024 * 1024;
    let total_bytes = 512 * mib;
    let mut state = segmented_state_for_test(
        total_bytes,
        vec![
            (0, 256 * mib - 1, 8 * mib, false),
            (256 * mib, total_bytes - 1, 0, false),
        ],
    );
    let mut active = HashSet::from([0_usize]);

    let claimed = claim_largest_dynamic_segment_for_profile_tests(
        &mut state,
        &mut active,
        dynamic_segment_queue_depth(profile),
        profile,
    )
    .expect("inactive pending range should still be claimable");

    assert_ne!(claimed.index, 0);
    assert_eq!(
        state.segments[0].range,
        ByteRange {
            start: 0,
            end: 256 * mib - 1
        }
    );
    assert_eq!(state.segments[0].downloaded_bytes, 8 * mib);
    assert!(active.contains(&0));
}

#[test]
fn fast_tail_stage_keeps_pending_one_mib_leases_available() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let mib = 1024 * 1024;
    let mut state = segmented_state_for_test(128 * mib, vec![(0, 128 * mib - 1, 0, false)]);
    let active = HashSet::new();

    fill_dynamic_segment_queue_for_profile_tests(
        &mut state,
        &active,
        dynamic_segment_queue_depth(profile),
        profile,
    );

    let pending_one_mib_leases = state
        .segments
        .iter()
        .filter(|segment| segment.range.len() <= mib)
        .count();
    assert!(pending_segment_count(&state, &active) >= profile.max_segments);
    assert!(pending_one_mib_leases >= profile.max_segments);
    assert_eq!(
        state
            .segments
            .iter()
            .map(|segment| segment.range.len())
            .sum::<u64>(),
        128 * mib
    );
}

#[test]
fn fast_tail_leasing_splits_large_clean_ranges_even_when_queue_is_full() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let mib = 1024 * 1024;
    let total_bytes = 80 * 64 * mib;
    let mut state = segmented_state_for_test(
        total_bytes,
        (0_u64..80)
            .map(|index| {
                let start = index * 64 * mib;
                (start, start + 64 * mib - 1, 0, false)
            })
            .collect(),
    );
    let mut active = HashSet::new();

    let claimed = claim_largest_dynamic_segment_for_profile_tests(
        &mut state,
        &mut active,
        dynamic_segment_queue_depth(profile),
        profile,
    )
    .expect("clean resumed ranges should be leased before claim");

    assert_eq!(claimed.range.len(), 32 * mib);
    assert!(
        state
            .segments
            .iter()
            .filter(|segment| segment.range.len() <= 32 * mib)
            .count()
            >= dynamic_segment_queue_depth(profile)
    );
    assert_eq!(
        state
            .segments
            .iter()
            .map(|segment| segment.range.len())
            .sum::<u64>(),
        total_bytes
    );
}

#[test]
fn range_plan_respects_segment_connection_budget() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let capped_plan = plan_segmented_ranges_with_budget(
        1024 * 1024 * 1024,
        ResumeSupport::Supported,
        None,
        profile,
        Some(8),
    )
    .expect("available segment budget should still allow segmented downloading");

    assert_eq!(capped_plan.segments.len(), 8);
    assert!(plan_segmented_ranges_with_budget(
        1024 * 1024 * 1024,
        ResumeSupport::Supported,
        None,
        profile,
        Some(1),
    )
    .is_none());
}

#[test]
fn dynamic_segment_queue_splits_largest_pending_ranges_before_claim() {
    let mut state = SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 64,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: vec![SegmentProgress {
            index: 0,
            range: ByteRange { start: 0, end: 63 },
            downloaded_bytes: 0,
            completed: false,
        }],
    };
    let mut active = HashSet::new();

    let claimed = claim_largest_dynamic_segment_for_tests(&mut state, &mut active, 4, 8)
        .expect("dynamic queue should claim a segment");

    assert_eq!(claimed.range, ByteRange { start: 0, end: 15 });
    assert!(active.contains(&claimed.index));
    assert_eq!(
        state
            .segments
            .iter()
            .map(|segment| segment.range)
            .collect::<Vec<_>>(),
        vec![
            ByteRange { start: 0, end: 15 },
            ByteRange { start: 16, end: 31 },
            ByteRange { start: 32, end: 47 },
            ByteRange { start: 48, end: 63 },
        ]
    );
}

#[test]
fn dynamic_segment_queue_does_not_reassign_completed_spans() {
    let mut state = SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 64,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: vec![
            SegmentProgress {
                index: 0,
                range: ByteRange { start: 0, end: 15 },
                downloaded_bytes: 16,
                completed: true,
            },
            SegmentProgress {
                index: 1,
                range: ByteRange { start: 16, end: 63 },
                downloaded_bytes: 0,
                completed: false,
            },
        ],
    };
    let mut active = HashSet::new();

    let claimed = claim_largest_dynamic_segment_for_tests(&mut state, &mut active, 4, 8)
        .expect("dynamic queue should claim unfinished work");

    assert_ne!(claimed.index, 0);
    assert!(claimed.range.start >= 16);
    assert!(state.segments[0].completed);
    assert_eq!(state.segments[0].downloaded_bytes, 16);
}

#[tokio::test]
async fn adaptive_segment_admission_blocks_after_throughput_regression() {
    let root = test_download_runtime_dir("adaptive-ramp-regression");
    let temp_path = root.join("download.part");
    let mut job = torrent_job("job_adaptive_regression", JobState::Downloading);
    job.transfer_kind = TransferKind::Http;
    job.torrent = None;
    job.temp_path = temp_path.display().to_string();
    job.target_path = root.join("download.bin").display().to_string();
    let state = SharedState::for_tests(test_storage_path("adaptive-ramp-regression"), vec![job]);
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let progress = Arc::new(SegmentedProgressCounters::new(vec![0; 96]));
    progress.store_segment_bytes(0, 128 * 1024 * 1024);
    let active_segments = Arc::new(Mutex::new((0_usize..31).collect::<HashSet<_>>()));
    let ramp_blocked = Arc::new(AtomicBool::new(false));
    let target_workers = Arc::new(AtomicUsize::new(32));
    let metadata = Arc::new(Mutex::new(SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 1024 * 1024 * 1024,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: (0_u64..96)
            .map(|index| SegmentProgress {
                index: index as usize,
                range: ByteRange {
                    start: index * 8 * 1024 * 1024,
                    end: ((index + 1) * 8 * 1024 * 1024).saturating_sub(1),
                },
                downloaded_bytes: 0,
                completed: false,
            })
            .collect(),
    }));
    let context = SegmentWorkerContext {
        state,
        client: download_client().unwrap(),
        job_id: "job_adaptive_regression".into(),
        url: "https://cdn.example.com/file.bin".into(),
        segment_pressure_key: "https://cdn.example.com:443".into(),
        handoff_auth: None,
        temp_path,
        total_bytes: 1024 * 1024 * 1024,
        profile,
        validators: EntityValidators::default(),
        progress: progress.clone(),
        metadata: metadata.clone(),
        metadata_persisted_at: Arc::new(Mutex::new(Instant::now())),
        stop: Arc::new(AtomicBool::new(false)),
        control_signal: WorkerControlSignal::default(),
        ramp_blocked: ramp_blocked.clone(),
        priority_throttle: Arc::new(Mutex::new(DynamicThrottleState::default())),
        priority_throttle_enabled: false,
        stall_timeout: None,
        reconnects: Arc::new(SegmentReconnectTracker::default()),
        target_workers: target_workers.clone(),
        active_workers: Arc::new(AtomicUsize::new(32)),
    };
    let admission = AdaptiveSegmentAdmission {
        context,
        active_segments,
        metadata,
        progress,
        admitted_workers: Arc::new(AtomicUsize::new(32)),
        target_workers,
        segment_lease: None,
        queue_depth: 128,
        min_split_size: 1024 * 1024,
        last_ramp_total_bytes: AtomicU64::new(120 * 1024 * 1024),
        last_ramp_speed_bps: AtomicU64::new(64 * 1024 * 1024),
        regression_windows: AtomicUsize::new(0),
    };

    assert!(!admission.can_admit_more().await);
    assert!(ramp_blocked.load(Ordering::Relaxed));

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn adaptive_segment_admission_blocks_after_moderate_throughput_regression() {
    let root = test_download_runtime_dir("adaptive-ramp-moderate-regression");
    let temp_path = root.join("download.part");
    let mut job = torrent_job("job_adaptive_moderate_regression", JobState::Downloading);
    job.transfer_kind = TransferKind::Http;
    job.torrent = None;
    job.temp_path = temp_path.display().to_string();
    job.target_path = root.join("download.bin").display().to_string();
    let state = SharedState::for_tests(
        test_storage_path("adaptive-ramp-moderate-regression"),
        vec![job],
    );
    let profile = performance_profile(DownloadPerformanceMode::Fast);
    let progress = Arc::new(SegmentedProgressCounters::new(vec![0; 96]));
    progress.store_segment_bytes(0, 154 * 1024 * 1024);
    let active_segments = Arc::new(Mutex::new((0_usize..31).collect::<HashSet<_>>()));
    let ramp_blocked = Arc::new(AtomicBool::new(false));
    let target_workers = Arc::new(AtomicUsize::new(32));
    let metadata = Arc::new(Mutex::new(SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 1024 * 1024 * 1024,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: (0_u64..96)
            .map(|index| SegmentProgress {
                index: index as usize,
                range: ByteRange {
                    start: index * 8 * 1024 * 1024,
                    end: ((index + 1) * 8 * 1024 * 1024).saturating_sub(1),
                },
                downloaded_bytes: 0,
                completed: false,
            })
            .collect(),
    }));
    let context = SegmentWorkerContext {
        state,
        client: download_client().unwrap(),
        job_id: "job_adaptive_moderate_regression".into(),
        url: "https://cdn.example.com/file.bin".into(),
        segment_pressure_key: "https://cdn.example.com:443".into(),
        handoff_auth: None,
        temp_path,
        total_bytes: 1024 * 1024 * 1024,
        profile,
        validators: EntityValidators::default(),
        progress: progress.clone(),
        metadata: metadata.clone(),
        metadata_persisted_at: Arc::new(Mutex::new(Instant::now())),
        stop: Arc::new(AtomicBool::new(false)),
        control_signal: WorkerControlSignal::default(),
        ramp_blocked: ramp_blocked.clone(),
        priority_throttle: Arc::new(Mutex::new(DynamicThrottleState::default())),
        priority_throttle_enabled: false,
        stall_timeout: None,
        reconnects: Arc::new(SegmentReconnectTracker::default()),
        target_workers: target_workers.clone(),
        active_workers: Arc::new(AtomicUsize::new(32)),
    };
    let admission = AdaptiveSegmentAdmission {
        context,
        active_segments,
        metadata,
        progress,
        admitted_workers: Arc::new(AtomicUsize::new(32)),
        target_workers,
        segment_lease: None,
        queue_depth: 128,
        min_split_size: 1024 * 1024,
        last_ramp_total_bytes: AtomicU64::new(120 * 1024 * 1024),
        last_ramp_speed_bps: AtomicU64::new(40 * 1024 * 1024),
        regression_windows: AtomicUsize::new(0),
    };

    assert!(!admission.can_admit_more().await);
    assert!(ramp_blocked.load(Ordering::Relaxed));

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn fast_adaptive_sustain_holds_at_soft_cap_without_clear_improvement() {
    let (root, ramp_blocked, target_workers, admission) = adaptive_admission_for_test(
        "adaptive-ramp-soft-cap-hold",
        32,
        31,
        120 * 1024 * 1024,
        204 * 1024 * 1024,
        40 * 1024 * 1024,
    );

    assert!(!admission.can_admit_more().await);
    assert!(!ramp_blocked.load(Ordering::Relaxed));
    assert_eq!(target_workers.load(Ordering::Relaxed), 32);

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn fast_adaptive_sustain_allows_peak_cap_after_clear_improvement() {
    let (root, ramp_blocked, target_workers, admission) = adaptive_admission_for_test(
        "adaptive-ramp-soft-cap-grow",
        32,
        31,
        120 * 1024 * 1024,
        208 * 1024 * 1024,
        40 * 1024 * 1024,
    );

    assert!(admission.can_admit_more().await);
    assert!(!ramp_blocked.load(Ordering::Relaxed));
    assert_eq!(target_workers.load(Ordering::Relaxed), 32);

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn dynamic_segment_claim_does_not_persist_queue_splits_immediately() {
    let root = test_download_runtime_dir("dynamic-claim-no-persist");
    let temp_path = root.join("download.part");
    let metadata = Arc::new(Mutex::new(SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 64,
        validators: EntityValidators::default(),
        effective_url: None,
        target_path: None,
        temp_path: None,
        last_verified_file_len: 0,
        retry_generation: 0,
        segments: vec![SegmentProgress {
            index: 0,
            range: ByteRange { start: 0, end: 63 },
            downloaded_bytes: 0,
            completed: false,
        }],
    }));
    let active = Arc::new(Mutex::new(HashSet::new()));

    let claimed = claim_dynamic_segment_work(&temp_path, &metadata, &active, 4, 8, None)
        .await
        .unwrap()
        .expect("dynamic queue should claim a segment");

    assert_eq!(claimed.range, ByteRange { start: 0, end: 15 });
    assert!(
        !segment_meta_path(&temp_path).exists(),
        "claiming split work should leave sidecar persistence to the shared persist cadence"
    );
    assert_eq!(metadata.lock().await.segments.len(), 4);

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[test]
fn range_plan_falls_back_for_stable_small_unknown_or_limited_downloads() {
    assert!(plan_segmented_ranges(
        256 * 1024 * 1024,
        ResumeSupport::Supported,
        None,
        performance_profile(DownloadPerformanceMode::Stable),
    )
    .is_none());
    assert!(plan_segmented_ranges(
        16 * 1024 * 1024,
        ResumeSupport::Supported,
        None,
        performance_profile(DownloadPerformanceMode::Balanced),
    )
    .is_none());
    assert!(plan_segmented_ranges(
        256 * 1024 * 1024,
        ResumeSupport::Unknown,
        None,
        performance_profile(DownloadPerformanceMode::Balanced),
    )
    .is_none());
    assert!(plan_segmented_ranges(
        256 * 1024 * 1024,
        ResumeSupport::Supported,
        Some(1024),
        performance_profile(DownloadPerformanceMode::Balanced),
    )
    .is_none());
    assert!(plan_segmented_ranges(
        256 * 1024 * 1024,
        ResumeSupport::Supported,
        Some(1024),
        performance_profile(DownloadPerformanceMode::Fast),
    )
    .is_none());
}

#[test]
fn fuckingfast_hoster_bulk_tasks_allow_safe_segmented_downloads() {
    let mut task = http_segment_policy_task(
        true,
        Some("https://fuckingfast.co/ecw0lw398okf#Game.part01.rar"),
    );
    task.url = "https://dl.fuckingfast.co/dl/token/Game.part01.rar".into();

    assert!(task_allows_segmented_download(&task));
}

#[test]
fn datanodes_hoster_bulk_tasks_allow_safe_segmented_downloads() {
    let mut task = http_segment_policy_task(
        true,
        Some("https://datanodes.to/abc123456789/fg-optional-bonus-content.bin"),
    );
    task.url = "https://s42.datanodes.to/d/abc123456789/fg-optional-bonus-content.bin".into();

    assert!(task_allows_segmented_download(&task));
}

#[test]
fn segment_budget_uses_active_connection_leases() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_3",
            "https://s1.datanodes.to/d/ghi/file.bin",
            SegmentConnectionBudget {
                total: 16,
                per_origin: 8,
            },
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/abc/file.bin",
                    4,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s2.datanodes.to/d/def/file.bin",
                    4,
                ),
            ],
        ),
        Some(4)
    );
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_4",
            "https://s3.datanodes.to/d/jkl/file.bin",
            SegmentConnectionBudget {
                total: 8,
                per_origin: 4,
            },
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/abc/file.bin",
                    4,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s2.datanodes.to/d/def/file.bin",
                    4,
                ),
            ],
        ),
        None
    );
}

#[test]
fn segment_budget_wait_action_defers_bulk_admissions_and_preserves_active_segment_state() {
    assert_eq!(
        DownloadAdmission::direct_bulk().segment_budget_wait_action(false),
        SegmentBudgetWaitAction::Defer
    );
    assert_eq!(
        DownloadAdmission::protected_hoster_bulk().segment_budget_wait_action(false),
        SegmentBudgetWaitAction::Defer
    );
    assert_eq!(
        DownloadAdmission::normal().segment_budget_wait_action(false),
        SegmentBudgetWaitAction::FallbackSingleStream
    );
    assert_eq!(
        DownloadAdmission::normal().segment_budget_wait_action(true),
        SegmentBudgetWaitAction::Defer
    );
}

#[test]
fn datanodes_balanced_budget_keeps_four_same_origin_jobs_segmented_with_two_segment_floor() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_4",
            "https://s1.datanodes.to/d/jkl/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Balanced).unwrap(),
            4,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/abc/file.bin",
                    4,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/def/file.bin",
                    2,
                ),
                (
                    "job_3",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/ghi/file.bin",
                    2,
                ),
            ],
        ),
        Some(2)
    );
}

#[test]
fn datanodes_oldest_balanced_worker_keeps_full_segment_cap() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_1",
            "https://s1.datanodes.to/d/abc/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Balanced).unwrap(),
            4,
            &[],
        ),
        Some(4)
    );
}

#[test]
fn datanodes_secondary_balanced_workers_start_with_two_segments() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_2",
            "https://s1.datanodes.to/d/def/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Balanced).unwrap(),
            4,
            &[(
                "job_1",
                SegmentConnectionClass::ProtectedHosterBulk,
                "https://s1.datanodes.to/d/abc/file.bin",
                4,
            )],
        ),
        Some(2)
    );
}

#[test]
fn datanodes_oldest_fast_worker_keeps_full_segment_cap() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_1",
            "https://s1.datanodes.to/d/abc/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Fast).unwrap(),
            6,
            &[],
        ),
        Some(6)
    );
}

#[test]
fn datanodes_fast_worker_can_grow_to_adaptive_segment_cap() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_1",
            "https://s1.datanodes.to/d/abc/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Fast).unwrap(),
            10,
            &[(
                "job_1",
                SegmentConnectionClass::ProtectedHosterBulk,
                "https://s1.datanodes.to/d/abc/file.bin",
                6,
            )],
        ),
        Some(10)
    );
}

#[test]
fn datanodes_secondary_fast_workers_start_with_two_segments() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_2",
            "https://s1.datanodes.to/d/def/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Fast).unwrap(),
            6,
            &[(
                "job_1",
                SegmentConnectionClass::ProtectedHosterBulk,
                "https://s1.datanodes.to/d/abc/file.bin",
                6,
            )],
        ),
        Some(2)
    );
}

#[test]
fn datanodes_third_fast_worker_stays_segmented_with_two_segment_floor() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_3",
            "https://s1.datanodes.to/d/ghi/file.bin",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Fast).unwrap(),
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/abc/file.bin",
                    6,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://s1.datanodes.to/d/def/file.bin",
                    2,
                ),
            ],
        ),
        Some(2)
    );
}

#[test]
fn fuckingfast_oldest_balanced_worker_keeps_full_segment_cap() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_1",
            "https://dl.fuckingfast.co/dl/token-a/Game.part01.rar",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Balanced).unwrap(),
            4,
            &[],
        ),
        Some(4)
    );
}

#[test]
fn fuckingfast_secondary_balanced_workers_start_with_two_segments() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_2",
            "https://dl.fuckingfast.co/dl/token-b/Game.part02.rar",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Balanced).unwrap(),
            4,
            &[(
                "job_1",
                SegmentConnectionClass::ProtectedHosterBulk,
                "https://dl.fuckingfast.co/dl/token-a/Game.part01.rar",
                4,
            )],
        ),
        Some(2)
    );
}

#[test]
fn fuckingfast_third_fast_worker_stays_segmented_with_two_segment_floor() {
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::ProtectedHosterBulk,
            "job_3",
            "https://dl.fuckingfast.co/dl/token-c/Game.part03.rar",
            hoster_segment_budget_for_mode(DownloadPerformanceMode::Fast).unwrap(),
            6,
            &[
                (
                    "job_1",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://dl.fuckingfast.co/dl/token-a/Game.part01.rar",
                    6,
                ),
                (
                    "job_2",
                    SegmentConnectionClass::ProtectedHosterBulk,
                    "https://dl.fuckingfast.co/dl/token-b/Game.part02.rar",
                    2,
                ),
            ],
        ),
        Some(2)
    );
}

#[test]
fn datanodes_warmup_completion_wakes_scheduler() {
    let source = include_str!("../http.rs");
    let warmup_function = source
        .split("pub(super) fn spawn_datanodes_hoster_warmups")
        .nth(1)
        .expect("DataNodes warmup spawning should exist");

    assert!(
        warmup_function.contains("schedule_downloads(app.clone(), state.clone())"),
        "ready or failed DataNodes warmups should wake the scheduler"
    );
}

#[test]
fn datanodes_warmup_cache_consumes_ready_links_and_drops_expired_links() {
    clear_hoster_warmup_cache_for_tests();
    let source_url = "https://datanodes.to/abc123456/Game.part.rar";
    put_hoster_warmup_for_tests(
        "job_warm",
        source_url,
        "https://s1.datanodes.to/d/abc123456/Game.part.rar",
        Instant::now() + Duration::from_secs(60),
    );

    assert_eq!(
        take_warmed_hoster_url_for_tests("job_warm", source_url).as_deref(),
        Some("https://s1.datanodes.to/d/abc123456/Game.part.rar")
    );
    assert!(take_warmed_hoster_url_for_tests("job_warm", source_url).is_none());

    put_hoster_warmup_for_tests(
        "job_expired",
        source_url,
        "https://s2.datanodes.to/d/abc123456/Game.part.rar",
        Instant::now() - Duration::from_secs(1),
    );
    assert!(take_warmed_hoster_url_for_tests("job_expired", source_url).is_none());
}

#[test]
fn segmented_hoster_workers_use_aggregate_priority_throttle_without_deferring() {
    let source = include_str!("../segmented.rs");
    let worker = source
        .split("pub(super) async fn download_segment_worker")
        .nth(1)
        .expect("segmented worker should exist");

    assert!(worker.contains("hoster_priority_throttle_decision"));
    assert!(worker.contains("throttle_download_with_dynamic_limit"));
    assert!(worker.contains("priority_throttle_limited"));
    assert!(source.contains("priority_throttle"));
    assert!(!source.contains("return Ok(DownloadOutcome::Deferred"));
}

#[test]
fn segmented_download_marks_bytes_complete_before_final_file_work() {
    let source = include_str!("../segmented.rs");
    let attempt = source
        .split("pub(super) async fn run_segmented_download_attempt")
        .nth(1)
        .expect("segmented attempt function should exist");
    let progress = attempt
        .find(".update_job_progress(&task.id, plan.total_bytes")
        .expect("segmented attempt should mark final byte progress");
    let sync = attempt
        .find("sync_direct_segment_file")
        .expect("segmented attempt should sync the partial file");
    let cleanup = attempt
        .find("cleanup_segment_artifacts")
        .expect("segmented attempt should clean segment artifacts");
    let move_to_final = attempt
        .find("move_to_final_path")
        .expect("segmented attempt should rename the completed file");

    assert!(progress < sync);
    assert!(progress < cleanup);
    assert!(progress < move_to_final);
}

#[test]
fn content_range_validation_rejects_mismatched_segments() {
    assert!(content_range_matches(
        "bytes 1048576-2097151/4194304",
        ByteRange {
            start: 1_048_576,
            end: 2_097_151,
        },
        4_194_304,
    ));
    assert!(!content_range_matches(
        "bytes 0-2097151/4194304",
        ByteRange {
            start: 1_048_576,
            end: 2_097_151,
        },
        4_194_304,
    ));
    assert!(!content_range_matches(
        "bytes 1048576-2097151/9999999",
        ByteRange {
            start: 1_048_576,
            end: 2_097_151,
        },
        4_194_304,
    ));
}

#[test]
fn probed_range_metadata_wins_when_head_size_disagrees() {
    let merged = merge_preflight_metadata(
        Some(PreflightMetadata {
            total_bytes: Some(64),
            resume_support: ResumeSupport::Supported,
            filename: Some("head.bin".into()),
            validators: EntityValidators {
                etag: None,
                last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
            },
        }),
        PreflightMetadata {
            total_bytes: Some(128),
            resume_support: ResumeSupport::Supported,
            filename: Some("probe.bin".into()),
            validators: EntityValidators {
                etag: Some("\"probe\"".into()),
                last_modified: None,
            },
        },
    );

    assert_eq!(merged.total_bytes, Some(128));
    assert_eq!(merged.filename.as_deref(), Some("head.bin"));
    assert_eq!(merged.validators.etag.as_deref(), Some("\"probe\""));
    assert_eq!(
        merged.validators.last_modified.as_deref(),
        Some("Wed, 21 Oct 2015 07:28:00 GMT")
    );
}

#[test]
fn rolling_speed_smoothing_avoids_one_sample_collapse() {
    let mut speed = RollingSpeed::default();

    assert_eq!(
        speed.record_sample(8 * 1024 * 1024, Duration::from_secs(1)),
        8 * 1024 * 1024
    );
    let smoothed = speed.record_sample(512, Duration::from_secs(1));

    assert!(
        smoothed > 1024 * 1024,
        "one tiny sample should not collapse the displayed speed to near zero"
    );
}

#[test]
fn segmented_progress_counters_track_totals_without_shared_mutex() {
    let counters = SegmentedProgressCounters::new(vec![10, 20, 0]);

    assert_eq!(counters.total_downloaded(), 30);
    counters.store_segment_bytes(2, 5);
    counters.add_sample_bytes(7);

    assert_eq!(counters.total_downloaded(), 35);
    assert_eq!(counters.drain_sample_bytes(), 7);
    assert_eq!(counters.drain_sample_bytes(), 0);
}

#[test]
fn segmented_progress_initial_bytes_are_index_aligned_after_dynamic_splits() {
    let segments = vec![
        SegmentProgress {
            index: 0,
            range: ByteRange { start: 0, end: 15 },
            downloaded_bytes: 10,
            completed: false,
        },
        SegmentProgress {
            index: 2,
            range: ByteRange { start: 16, end: 31 },
            downloaded_bytes: 4,
            completed: false,
        },
        SegmentProgress {
            index: 1,
            range: ByteRange { start: 32, end: 47 },
            downloaded_bytes: 8,
            completed: false,
        },
    ];

    let counters = SegmentedProgressCounters::new(segment_existing_lengths_by_index(
        Path::new("unused"),
        &segments,
    ));

    assert_eq!(counters.total_downloaded(), 22);
    counters.store_segment_bytes(1, 12);
    assert_eq!(counters.total_downloaded(), 26);
}

#[tokio::test]
async fn segment_state_persists_concurrent_writers_without_temp_file_race() {
    let root = test_download_runtime_dir("segment-concurrent-sidecar");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let validators = EntityValidators::default();
    prepare_direct_segment_file(&temp_path, plan.total_bytes)
        .await
        .unwrap();

    let writer_count = 96;
    let barrier = Arc::new(tokio::sync::Barrier::new(writer_count));
    let mut handles = Vec::with_capacity(writer_count);

    for index in 0..writer_count {
        let barrier = barrier.clone();
        let temp_path = temp_path.clone();
        let plan = plan.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let mut state = new_segment_state_for_test(&plan, EntityValidators::default());
            let segment_index = index % state.segments.len();
            state.segments[segment_index].downloaded_bytes =
                (index as u64 % state.segments[segment_index].range.len()).saturating_add(1);
            persist_segment_state(&temp_path, &state).await
        }));
    }

    for handle in handles {
        handle
            .await
            .expect("segment metadata writer should not panic")
            .expect("segment metadata writer should not race on temp file replacement");
    }

    let reloaded = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .expect("final segment metadata should remain readable");
    assert_eq!(reloaded.total_bytes, plan.total_bytes);
    assert_eq!(reloaded.segments.len(), plan.segments.len());
    assert!(
        !segment_meta_temp_path(&temp_path).exists(),
        "fixed legacy metadata temp path should not be left behind"
    );

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segment_state_preserves_progress_when_validators_match() {
    let root = test_download_runtime_dir("segment-validator-match");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let validators = EntityValidators {
        etag: Some("\"abc123\"".into()),
        last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
    };
    let mut state = new_segment_state_for_test(&plan, validators.clone());
    state.segments[0].downloaded_bytes = 4;
    state.segments[0].completed = true;
    tokio::fs::write(&temp_path, b"abcdefghijkl").await.unwrap();
    persist_segment_state(&temp_path, &state).await.unwrap();

    let reloaded = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .unwrap();

    assert_eq!(reloaded.validators, validators);
    assert_eq!(reloaded.segments[0].downloaded_bytes, 4);
    assert!(reloaded.segments[0].completed);
    assert!(temp_path.exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segment_state_preserves_partial_and_requires_restart_when_validators_change() {
    let root = test_download_runtime_dir("segment-validator-changed");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let mut state = new_segment_state_for_test(
        &plan,
        EntityValidators {
            etag: Some("\"old\"".into()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
        },
    );
    state.segments[0].downloaded_bytes = 4;
    state.segments[0].completed = true;
    tokio::fs::write(&temp_path, b"abcdefghijkl").await.unwrap();
    persist_segment_state(&temp_path, &state).await.unwrap();

    let next_validators = EntityValidators {
        etag: Some("\"new\"".into()),
        last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".into()),
    };
    let error = load_or_create_segment_state(&temp_path, &plan, &next_validators)
        .await
        .expect_err("validator conflicts with an existing partial should require restart");

    assert_eq!(error.category, FailureCategory::Resume);
    assert!(error.message.contains("Resume metadata is missing"));
    assert_eq!(
        error.resume_metadata_issue,
        Some(SegmentResumeMetadataIssue::ValidatorConflict)
    );
    assert!(temp_path.exists());
    assert!(segment_meta_path(&temp_path).exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segment_state_preserves_partial_and_reports_incompatible_metadata() {
    let root = test_download_runtime_dir("segment-plan-incompatible");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let mut state = new_segment_state_for_test(&plan, EntityValidators::default());
    state.total_bytes = state.total_bytes.saturating_add(1);
    tokio::fs::write(&temp_path, b"abcdefghijkl").await.unwrap();
    persist_segment_state(&temp_path, &state).await.unwrap();

    let error = load_or_create_segment_state(&temp_path, &plan, &EntityValidators::default())
        .await
        .expect_err("incompatible segment metadata should require restart");

    assert_eq!(error.category, FailureCategory::Resume);
    assert!(error.message.contains("Resume metadata is missing"));
    assert_eq!(
        error.resume_metadata_issue,
        Some(SegmentResumeMetadataIssue::PlanIncompatible)
    );
    assert!(temp_path.exists());
    assert!(segment_meta_path(&temp_path).exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segment_state_keeps_old_progress_when_stored_validators_are_missing() {
    let root = test_download_runtime_dir("segment-validator-missing");
    let temp_path = root.join("download.bin.part");
    let plan = three_segment_test_plan();
    let mut state = new_segment_state_for_test(&plan, EntityValidators::default());
    state.segments[1].downloaded_bytes = 2;
    tokio::fs::write(&temp_path, b"abcdefghijkl").await.unwrap();
    persist_segment_state(&temp_path, &state).await.unwrap();

    let next_validators = EntityValidators {
        etag: Some("\"new\"".into()),
        last_modified: None,
    };
    let reloaded = load_or_create_segment_state(&temp_path, &plan, &next_validators)
        .await
        .unwrap();

    assert_eq!(reloaded.validators, next_validators);
    assert_eq!(reloaded.segments[1].downloaded_bytes, 2);
    assert!(temp_path.exists());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segment_worker_resumes_partial_range_into_existing_file() {
    let root = test_download_runtime_dir("segment-worker-resume");
    let temp_path = root.join("download.bin.part");
    let response = concat!(
        "HTTP/1.1 206 Partial Content\r\n",
        "Content-Range: bytes 4-11/12\r\n",
        "Content-Length: 8\r\n",
        "\r\n",
        "efghijkl"
    );
    let (url, request_handle) = spawn_one_response_server(response).await;
    let validators = EntityValidators {
        etag: Some("\"segment-source\"".into()),
        last_modified: None,
    };
    let segment = SegmentProgress {
        index: 0,
        range: ByteRange { start: 0, end: 11 },
        downloaded_bytes: 4,
        completed: false,
    };
    let mut stored = SegmentedDownloadState {
        schema_version: default_segment_state_schema_version(),
        total_bytes: 12,
        validators: validators.clone(),
        effective_url: Some(url.clone()),
        target_path: Some(root.join("download.bin").display().to_string()),
        temp_path: Some(temp_path.display().to_string()),
        last_verified_file_len: 12,
        retry_generation: 0,
        segments: vec![segment.clone()],
    };
    prepare_direct_segment_file(&temp_path, 12).await.unwrap();
    let mut file = open_direct_segment_file(&temp_path).await.unwrap();
    write_segment_chunk_to(&mut file, 0, b"abcd").await.unwrap();
    drop(file);

    let mut job = torrent_job("job_segment_resume", JobState::Downloading);
    job.transfer_kind = TransferKind::Http;
    job.torrent = None;
    job.temp_path = temp_path.display().to_string();
    job.target_path = root.join("download.bin").display().to_string();
    let state = SharedState::for_tests(test_storage_path("segment-worker-resume-state"), vec![job]);
    stored.segments[0].downloaded_bytes = 4;
    let context = SegmentWorkerContext {
        state,
        client: download_client().unwrap(),
        job_id: "job_segment_resume".into(),
        url,
        segment_pressure_key: "http://127.0.0.1:80".into(),
        handoff_auth: None,
        temp_path: temp_path.clone(),
        total_bytes: 12,
        profile: performance_profile(DownloadPerformanceMode::Balanced),
        validators,
        progress: Arc::new(SegmentedProgressCounters::new(vec![4])),
        metadata: Arc::new(Mutex::new(stored)),
        metadata_persisted_at: Arc::new(Mutex::new(Instant::now())),
        stop: Arc::new(AtomicBool::new(false)),
        control_signal: WorkerControlSignal::default(),
        ramp_blocked: Arc::new(AtomicBool::new(false)),
        priority_throttle: Arc::new(Mutex::new(DynamicThrottleState::default())),
        priority_throttle_enabled: false,
        stall_timeout: None,
        reconnects: Arc::new(SegmentReconnectTracker::default()),
        target_workers: Arc::new(AtomicUsize::new(1)),
        active_workers: Arc::new(AtomicUsize::new(1)),
    };

    let outcome = download_segment_worker(context, segment).await.unwrap();
    let request = request_handle.await.unwrap();
    let request_lower = request.to_ascii_lowercase();
    let bytes = tokio::fs::read(&temp_path).await.unwrap();

    assert_eq!(outcome, DownloadOutcome::Completed);
    assert!(request_lower.contains("range: bytes=4-11"));
    assert!(request_lower.contains("if-range: \"segment-source\""));
    assert_eq!(bytes, b"abcdefghijkl");

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn segment_worker_collector_returns_on_first_error() {
    let mut workers = tokio::task::JoinSet::new();
    workers.spawn(async {
        Err(download_error(
            FailureCategory::Network,
            "segment failed quickly".into(),
            true,
        ))
    });
    workers.spawn(async {
        tokio::time::sleep(Duration::from_secs(5)).await;
        Ok(DownloadOutcome::Completed)
    });

    let started = Instant::now();
    let (_outcome, error) = await_segment_workers(workers).await;

    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(
        error
            .expect("first worker error should be returned")
            .message,
        "segment failed quickly"
    );
}

#[tokio::test]
async fn segment_worker_collector_signals_stop_before_returning_error() {
    let stop = Arc::new(AtomicBool::new(false));
    let peer_observed_stop = Arc::new(AtomicBool::new(false));
    let mut workers = tokio::task::JoinSet::new();
    workers.spawn(async {
        Err(download_error(
            FailureCategory::Network,
            "segment failed quickly".into(),
            true,
        ))
    });
    {
        let stop = stop.clone();
        let peer_observed_stop = peer_observed_stop.clone();
        workers.spawn(async move {
            while !stop.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            peer_observed_stop.store(true, Ordering::Relaxed);
            Ok(DownloadOutcome::Paused)
        });
    }

    let (_outcome, error) = await_segment_workers_with_stop(workers, stop.clone()).await;

    assert_eq!(
        error
            .expect("first worker error should be returned")
            .message,
        "segment failed quickly"
    );
    assert!(stop.load(Ordering::Relaxed));
    assert!(peer_observed_stop.load(Ordering::Relaxed));
}

#[test]
fn normal_fast_segment_budget_allows_adaptive_fast_plus_ceiling() {
    let budget = normal_segment_budget_for_mode(DownloadPerformanceMode::Fast)
        .expect("fast normal downloads should use brokered segment budgets");

    assert_eq!(budget.total, 128);
    assert_eq!(budget.per_origin, 64);
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::Normal,
            "job_fast_1",
            "https://cdn.example.com/fast.bin",
            budget,
            usize::MAX,
            &[],
        ),
        Some(64)
    );
}

#[test]
fn fast_profile_uses_adaptive_sustain_defaults() {
    let profile = performance_profile(DownloadPerformanceMode::Fast);

    assert_eq!(profile.initial_segments, 16);
    assert_eq!(profile.soft_max_segments, 32);
    assert_eq!(profile.max_segments, 64);
    assert_eq!(profile.adaptive_ramp_step, 4);
    assert_eq!(profile.adaptive_ramp_interval, Duration::from_secs(2));
}

#[test]
fn gofile_fast_profile_uses_conservative_direct_cap() {
    let profile = profile_for_effective_http_url(
        DownloadPerformanceMode::Fast,
        "https://store1.gofile.io/download/web/file-token/BeamNG-drive-SteamRIP.com.rar",
    );

    assert_eq!(profile.initial_segments, 8);
    assert_eq!(profile.soft_max_segments, 16);
    assert_eq!(profile.max_segments, 16);
    assert_eq!(profile.adaptive_ramp_step, 4);
}

#[test]
fn host_score_temporarily_caps_fast_profile_after_reconnects() {
    let _guard = segment_host_score_test_guard();
    reset_segment_host_scores_for_tests();
    let now = Instant::now();
    let url = "https://cdn.example.com/downloads/game.rar";

    record_segment_host_success(url, 32, now);
    record_segment_host_failure(url, 16, "segment reconnect", now + Duration::from_secs(1));

    let score = segment_host_score_snapshot(url, now + Duration::from_secs(2))
        .expect("host score should be retained during its TTL");
    assert_eq!(score.best_cap, 16);
    assert_eq!(score.recent_reconnects, 1);
    assert_eq!(
        score.last_failure_reason.as_deref(),
        Some("segment reconnect")
    );

    let profile = profile_for_effective_http_url_at(
        DownloadPerformanceMode::Fast,
        url,
        now + Duration::from_secs(2),
    );
    assert_eq!(profile.max_segments, 16);
    assert_eq!(profile.soft_max_segments, 16);

    assert!(segment_host_score_snapshot(url, now + Duration::from_secs(31 * 60)).is_none());
}

#[test]
fn segment_pressure_requires_repeated_429_before_capping_fast_profile() {
    let _guard = segment_host_score_test_guard();
    reset_segment_host_scores_for_tests();
    let now = Instant::now();
    let key = "hoster:test-file";
    let url = "https://rotating-cdn.example.com/downloads/game.rar";
    let error = error_for_http_status(StatusCode::TOO_MANY_REQUESTS, false);

    let first = record_segment_reconnect_pressure_for_error(key, 16, &error, now);
    assert_eq!(first.reduced_target, None);

    let uncapped = profile_for_effective_http_url_with_pressure_key_at(
        DownloadPerformanceMode::Fast,
        url,
        Some(key),
        now + Duration::from_secs(1),
    );
    assert_eq!(uncapped.initial_segments, 16);
    assert_eq!(uncapped.soft_max_segments, 32);
    assert_eq!(uncapped.max_segments, 64);

    record_segment_reconnect_pressure_for_error(key, 16, &error, now + Duration::from_secs(2));
    let third =
        record_segment_reconnect_pressure_for_error(key, 16, &error, now + Duration::from_secs(3));

    assert_eq!(third.reduced_target, Some(8));
    let capped = profile_for_effective_http_url_with_pressure_key_at(
        DownloadPerformanceMode::Fast,
        url,
        Some(key),
        now + Duration::from_secs(4),
    );
    assert_eq!(capped.initial_segments, 8);
    assert_eq!(capped.soft_max_segments, 8);
    assert_eq!(capped.max_segments, 8);
}

#[test]
fn segment_pressure_expires_and_allows_fast_profile_recovery() {
    let _guard = segment_host_score_test_guard();
    reset_segment_host_scores_for_tests();
    let now = Instant::now();
    let key = "hoster:ttl";
    let url = "https://cdn.example.com/downloads/game.rar";
    let error = error_for_http_status(StatusCode::TOO_MANY_REQUESTS, false);

    for offset in 0..3 {
        record_segment_reconnect_pressure_for_error(
            key,
            16,
            &error,
            now + Duration::from_secs(offset),
        );
    }

    let capped = profile_for_effective_http_url_with_pressure_key_at(
        DownloadPerformanceMode::Fast,
        url,
        Some(key),
        now + Duration::from_secs(4),
    );
    assert_eq!(capped.max_segments, 8);

    let recovered = profile_for_effective_http_url_with_pressure_key_at(
        DownloadPerformanceMode::Fast,
        url,
        Some(key),
        now + Duration::from_secs(31 * 60),
    );
    assert_eq!(recovered.initial_segments, 16);
    assert_eq!(recovered.soft_max_segments, 32);
    assert_eq!(recovered.max_segments, 64);
}

#[test]
fn segment_write_coalescer_batches_small_chunks_and_flushes_tail() {
    let mut coalescer = SegmentWriteCoalescer::new(8);

    assert_eq!(coalescer.push(b"abc"), None);
    assert_eq!(coalescer.push(b"defg"), None);
    assert_eq!(coalescer.push(b"h"), Some(b"abcdefgh".to_vec()));
    assert_eq!(coalescer.flush(), None);

    assert_eq!(coalescer.push(b"xy"), None);
    assert_eq!(coalescer.flush(), Some(b"xy".to_vec()));
    assert_eq!(coalescer.flush(), None);
}

#[test]
fn direct_bulk_fast_budget_reuses_normal_fast_connection_ceiling() {
    let budget = direct_bulk_segment_budget_for_mode(DownloadPerformanceMode::Fast)
        .expect("fast direct bulk downloads should use brokered segment budgets");
    let normal_budget = normal_segment_budget_for_mode(DownloadPerformanceMode::Fast)
        .expect("fast normal downloads should use brokered segment budgets");

    assert_eq!(budget, normal_budget);
    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::DirectBulk,
            "bulk_fast_1",
            "https://cdn.example.com/bulk-fast.bin",
            budget,
            usize::MAX,
            &[],
        ),
        Some(64)
    );
}

#[test]
fn direct_bulk_and_normal_segment_budgets_are_isolated_by_class() {
    let budget = direct_bulk_segment_budget_for_mode(DownloadPerformanceMode::Fast)
        .expect("fast direct bulk downloads should use brokered segment budgets");

    assert_eq!(
        segment_budget_from_test_leases(
            SegmentConnectionClass::DirectBulk,
            "bulk_fast_1",
            "https://cdn.example.com/bulk-fast.bin",
            budget,
            usize::MAX,
            &[(
                "normal_fast_1",
                SegmentConnectionClass::Normal,
                "https://cdn.example.com/normal-fast.bin",
                64,
            )],
        ),
        Some(64)
    );
}

#[test]
fn segment_metadata_persist_gate_coalesces_regular_writes_and_allows_forced_writes() {
    let now = Instant::now();
    let mut last_persisted_at = now;

    assert!(
        !should_persist_segment_metadata(
            &mut last_persisted_at,
            now + Duration::from_secs(1),
            false,
        ),
        "regular segment progress should not persist before the shared interval"
    );
    assert_eq!(last_persisted_at, now);

    let interval_elapsed = now + PROGRESS_PERSIST_INTERVAL;
    assert!(
        should_persist_segment_metadata(&mut last_persisted_at, interval_elapsed, false),
        "one worker should persist after the shared interval elapses"
    );
    assert_eq!(last_persisted_at, interval_elapsed);

    assert!(
        !should_persist_segment_metadata(&mut last_persisted_at, interval_elapsed, false),
        "peer segment workers should be coalesced after another worker persists"
    );
    assert!(
        should_persist_segment_metadata(&mut last_persisted_at, interval_elapsed, true),
        "forced interruption and completion writes should bypass coalescing"
    );
}

#[tokio::test]
async fn segmented_sidecar_resets_progress_when_partial_file_is_missing() {
    let root = test_download_runtime_dir("segment-missing-partial");
    let temp_path = root.join("download.bin.part");
    let plan = RangePlan {
        total_bytes: 12,
        segments: vec![
            ByteRange { start: 0, end: 3 },
            ByteRange { start: 4, end: 7 },
            ByteRange { start: 8, end: 11 },
        ],
    };

    let validators = EntityValidators::default();
    let mut state = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .unwrap();
    state.segments[0].downloaded_bytes = 4;
    state.segments[0].completed = true;
    state.segments[1].downloaded_bytes = 2;
    persist_segment_state(&temp_path, &state).await.unwrap();

    refresh_segment_completion_from_disk(&temp_path, &mut state).await;

    assert_eq!(state.segments[0].downloaded_bytes, 0);
    assert!(!state.segments[0].completed);
    assert_eq!(state.segments[1].downloaded_bytes, 0);
    assert!(!state.segments[1].completed);

    persist_segment_state(&temp_path, &state).await.unwrap();
    let reloaded = load_or_create_segment_state(&temp_path, &plan, &validators)
        .await
        .unwrap();
    assert_eq!(reloaded.segments[0].downloaded_bytes, 0);
    assert!(!reloaded.segments[0].completed);
    assert_eq!(reloaded.segments[1].downloaded_bytes, 0);
    assert!(!reloaded.segments[1].completed);

    let _ = tokio::fs::remove_dir_all(root).await;
}
