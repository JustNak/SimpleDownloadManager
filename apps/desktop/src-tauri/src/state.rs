use crate::storage::{
    default_download_directory, default_extension_listen_port,
    default_torrent_download_directory_for, default_torrent_port_forwarding_port,
    load_persisted_state, normalize_bulk_settings_for_download_directory, persist_state,
    BulkArchiveInfo, BulkArchiveOutputKind, BulkArchiveStatus, BulkFinalizeMode,
    BulkHosterAccelerationMode, BulkHosterFairnessMode, ConnectionState, DesktopSnapshot,
    DiagnosticEvent, DiagnosticLevel, DiagnosticsSnapshot, DownloadJob, DownloadPerformanceMode,
    DownloadPrompt, DownloadSource, ExtensionIntegrationSettings, FailureCategory, HandoffAuth,
    HandoffAuthHeader, HostRegistrationDiagnostics, HosterPreflightInfo, HosterPreflightStatus,
    IntegrityAlgorithm, IntegrityCheck, IntegrityStatus, JobState, MainWindowState, PersistedState,
    ProtectedDownloadAuthScope, QueueSummary, RemovalState, ResumeSupport, Settings, TorrentInfo,
    TorrentJobDiagnostics, TorrentSeedMode, TorrentSettings, TransferKind,
};
use percent_encoding::percent_decode_str;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use url::Url;

mod enqueue;
mod jobs;
mod lifecycle;
mod paths;
mod progress;
mod runtime;
mod scheduler;
mod settings;
mod torrent;
mod types;
use jobs::*;
use paths::*;
#[cfg(test)]
use progress::*;
use runtime::*;
pub use settings::validate_settings;
use settings::*;
#[cfg(test)]
pub(crate) use torrent::pending_torrent_session_cache_clear_path;
pub(crate) use torrent::{
    apply_pending_torrent_session_cache_clear, clear_torrent_session_cache_directory,
    should_stop_seeding,
};
pub use types::{
    BackendError, BatchDownloadEntry, BulkArchiveEntry, BulkArchiveReady,
    BulkMemberAutoRestartCandidate, BulkMemberAutoRestartMode, BulkMemberRetryCandidate,
    BulkMemberSlowRecoveryState, DestructiveCleanupJob, DestructiveCleanupPlan, DownloadTask,
    DuplicatePolicy, EnqueueOptions, EnqueueResult, EnqueueStatus, ExternalReseedAttempt,
    ExternalUsePreparation, HosterWarmupCandidate, TorrentRemovalCleanupInfo, TorrentRuntimePhase,
    TorrentRuntimeSnapshot, TorrentSeedingRestoreFailure, TorrentSessionCacheClearResult,
    TorrentSessionCacheClearState, WorkerControl,
};

