use crate::storage::{
    default_download_directory, default_extension_listen_port,
    default_torrent_download_directory_for, default_torrent_port_forwarding_port,
    load_persisted_state, normalize_bulk_settings_for_download_directory, persist_state,
    BulkArchiveInfo, BulkArchiveOutputKind, BulkArchiveStatus, BulkFinalizeMode,
    BulkHosterFairnessMode, ConnectionState, DesktopSnapshot, DiagnosticEvent, DiagnosticLevel,
    DiagnosticsSnapshot, DownloadJob, DownloadPerformanceMode, DownloadPrompt, DownloadSource,
    ExtensionIntegrationSettings, FailureCategory, HandoffAuth, HandoffAuthHeader,
    HostRegistrationDiagnostics, HosterPreflightInfo, IntegrityAlgorithm, IntegrityCheck,
    IntegrityStatus, JobState, MainWindowState, PersistedState, ProtectedDownloadAuthScope,
    QueueSummary, ResumeSupport, Settings, TorrentInfo, TorrentJobDiagnostics, TorrentSeedMode,
    TorrentSettings, TransferKind,
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
    BulkMemberAutoRestartCandidate, BulkMemberRetryCandidate, BulkMemberSlowRecoveryState,
    DownloadTask, DuplicatePolicy, EnqueueOptions, EnqueueResult, EnqueueStatus,
    ExternalReseedAttempt, ExternalUsePreparation, TorrentRemovalCleanupInfo, TorrentRuntimePhase,
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
const BULK_HOSTER_AGGREGATE_DEGRADATION_WINDOW: Duration = Duration::from_secs(20);
const BULK_HOSTER_FAIRNESS_COOLDOWN_WINDOW: Duration = Duration::from_secs(30);
const BULK_HOSTER_MAX_ADAPTIVE_CONCURRENCY: u32 = 4;
const BULK_HOSTER_AGGREGATE_DEGRADATION_PERCENT: u64 = 35;
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
    bulk_hoster_fairness: BulkHosterFairnessController,
    external_reseed_jobs: HashSet<String>,
    last_host_contact: Option<Instant>,
    last_progress_persist_at: Option<Instant>,
}

#[derive(Debug, Clone)]
struct BulkHosterWorkerHealth {
    _claimed_at: Instant,
    resolver_started_at: Option<Instant>,
    transfer_started_at: Option<Instant>,
    phase: BulkHosterWorkerPhase,
    last_progress_at: Instant,
    last_downloaded_bytes: u64,
    last_reported_speed: u64,
    low_speed_since: Option<Instant>,
    healthy_sample_count: u8,
    last_healthy_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BulkHosterWorkerPhase {
    Claimed,
    Resolving,
    Transferring,
}

impl BulkHosterWorkerHealth {
    fn from_job(job: &DownloadJob, now: Instant) -> Self {
        Self {
            _claimed_at: now,
            resolver_started_at: None,
            transfer_started_at: None,
            phase: BulkHosterWorkerPhase::Claimed,
            last_progress_at: now,
            last_downloaded_bytes: job.downloaded_bytes,
            last_reported_speed: job.speed,
            low_speed_since: None,
            healthy_sample_count: 0,
            last_healthy_at: None,
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

        if speed < BULK_HOSTER_HEALTH_FLOOR_BYTES_PER_SECOND {
            self.low_speed_since.get_or_insert(now);
            self.healthy_sample_count = 0;
        } else {
            self.low_speed_since = None;
            if progressed {
                self.healthy_sample_count = self.healthy_sample_count.saturating_add(1);
                self.last_healthy_at = Some(now);
            }
        }
    }

    fn blocks_bulk_hoster_claim(&self, now: Instant) -> bool {
        if self.phase != BulkHosterWorkerPhase::Transferring {
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
            && self.transfer_started_at.is_some_and(|started| {
                now.saturating_duration_since(started) >= BULK_HOSTER_STARTUP_GRACE_WINDOW
            })
            && self.last_reported_speed >= BULK_HOSTER_HEALTH_FLOOR_BYTES_PER_SECOND
            && self.healthy_sample_count >= 2
            && self.last_healthy_at.is_some_and(|last| {
                now.saturating_duration_since(last)
                    <= BULK_HOSTER_LOW_SPEED_WINDOW + BULK_HOSTER_HEALTH_SAMPLE_WINDOW
            })
            && now.saturating_duration_since(self.last_progress_at) < BULK_HOSTER_LOW_SPEED_WINDOW
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

#[derive(Debug, Clone, Copy, Default)]
struct BulkHosterFairnessMetrics {
    active_count: u32,
    aggregate_speed: u64,
    all_healthy: bool,
    has_blocking_worker: bool,
}

impl BulkHosterFairnessController {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn target_for_bulk_limit(&self, bulk_slot_limit: u32) -> u32 {
        self.target_active
            .max(1)
            .min(bulk_slot_limit.max(1))
            .min(BULK_HOSTER_MAX_ADAPTIVE_CONCURRENCY)
    }

    fn reconcile(
        &mut self,
        metrics: BulkHosterFairnessMetrics,
        bulk_slot_limit: u32,
        now: Instant,
    ) -> Vec<String> {
        let max_target = bulk_slot_limit
            .max(1)
            .min(BULK_HOSTER_MAX_ADAPTIVE_CONCURRENCY);
        self.target_active = self.target_active.max(1).min(max_target);

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
                diagnostics.push(
                    "Adaptive fairness froze protected bulk hoster claims while an active hoster row is warming up or slow."
                        .into(),
                );
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

#[cfg(test)]
mod tests;
