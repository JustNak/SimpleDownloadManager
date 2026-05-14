use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RetryDecision {
    Retry,
    PauseRecoverably,
    Fail,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct HttpTransferPolicy {
    pub(super) mode: DownloadPerformanceMode,
    pub(super) profile: DownloadPerformanceProfile,
}

#[derive(Debug, Clone, Copy)]
struct HttpPerformanceModeConfig {
    mode: DownloadPerformanceMode,
    profile: DownloadPerformanceProfile,
}

#[derive(Debug, Clone)]
pub(super) struct SegmentHostScoreSnapshot {
    pub(super) best_cap: usize,
    pub(super) rate_limited_cap: Option<usize>,
    pub(super) recent_reconnects: u32,
    pub(super) recent_rate_limits: u32,
    pub(super) last_failure_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct SegmentHostScore {
    best_cap: Option<usize>,
    rate_limited_cap: Option<usize>,
    recent_reconnects: u32,
    recent_rate_limits: u32,
    rate_limit_window_started_at: Option<Instant>,
    last_failure_reason: Option<String>,
    updated_at: Instant,
}

static SEGMENT_HOST_SCORES: OnceLock<StdMutex<HashMap<String, SegmentHostScore>>> = OnceLock::new();

const SEGMENT_HOST_SCORE_TTL: Duration = Duration::from_secs(30 * 60);
const SEGMENT_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const SEGMENT_RATE_LIMIT_THRESHOLD: u32 = 3;
const SEGMENT_PRESSURE_MIN_TARGET: usize = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct SegmentPressureDecision {
    pub(super) recent_rate_limits: u32,
    pub(super) reduced_target: Option<usize>,
}

const HTTP_PERFORMANCE_PROFILES: [HttpPerformanceModeConfig; 3] = [
    HttpPerformanceModeConfig {
        mode: DownloadPerformanceMode::Stable,
        profile: DownloadPerformanceProfile {
            initial_segments: 1,
            soft_max_segments: 1,
            max_segments: 1,
            min_segmented_size: u64::MAX,
            target_segment_size: u64::MAX,
            low_speed_threshold_bytes_per_second: 4 * 1024,
            low_speed_window: Duration::from_secs(30),
            bulk_hoster_stall_timeout: Duration::from_secs(90),
            max_low_speed_retries: 2,
            speed_smoothing_alpha: 0.25,
            adaptive_ramp_step: 0,
            adaptive_ramp_interval: Duration::from_millis(1500),
        },
    },
    HttpPerformanceModeConfig {
        mode: DownloadPerformanceMode::Balanced,
        profile: DownloadPerformanceProfile {
            initial_segments: 6,
            soft_max_segments: 6,
            max_segments: 6,
            min_segmented_size: BALANCED_MIN_SEGMENTED_SIZE,
            target_segment_size: BALANCED_TARGET_SEGMENT_SIZE,
            low_speed_threshold_bytes_per_second: 8 * 1024,
            low_speed_window: Duration::from_secs(20),
            bulk_hoster_stall_timeout: Duration::from_secs(25),
            max_low_speed_retries: 2,
            speed_smoothing_alpha: 0.25,
            adaptive_ramp_step: 0,
            adaptive_ramp_interval: Duration::from_millis(1500),
        },
    },
    HttpPerformanceModeConfig {
        mode: DownloadPerformanceMode::Fast,
        profile: DownloadPerformanceProfile {
            initial_segments: 16,
            soft_max_segments: 32,
            max_segments: 64,
            min_segmented_size: FAST_MIN_SEGMENTED_SIZE,
            target_segment_size: FAST_TARGET_SEGMENT_SIZE,
            low_speed_threshold_bytes_per_second: 16 * 1024,
            low_speed_window: Duration::from_secs(15),
            bulk_hoster_stall_timeout: Duration::from_secs(15),
            max_low_speed_retries: 3,
            speed_smoothing_alpha: 0.75,
            adaptive_ramp_step: 4,
            adaptive_ramp_interval: Duration::from_secs(2),
        },
    },
];

impl HttpTransferPolicy {
    pub(super) fn for_mode(mode: DownloadPerformanceMode) -> Self {
        let profile = HTTP_PERFORMANCE_PROFILES
            .iter()
            .find(|config| config.mode == mode)
            .map(|config| config.profile)
            .unwrap_or_else(|| {
                HTTP_PERFORMANCE_PROFILES
                    .iter()
                    .find(|config| config.mode == DownloadPerformanceMode::Balanced)
                    .expect("balanced HTTP transfer policy should exist")
                    .profile
            });

        Self { mode, profile }
    }
}

#[cfg(test)]
pub(super) fn performance_profile(mode: DownloadPerformanceMode) -> DownloadPerformanceProfile {
    HttpTransferPolicy::for_mode(mode).profile
}

#[cfg(test)]
pub(super) fn profile_for_effective_http_url(
    mode: DownloadPerformanceMode,
    effective_url: &str,
) -> DownloadPerformanceProfile {
    profile_for_effective_http_url_with_pressure_key_at(mode, effective_url, None, Instant::now())
}

#[cfg(test)]
pub(super) fn profile_for_effective_http_url_at(
    mode: DownloadPerformanceMode,
    effective_url: &str,
    now: Instant,
) -> DownloadPerformanceProfile {
    profile_for_effective_http_url_with_pressure_key_at(mode, effective_url, None, now)
}

pub(super) fn profile_for_effective_http_url_with_pressure_key_at(
    mode: DownloadPerformanceMode,
    effective_url: &str,
    pressure_key: Option<&str>,
    now: Instant,
) -> DownloadPerformanceProfile {
    let mut profile = HttpTransferPolicy::for_mode(mode).profile;
    if mode == DownloadPerformanceMode::Fast && is_gofile_direct_http_url(effective_url) {
        profile.initial_segments = profile.initial_segments.min(8);
        profile.soft_max_segments = profile.soft_max_segments.min(16);
        profile.max_segments = profile.max_segments.min(16);
        profile.adaptive_ramp_step = profile.adaptive_ramp_step.min(4);
    }
    if mode == DownloadPerformanceMode::Fast {
        let score = pressure_key
            .and_then(|key| segment_host_score_snapshot(key, now))
            .or_else(|| segment_host_score_snapshot(effective_url, now));
        if let Some(score) = score {
            let _has_recent_failure = score.recent_reconnects > 0
                || score.recent_rate_limits > 0
                || score.last_failure_reason.is_some();
            if let Some(rate_limited_cap) = score.rate_limited_cap {
                let cap = rate_limited_cap
                    .max(SEGMENT_PRESSURE_MIN_TARGET)
                    .min(profile.max_segments);
                profile.initial_segments = profile.initial_segments.min(cap).max(1);
                profile.soft_max_segments = profile.soft_max_segments.min(cap).max(1);
                profile.max_segments = profile.max_segments.min(cap).max(1);
            } else if score.best_cap != usize::MAX {
                let scored_cap = score.best_cap.max(profile.initial_segments).max(1);
                profile.max_segments = profile.max_segments.min(scored_cap);
                profile.soft_max_segments = profile.soft_max_segments.min(profile.max_segments);
            }
        }
    }
    profile
}

pub(super) fn is_gofile_direct_http_url(raw_url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(raw_url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    host == "gofile.io" || host.ends_with(".gofile.io")
}

pub(super) fn record_segment_host_success(raw_url: &str, cap: usize, now: Instant) {
    let Some(key) = segment_host_score_key(raw_url) else {
        return;
    };
    let scores = segment_host_scores();
    let Ok(mut scores) = scores.lock() else {
        return;
    };
    expire_segment_host_scores_locked(&mut scores, now);
    let entry = scores.entry(key).or_insert_with(|| SegmentHostScore {
        best_cap: None,
        rate_limited_cap: None,
        recent_reconnects: 0,
        recent_rate_limits: 0,
        rate_limit_window_started_at: None,
        last_failure_reason: None,
        updated_at: now,
    });
    let cap = cap.max(1);
    entry.best_cap = Some(entry.best_cap.map_or(cap, |best_cap| best_cap.max(cap)));
    entry.updated_at = now;
}

pub(super) fn record_segment_host_failure(
    raw_url: &str,
    current_cap: usize,
    reason: &str,
    now: Instant,
) {
    let Some(key) = segment_host_score_key(raw_url) else {
        return;
    };
    let scores = segment_host_scores();
    let Ok(mut scores) = scores.lock() else {
        return;
    };
    expire_segment_host_scores_locked(&mut scores, now);
    let cap = current_cap.max(1);
    let entry = scores.entry(key).or_insert_with(|| SegmentHostScore {
        best_cap: None,
        rate_limited_cap: None,
        recent_reconnects: 0,
        recent_rate_limits: 0,
        rate_limit_window_started_at: None,
        last_failure_reason: None,
        updated_at: now,
    });
    entry.best_cap = Some(
        entry
            .best_cap
            .map_or(cap, |best_cap| best_cap.min(cap).max(1)),
    );
    entry.recent_reconnects = entry.recent_reconnects.saturating_add(1);
    entry.last_failure_reason = Some(reason.to_string());
    entry.updated_at = now;
}

pub(super) fn record_segment_reconnect_pressure_for_error(
    pressure_key: &str,
    current_target: usize,
    error: &DownloadError,
    now: Instant,
) -> SegmentPressureDecision {
    if error.http_status != Some(StatusCode::TOO_MANY_REQUESTS) {
        return SegmentPressureDecision::default();
    }

    record_segment_rate_limit_pressure(pressure_key, current_target, now)
}

fn record_segment_rate_limit_pressure(
    pressure_key: &str,
    current_target: usize,
    now: Instant,
) -> SegmentPressureDecision {
    let Some(key) = segment_host_score_key(pressure_key) else {
        return SegmentPressureDecision::default();
    };
    let scores = segment_host_scores();
    let Ok(mut scores) = scores.lock() else {
        return SegmentPressureDecision::default();
    };
    expire_segment_host_scores_locked(&mut scores, now);

    let current_target = current_target.max(1);
    let entry = scores.entry(key).or_insert_with(|| SegmentHostScore {
        best_cap: None,
        rate_limited_cap: None,
        recent_reconnects: 0,
        recent_rate_limits: 0,
        rate_limit_window_started_at: None,
        last_failure_reason: None,
        updated_at: now,
    });
    if entry
        .rate_limit_window_started_at
        .is_none_or(|started| now.saturating_duration_since(started) > SEGMENT_RATE_LIMIT_WINDOW)
    {
        entry.recent_rate_limits = 0;
        entry.rate_limit_window_started_at = Some(now);
    }

    entry.recent_rate_limits = entry.recent_rate_limits.saturating_add(1);
    entry.recent_reconnects = entry.recent_reconnects.saturating_add(1);
    entry.last_failure_reason = Some("HTTP 429 rate limit".into());
    entry.updated_at = now;

    let reduced_target = if entry.recent_rate_limits >= SEGMENT_RATE_LIMIT_THRESHOLD
        && entry.recent_rate_limits % SEGMENT_RATE_LIMIT_THRESHOLD == 0
        && current_target > SEGMENT_PRESSURE_MIN_TARGET
    {
        let reduced = (current_target / 2)
            .max(SEGMENT_PRESSURE_MIN_TARGET)
            .min(current_target.saturating_sub(1));
        entry.rate_limited_cap = Some(
            entry
                .rate_limited_cap
                .map_or(reduced, |cap| cap.min(reduced)),
        );
        Some(reduced)
    } else {
        None
    };

    SegmentPressureDecision {
        recent_rate_limits: entry.recent_rate_limits,
        reduced_target,
    }
}

pub(super) fn segment_host_score_snapshot(
    raw_url: &str,
    now: Instant,
) -> Option<SegmentHostScoreSnapshot> {
    let key = segment_host_score_key(raw_url)?;
    let scores = segment_host_scores();
    let Ok(mut scores) = scores.lock() else {
        return None;
    };
    expire_segment_host_scores_locked(&mut scores, now);
    let score = scores.get(&key)?;
    Some(SegmentHostScoreSnapshot {
        best_cap: score
            .best_cap
            .into_iter()
            .chain(score.rate_limited_cap)
            .min()
            .unwrap_or(usize::MAX),
        rate_limited_cap: score.rate_limited_cap,
        recent_reconnects: score.recent_reconnects,
        recent_rate_limits: score.recent_rate_limits,
        last_failure_reason: score.last_failure_reason.clone(),
    })
}

#[cfg(test)]
pub(super) fn reset_segment_host_scores_for_tests() {
    if let Ok(mut scores) = segment_host_scores().lock() {
        scores.clear();
    }
}

fn segment_host_scores() -> &'static StdMutex<HashMap<String, SegmentHostScore>> {
    SEGMENT_HOST_SCORES.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn expire_segment_host_scores_locked(scores: &mut HashMap<String, SegmentHostScore>, now: Instant) {
    scores.retain(|_, score| {
        now.saturating_duration_since(score.updated_at) < SEGMENT_HOST_SCORE_TTL
    });
}

fn segment_host_score_key(raw_url: &str) -> Option<String> {
    let trimmed = raw_url.trim();
    if trimmed.starts_with("hoster:") {
        return Some(trimmed.to_string());
    }
    let parsed = reqwest::Url::parse(raw_url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    Some(format!(
        "{}://{}:{}",
        parsed.scheme(),
        host,
        parsed.port_or_known_default().unwrap_or(0)
    ))
}

pub(super) fn retry_decision_for_attempt_error(
    error: &DownloadError,
    retry_attempts: u32,
    max_retry_attempts: u32,
    has_valid_partial: bool,
) -> RetryDecision {
    if error.retryable && retry_attempts < max_retry_attempts {
        return RetryDecision::Retry;
    }

    if retry_exhaustion_can_pause_recoverably(error, has_valid_partial) {
        RetryDecision::PauseRecoverably
    } else {
        RetryDecision::Fail
    }
}

pub(super) fn retry_exhaustion_can_pause_recoverably(
    error: &DownloadError,
    has_valid_partial: bool,
) -> bool {
    has_valid_partial
        && (error.category == FailureCategory::Resume
            || (error.retryable
                && matches!(
                    error.category,
                    FailureCategory::Network | FailureCategory::Server | FailureCategory::Http
                )))
}

pub(super) fn recoverable_retry_pause_message(error: &DownloadError, attempts: u32) -> String {
    if error.category == FailureCategory::Resume {
        return format!(
            "Resume was refused and partial data preserved. Use Restart to download from zero. Last error: {}",
            error.message
        );
    }

    format!(
        "Network remained unstable after {attempts} retry attempts. Paused with partial data preserved; resume the download to continue. Last error: {}",
        error.message
    )
}