const MAX_URL_LENGTH: usize = 2048;
const SHA256_HEX_LENGTH: usize = 64;
const DIAGNOSTIC_EVENT_LIMIT: usize = 100;
const MAX_TORRENT_UPLOAD_LIMIT_KIB_PER_SECOND: u32 = 1_048_576;
const MIN_TORRENT_FORWARDING_PORT: u32 = 1024;
const MAX_TORRENT_FORWARDING_PORT: u32 = 65_534;
const EXTERNAL_USE_HANDLE_RELEASE_TIMEOUT: Duration = Duration::from_secs(5);
const EXTERNAL_USE_HANDLE_RELEASE_POLL: Duration = Duration::from_millis(50);
const MAX_HANDOFF_AUTH_HEADERS: usize = 16;
const MAX_HANDOFF_AUTH_HEADER_NAME_LENGTH: usize = 64;
const MAX_HANDOFF_AUTH_HEADER_VALUE_LENGTH: usize = 16 * 1024;
const PROGRESS_PERSIST_COALESCE_WINDOW: Duration = Duration::from_secs(1);
const BULK_HOSTER_STARTUP_GRACE_WINDOW: Duration = Duration::from_secs(20);
const BULK_HOSTER_LOW_SPEED_WINDOW: Duration = Duration::from_secs(20);
const BULK_HOSTER_HEALTH_FLOOR_BYTES_PER_SECOND: u64 = 64 * 1024;
const BULK_HOSTER_HEALTH_SAMPLE_WINDOW: Duration = Duration::from_secs(1);
const BULK_HOSTER_TRANSIENT_LOW_SAMPLE_GRACE: Duration = Duration::from_secs(3);
const BULK_HOSTER_AGGREGATE_DEGRADATION_WINDOW: Duration = Duration::from_secs(20);
const BULK_HOSTER_FAIRNESS_COOLDOWN_WINDOW: Duration = Duration::from_secs(30);
const BULK_HOSTER_MAX_ADAPTIVE_CONCURRENCY: u32 = 4;
const BULK_HOSTER_AGGREGATE_DEGRADATION_PERCENT: u64 = 35;
const DATANODES_PRIORITY_PRESSURE_WINDOW: Duration = Duration::from_secs(6);
const DATANODES_PRIORITY_BALANCED_RUNWAY: Duration = Duration::from_secs(8);
const DATANODES_PRIORITY_FAST_RUNWAY: Duration = Duration::from_secs(5);
const DATANODES_PRIORITY_MIN_SPEED_BYTES_PER_SECOND: u64 = 128 * 1024;
const DATANODES_PRIORITY_BASELINE_SPEED_PERCENT: u64 = 75;
const DATANODES_PRIORITY_REQUIRED_HEALTHY_SAMPLES: u8 = 3;
const HOSTER_PRIORITY_CAP_REPORT_CHANGE_PERCENT: u64 = 25;
const DOWNLOAD_CATEGORY_FOLDERS: [&str; 7] = [
    "Document",
    "Program",
    "Picture",
    "Video",
    "Compressed",
    "Music",
    "Other",
];
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "csv", "md", "epub",
];
const PROGRAM_EXTENSIONS: &[&str] = &["exe", "msi", "apk", "dmg", "pkg", "deb", "rpm", "appimage"];
const PICTURE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "tif", "tiff", "heic",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv", "flv"];
const COMPRESSED_EXTENSIONS: &[&str] = &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "tgz"];
const MUSIC_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "ogg", "m4a", "aac", "opus", "wma"];

#[derive(Debug)]
struct RuntimeState {
    connection_state: ConnectionState,
    jobs: Vec<DownloadJob>,
    settings: Settings,
    main_window: Option<MainWindowState>,
    diagnostic_events: Vec<DiagnosticEvent>,
    next_job_number: u64,
    job_indexes: HashMap<String, usize>,
    active_workers: HashSet<String>,
    bulk_hoster_worker_health: HashMap<String, BulkHosterWorkerHealth>,
    bulk_hoster_fairness: HashMap<String, BulkHosterFairnessController>,
    datanodes_priority_defer_until: HashMap<String, Instant>,
    hoster_priority_cap_reports: HashMap<String, HosterPriorityCapReport>,
    external_reseed_jobs: HashSet<String>,
    last_host_contact: Option<Instant>,
    last_progress_persist_at: Option<Instant>,
}

