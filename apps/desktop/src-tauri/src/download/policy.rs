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

const HTTP_PERFORMANCE_PROFILES: [HttpPerformanceModeConfig; 3] = [
    HttpPerformanceModeConfig {
        mode: DownloadPerformanceMode::Stable,
        profile: DownloadPerformanceProfile {
            max_segments: 1,
            min_segmented_size: u64::MAX,
            target_segment_size: u64::MAX,
            low_speed_threshold_bytes_per_second: 4 * 1024,
            low_speed_window: Duration::from_secs(30),
            bulk_hoster_stall_timeout: Duration::from_secs(90),
            max_low_speed_retries: 2,
            speed_smoothing_alpha: 0.25,
        },
    },
    HttpPerformanceModeConfig {
        mode: DownloadPerformanceMode::Balanced,
        profile: DownloadPerformanceProfile {
            max_segments: 6,
            min_segmented_size: BALANCED_MIN_SEGMENTED_SIZE,
            target_segment_size: BALANCED_TARGET_SEGMENT_SIZE,
            low_speed_threshold_bytes_per_second: 8 * 1024,
            low_speed_window: Duration::from_secs(20),
            bulk_hoster_stall_timeout: Duration::from_secs(25),
            max_low_speed_retries: 2,
            speed_smoothing_alpha: 0.25,
        },
    },
    HttpPerformanceModeConfig {
        mode: DownloadPerformanceMode::Fast,
        profile: DownloadPerformanceProfile {
            max_segments: 12,
            min_segmented_size: FAST_MIN_SEGMENTED_SIZE,
            target_segment_size: FAST_TARGET_SEGMENT_SIZE,
            low_speed_threshold_bytes_per_second: 16 * 1024,
            low_speed_window: Duration::from_secs(15),
            bulk_hoster_stall_timeout: Duration::from_secs(15),
            max_low_speed_retries: 3,
            speed_smoothing_alpha: 0.25,
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
    error.retryable
        && has_valid_partial
        && matches!(
            error.category,
            FailureCategory::Network | FailureCategory::Server | FailureCategory::Http
        )
}

pub(super) fn recoverable_retry_pause_message(error: &DownloadError, attempts: u32) -> String {
    format!(
        "Network remained unstable after {attempts} retry attempts. Paused with partial data preserved; resume the download to continue. Last error: {}",
        error.message
    )
}