#[derive(Debug, Clone)]
struct BulkHosterWorkerHealth {
    profile: BulkHosterWorkerProfile,
    claimed_at: Instant,
    resolver_started_at: Option<Instant>,
    transfer_started_at: Option<Instant>,
    phase: BulkHosterWorkerPhase,
    last_progress_at: Instant,
    last_downloaded_bytes: u64,
    last_reported_speed: u64,
    low_speed_since: Option<Instant>,
    healthy_sample_count: u8,
    last_healthy_at: Option<Instant>,
    priority_peak_speed: u64,
    priority_baseline_speed: Option<u64>,
    priority_pressure_since: Option<Instant>,
    priority_pressure_active: bool,
    priority_recovery_sample_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BulkHosterWorkerPhase {
    Claimed,
    Resolving,
    Transferring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BulkHosterWorkerProfile {
    Conservative,
    Accelerated { max_concurrency: u32 },
}

impl BulkHosterWorkerHealth {
    #[cfg(test)]
    fn from_job(job: &DownloadJob, now: Instant) -> Self {
        Self::from_job_with_profile(job, BulkHosterWorkerProfile::Conservative, now)
    }

    fn from_job_with_profile(
        job: &DownloadJob,
        profile: BulkHosterWorkerProfile,
        now: Instant,
    ) -> Self {
        Self {
            profile,
            claimed_at: now,
            resolver_started_at: None,
            transfer_started_at: None,
            phase: BulkHosterWorkerPhase::Claimed,
            last_progress_at: now,
            last_downloaded_bytes: job.downloaded_bytes,
            last_reported_speed: job.speed,
            low_speed_since: None,
            healthy_sample_count: 0,
            last_healthy_at: None,
            priority_peak_speed: job.speed,
            priority_baseline_speed: None,
            priority_pressure_since: None,
            priority_pressure_active: false,
            priority_recovery_sample_count: 0,
        }
    }

    fn mark_resolving(&mut self, now: Instant) {
        self.resolver_started_at.get_or_insert(now);
        if self.phase == BulkHosterWorkerPhase::Claimed {
            self.phase = BulkHosterWorkerPhase::Resolving;
        }
    }

    fn mark_transferring(&mut self, downloaded_bytes: u64, now: Instant) {
        self.transfer_started_at.get_or_insert(now);
        self.phase = BulkHosterWorkerPhase::Transferring;
        self.last_progress_at = now;
        self.last_downloaded_bytes = downloaded_bytes;
        self.last_reported_speed = 0;
        self.low_speed_since = Some(now);
        self.healthy_sample_count = 0;
        self.last_healthy_at = None;
        self.priority_pressure_since = None;
        self.priority_pressure_active = false;
        self.priority_recovery_sample_count = 0;
    }

    fn update(&mut self, downloaded_bytes: u64, speed: u64, now: Instant) {
        if self.phase != BulkHosterWorkerPhase::Transferring {
            self.mark_transferring(self.last_downloaded_bytes, now);
        }
        let progressed = downloaded_bytes > self.last_downloaded_bytes;
        if progressed {
            self.last_progress_at = now;
        }
        self.last_downloaded_bytes = downloaded_bytes;
        self.last_reported_speed = speed;
        self.priority_peak_speed = self.priority_peak_speed.max(speed);

        if speed < BULK_HOSTER_HEALTH_FLOOR_BYTES_PER_SECOND {
            let low_speed_since = *self.low_speed_since.get_or_insert(now);
            if now.saturating_duration_since(low_speed_since)
                > BULK_HOSTER_TRANSIENT_LOW_SAMPLE_GRACE
            {
                self.healthy_sample_count = 0;
            }
        } else {
            self.low_speed_since = None;
            if progressed {
                self.healthy_sample_count = self.healthy_sample_count.saturating_add(1);
                self.last_healthy_at = Some(now);
            }
        }

        let priority_target = self
            .priority_baseline_speed
            .map(datanodes_priority_pressure_floor)
            .unwrap_or(DATANODES_PRIORITY_MIN_SPEED_BYTES_PER_SECOND);
        if progressed && speed >= priority_target {
            self.priority_baseline_speed = Some(
                self.priority_baseline_speed
                    .map(|average| {
                        average
                            .saturating_mul(3)
                            .saturating_add(speed)
                            .saturating_div(4)
                    })
                    .unwrap_or(speed),
            );
        }

        if let Some(baseline_speed) = self.priority_baseline_speed {
            let priority_target = datanodes_priority_pressure_floor(baseline_speed);
            if speed < priority_target {
                self.priority_pressure_since.get_or_insert(now);
                self.priority_recovery_sample_count = 0;
                if self.priority_pressure_since.is_some_and(|since| {
                    now.saturating_duration_since(since) >= DATANODES_PRIORITY_PRESSURE_WINDOW
                }) {
                    self.priority_pressure_active = true;
                }
            } else if self.priority_pressure_active {
                self.priority_recovery_sample_count =
                    self.priority_recovery_sample_count.saturating_add(1);
                if self.priority_recovery_sample_count
                    >= DATANODES_PRIORITY_REQUIRED_HEALTHY_SAMPLES
                {
                    self.priority_pressure_since = None;
                    self.priority_pressure_active = false;
                    self.priority_recovery_sample_count = 0;
                }
            } else {
                self.priority_pressure_since = None;
                self.priority_recovery_sample_count = 0;
            }
        }
    }

    fn blocks_bulk_hoster_claim(&self, now: Instant) -> bool {
        if self.phase != BulkHosterWorkerPhase::Transferring {
            return true;
        }

        if matches!(self.profile, BulkHosterWorkerProfile::Accelerated { .. }) {
            if self.accelerated_claim_ready(now) || self.accelerated_transient_low_ready(now) {
                return false;
            }
            return true;
        }

        let Some(transfer_started_at) = self.transfer_started_at else {
            return true;
        };
        if now.saturating_duration_since(transfer_started_at) < BULK_HOSTER_STARTUP_GRACE_WINDOW {
            return true;
        }

        if self.last_reported_speed >= BULK_HOSTER_HEALTH_FLOOR_BYTES_PER_SECOND {
            return false;
        }

        let sustained_low_speed = self.low_speed_since.is_some_and(|since| {
            now.saturating_duration_since(since) >= BULK_HOSTER_LOW_SPEED_WINDOW
        });
        let stalled_progress =
            now.saturating_duration_since(self.last_progress_at) >= BULK_HOSTER_LOW_SPEED_WINDOW;

        sustained_low_speed || stalled_progress
    }

    fn is_healthy(&self, now: Instant) -> bool {
        self.phase == BulkHosterWorkerPhase::Transferring
            && self.profile_allows_healthy_sample_window(now)
            && (self.accelerated_claim_ready(now)
                || self.recent_healthy_samples(now)
                || self.accelerated_transient_low_ready(now)
                || (!matches!(self.profile, BulkHosterWorkerProfile::Accelerated { .. })
                    && self.transient_low_sample_after_health(now)))
    }

    fn priority_started_at(&self) -> Instant {
        self.transfer_started_at
            .or(self.resolver_started_at)
            .unwrap_or(self.claimed_at)
    }

    fn accelerated_claim_ready(&self, now: Instant) -> bool {
        let BulkHosterWorkerProfile::Accelerated { max_concurrency } = self.profile else {
            return self.recent_healthy_samples(now);
        };
        let Some(transfer_started_at) = self.transfer_started_at else {
            return false;
        };
        self.healthy_sample_count >= DATANODES_PRIORITY_REQUIRED_HEALTHY_SAMPLES
            && now.saturating_duration_since(transfer_started_at)
                >= datanodes_priority_runway(max_concurrency)
            && self.recent_healthy_samples(now)
    }

    fn accelerated_transient_low_ready(&self, now: Instant) -> bool {
        let BulkHosterWorkerProfile::Accelerated { max_concurrency } = self.profile else {
            return self.transient_low_sample_after_health(now);
        };
        let Some(transfer_started_at) = self.transfer_started_at else {
            return false;
        };
        self.healthy_sample_count >= DATANODES_PRIORITY_REQUIRED_HEALTHY_SAMPLES
            && now.saturating_duration_since(transfer_started_at)
                >= datanodes_priority_runway(max_concurrency)
            && self.transient_low_sample_after_health(now)
    }

    fn datanodes_priority_pressure(&self, now: Instant) -> Option<DataNodesPriorityPressureSample> {
        let BulkHosterWorkerProfile::Accelerated { .. } = self.profile else {
            return None;
        };
        let baseline_speed = self.priority_baseline_speed?;
        let pressure_since = self.priority_pressure_since?;
        if !self.priority_pressure_active
            && now.saturating_duration_since(pressure_since) < DATANODES_PRIORITY_PRESSURE_WINDOW
        {
            return None;
        }
        let target_speed = datanodes_priority_pressure_floor(baseline_speed);
        Some(DataNodesPriorityPressureSample {
            current_speed: self.last_reported_speed,
            peak_speed: self.priority_peak_speed,
            baseline_speed,
            target_speed,
        })
    }

    fn hoster_priority_reference(&self) -> Option<HosterPrioritySpeedReference> {
        if self.phase != BulkHosterWorkerPhase::Transferring {
            return None;
        }

        let baseline_speed = self.priority_baseline_speed.unwrap_or(0);
        let target_speed = self
            .priority_baseline_speed
            .map(datanodes_priority_pressure_floor)
            .unwrap_or(0);
        let reference_bytes_per_second = if self.last_reported_speed > 0 {
            self.last_reported_speed
        } else {
            self.priority_baseline_speed?
        };

        Some(HosterPrioritySpeedReference {
            current_speed: self.last_reported_speed,
            peak_speed: self.priority_peak_speed,
            baseline_speed,
            target_speed,
            reference_bytes_per_second,
        })
    }

    fn recent_healthy_samples(&self, now: Instant) -> bool {
        self.last_reported_speed >= BULK_HOSTER_HEALTH_FLOOR_BYTES_PER_SECOND
            && self.recent_healthy_progress(now)
    }

    fn transient_low_sample_after_health(&self, now: Instant) -> bool {
        self.last_reported_speed < BULK_HOSTER_HEALTH_FLOOR_BYTES_PER_SECOND
            && self.recent_healthy_progress(now)
            && self.low_speed_since.is_some_and(|since| {
                now.saturating_duration_since(since) <= BULK_HOSTER_TRANSIENT_LOW_SAMPLE_GRACE
            })
    }

    fn recent_healthy_progress(&self, now: Instant) -> bool {
        self.phase == BulkHosterWorkerPhase::Transferring
            && self.healthy_sample_count >= 2
            && self.last_healthy_at.is_some_and(|last| {
                now.saturating_duration_since(last)
                    <= BULK_HOSTER_LOW_SPEED_WINDOW + BULK_HOSTER_HEALTH_SAMPLE_WINDOW
            })
            && now.saturating_duration_since(self.last_progress_at) < BULK_HOSTER_LOW_SPEED_WINDOW
    }

    fn profile_allows_healthy_sample_window(&self, now: Instant) -> bool {
        match self.profile {
            BulkHosterWorkerProfile::Conservative => {
                self.transfer_started_at.is_some_and(|started| {
                    now.saturating_duration_since(started) >= BULK_HOSTER_STARTUP_GRACE_WINDOW
                })
            }
            BulkHosterWorkerProfile::Accelerated { .. } => true,
        }
    }
}

#[derive(Debug, Clone)]
struct BulkHosterFairnessController {
    target_active: u32,
    aggregate_baseline_speed: Option<u64>,
    degraded_since: Option<Instant>,
    cooldown_until: Option<Instant>,
    last_freeze_reported_at: Option<Instant>,
}

impl Default for BulkHosterFairnessController {
    fn default() -> Self {
        Self {
            target_active: 1,
            aggregate_baseline_speed: None,
            degraded_since: None,
            cooldown_until: None,
            last_freeze_reported_at: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct BulkHosterFairnessMetrics {
    active_count: u32,
    aggregate_speed: u64,
    all_healthy: bool,
    has_blocking_worker: bool,
    priority_pressure: Option<DataNodesPriorityPressure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DataNodesPriorityPressure {
    pub older_job_id: String,
    pub current_speed: u64,
    pub peak_speed: u64,
    pub baseline_speed: u64,
    pub target_speed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HosterPriorityThrottleDecision {
    pub protected_job_id: String,
    pub current_speed: u64,
    pub peak_speed: u64,
    pub baseline_speed: u64,
    pub target_speed: u64,
    pub reference_bytes_per_second: u64,
    pub cap_bytes_per_second: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DataNodesPriorityPressureSample {
    current_speed: u64,
    peak_speed: u64,
    baseline_speed: u64,
    target_speed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HosterPrioritySpeedReference {
    current_speed: u64,
    peak_speed: u64,
    baseline_speed: u64,
    target_speed: u64,
    reference_bytes_per_second: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HosterPriorityCapReport {
    protected_job_id: String,
    cap_bytes_per_second: u64,
}

impl BulkHosterFairnessController {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn target_for_bulk_limit(&self, bulk_slot_limit: u32, max_adaptive_concurrency: u32) -> u32 {
        self.target_active
            .max(1)
            .min(bulk_slot_limit.max(1))
            .min(max_adaptive_concurrency.max(1))
    }

    fn reconcile(
        &mut self,
        metrics: &BulkHosterFairnessMetrics,
        bulk_slot_limit: u32,
        max_adaptive_concurrency: u32,
        now: Instant,
    ) -> Vec<String> {
        let max_target = bulk_slot_limit.clamp(1, max_adaptive_concurrency.max(1));
        self.target_active = self.target_active.clamp(1, max_target);

        if metrics.active_count == 0 {
            self.reset();
            return Vec::new();
        }

        let mut diagnostics = Vec::new();
        let cooldown_active = self.cooldown_until.is_some_and(|until| now < until);
        let mut downshifted = false;

        if metrics.active_count >= 2 && metrics.all_healthy {
            if let Some(baseline) = self.aggregate_baseline_speed {
                let degraded_threshold = baseline.saturating_mul(
                    100_u64.saturating_sub(BULK_HOSTER_AGGREGATE_DEGRADATION_PERCENT),
                ) / 100;
                if metrics.aggregate_speed < degraded_threshold {
                    let degraded_since = *self.degraded_since.get_or_insert(now);
                    if now.saturating_duration_since(degraded_since)
                        >= BULK_HOSTER_AGGREGATE_DEGRADATION_WINDOW
                        && self.target_active > 1
                    {
                        self.target_active = self.target_active.saturating_sub(1).max(1);
                        self.cooldown_until = Some(now + BULK_HOSTER_FAIRNESS_COOLDOWN_WINDOW);
                        self.aggregate_baseline_speed = Some(metrics.aggregate_speed.max(1));
                        self.degraded_since = None;
                        downshifted = true;
                        diagnostics.push(format!(
                            "Adaptive fairness downshifted protected bulk hoster concurrency to {} after aggregate speed dropped from {} B/s to {} B/s.",
                            self.target_active, baseline, metrics.aggregate_speed
                        ));
                    }
                } else {
                    self.degraded_since = None;
                    if metrics.aggregate_speed > baseline {
                        self.aggregate_baseline_speed = Some(metrics.aggregate_speed);
                    }
                }
            } else {
                self.aggregate_baseline_speed = Some(metrics.aggregate_speed.max(1));
            }
        } else if metrics.has_blocking_worker {
            self.degraded_since = None;
        }

        if metrics.has_blocking_worker {
            let should_report_freeze = match self.last_freeze_reported_at {
                Some(last) => {
                    now.saturating_duration_since(last) >= BULK_HOSTER_FAIRNESS_COOLDOWN_WINDOW
                }
                None => true,
            };
            if should_report_freeze {
                self.last_freeze_reported_at = Some(now);
                if let Some(pressure) = metrics.priority_pressure.as_ref() {
                    diagnostics.push(format!(
                        "DataNodes priority blocked newer hoster claims to protect {} at {} B/s after peak {} B/s.",
                        pressure.older_job_id, pressure.current_speed, pressure.peak_speed
                    ));
                } else {
                    diagnostics.push(
                        "Adaptive fairness froze protected bulk hoster claims while an active hoster row is warming up or slow."
                            .into(),
                    );
                }
            }
            return diagnostics;
        }

        if metrics.all_healthy
            && !cooldown_active
            && !downshifted
            && metrics.active_count >= self.target_active
            && self.target_active < max_target
        {
            self.target_active += 1;
            self.aggregate_baseline_speed = Some(metrics.aggregate_speed.max(1));
            self.degraded_since = None;
            diagnostics.push(format!(
                "Adaptive fairness ramped protected bulk hoster concurrency to {}.",
                self.target_active
            ));
        }

        diagnostics
    }
}

#[derive(Clone)]
pub struct SharedState {
    inner: Arc<RwLock<RuntimeState>>,
    storage_path: Arc<PathBuf>,
    handoff_auth: Arc<RwLock<HashMap<String, HandoffAuth>>>,
}

fn internal_error(error: String) -> BackendError {
    BackendError {
        code: "INTERNAL_ERROR",
        message: error,
    }
}

fn is_bulk_member_job(job: &DownloadJob) -> bool {
    job.transfer_kind == TransferKind::Http && job.bulk_archive.is_some()
}

fn is_protected_bulk_hoster_job(job: &DownloadJob) -> bool {
    is_bulk_member_job(job) && job.resolved_from_url.is_some()
}

fn is_direct_bulk_http_job(job: &DownloadJob) -> bool {
    is_bulk_member_job(job) && job.resolved_from_url.is_none()
}

fn download_origin_key(raw_url: &str) -> Option<String> {
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

fn protected_bulk_hoster_fairness_key(job: &DownloadJob) -> Option<String> {
    if !is_protected_bulk_hoster_job(job) {
        return None;
    }

    download_origin_key(job.resolved_from_url.as_deref().unwrap_or(&job.url))
}

fn protected_bulk_hoster_priority_group_key(job: &DownloadJob) -> Option<(String, String)> {
    let archive_id = job.bulk_archive.as_ref()?.id.clone();
    let hoster_origin = protected_bulk_hoster_fairness_key(job)?;
    Some((archive_id, hoster_origin))
}

fn bulk_hoster_worker_profile_for_job(
    settings: &Settings,
    job: &DownloadJob,
) -> BulkHosterWorkerProfile {
    datanodes_accelerated_hoster_concurrency(settings, job)
        .map(|max_concurrency| BulkHosterWorkerProfile::Accelerated { max_concurrency })
        .unwrap_or(BulkHosterWorkerProfile::Conservative)
}

fn protected_bulk_hoster_max_adaptive_concurrency_for_key(
    state: &RuntimeState,
    fairness_key: &str,
) -> u32 {
    state
        .jobs
        .iter()
        .filter(|job| protected_bulk_hoster_fairness_key(job).as_deref() == Some(fairness_key))
        .filter_map(|job| datanodes_accelerated_hoster_concurrency(&state.settings, job))
        .max()
        .unwrap_or(BULK_HOSTER_MAX_ADAPTIVE_CONCURRENCY)
}

fn datanodes_accelerated_hoster_concurrency(settings: &Settings, job: &DownloadJob) -> Option<u32> {
    if settings.bulk.hoster_acceleration_mode == BulkHosterAccelerationMode::Off {
        return None;
    }

    let source_url = job.resolved_from_url.as_deref().unwrap_or(&job.url);
    if !crate::hosters::is_datanodes_page_url(source_url) {
        return None;
    }

    match settings.bulk.download_performance_mode {
        DownloadPerformanceMode::Stable => None,
        DownloadPerformanceMode::Balanced => Some(4),
        DownloadPerformanceMode::Fast => Some(8),
    }
}

fn is_accelerated_datanodes_bulk_job(settings: &Settings, job: &DownloadJob) -> bool {
    is_protected_bulk_hoster_job(job)
        && datanodes_accelerated_hoster_concurrency(settings, job).is_some()
}

fn datanodes_priority_runway(max_concurrency: u32) -> Duration {
    if max_concurrency >= 8 {
        DATANODES_PRIORITY_FAST_RUNWAY
    } else {
        DATANODES_PRIORITY_BALANCED_RUNWAY
    }
}

fn datanodes_priority_pressure_floor(peak_speed: u64) -> u64 {
    DATANODES_PRIORITY_MIN_SPEED_BYTES_PER_SECOND
        .max(peak_speed.saturating_mul(DATANODES_PRIORITY_BASELINE_SPEED_PERCENT) / 100)
}

fn datanodes_hoster_warmup_horizon(settings: &Settings) -> Option<usize> {
    if settings.bulk.hoster_acceleration_mode == BulkHosterAccelerationMode::Off {
        return None;
    }

    let mode_limit = match settings.bulk.download_performance_mode {
        DownloadPerformanceMode::Stable => return None,
        DownloadPerformanceMode::Balanced => 4,
        DownloadPerformanceMode::Fast => 8,
    };
    Some(
        (settings.bulk.max_concurrent_downloads.max(1) as usize)
            .min(mode_limit)
            .max(1),
    )
}

#[cfg(test)]
mod tests;
