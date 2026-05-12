use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DownloadOutcome {
    Completed,
    Paused,
    Canceled,
    Deferred(Duration),
}

#[derive(Debug)]
pub(super) enum TorrentAddOutcome {
    Added(TorrentAddSessionOutcome),
    Interrupted(DownloadOutcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TransferDispatch {
    Http,
    Torrent,
}

pub(super) fn transfer_dispatch_for_kind(kind: TransferKind) -> Option<TransferDispatch> {
    match kind {
        TransferKind::Http => Some(TransferDispatch::Http),
        TransferKind::Torrent => Some(TransferDispatch::Torrent),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ByteRange {
    pub(super) start: u64,
    pub(super) end: u64,
}

impl ByteRange {
    pub(super) fn len(self) -> u64 {
        self.end.saturating_sub(self.start).saturating_add(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RangePlan {
    pub(super) total_bytes: u64,
    pub(super) segments: Vec<ByteRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SegmentProgress {
    pub(super) index: usize,
    pub(super) range: ByteRange,
    #[serde(default)]
    pub(super) downloaded_bytes: u64,
    pub(super) completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SegmentedDownloadState {
    #[serde(default = "default_segment_state_schema_version")]
    pub(super) schema_version: u32,
    pub(super) total_bytes: u64,
    #[serde(default)]
    pub(super) validators: EntityValidators,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) effective_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) target_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) temp_path: Option<String>,
    #[serde(default)]
    pub(super) last_verified_file_len: u64,
    #[serde(default)]
    pub(super) retry_generation: u32,
    pub(super) segments: Vec<SegmentProgress>,
}

pub(super) fn default_segment_state_schema_version() -> u32 {
    2
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct EntityValidators {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) last_modified: Option<String>,
}

impl EntityValidators {
    pub(super) fn if_range_value(&self) -> Option<&str> {
        self.etag.as_deref().or(self.last_modified.as_deref())
    }

    pub(super) fn conflicts_with(&self, latest: &Self) -> bool {
        let etag_changed = self
            .etag
            .as_deref()
            .zip(latest.etag.as_deref())
            .is_some_and(|(stored, remote)| stored != remote);
        let last_modified_changed = self
            .last_modified
            .as_deref()
            .zip(latest.last_modified.as_deref())
            .is_some_and(|(stored, remote)| stored != remote);

        etag_changed || last_modified_changed
    }

    pub(super) fn reconcile_with(&self, latest: &Self) -> Self {
        Self {
            etag: latest.etag.clone().or_else(|| self.etag.clone()),
            last_modified: latest
                .last_modified
                .clone()
                .or_else(|| self.last_modified.clone()),
        }
    }
}

pub(super) struct SegmentedProgressCounters {
    pub(super) segment_bytes: StdMutex<Vec<AtomicU64>>,
    pub(super) sample_bytes: AtomicU64,
}

impl SegmentedProgressCounters {
    pub(super) fn new(segment_bytes: Vec<u64>) -> Self {
        Self {
            segment_bytes: StdMutex::new(segment_bytes.into_iter().map(AtomicU64::new).collect()),
            sample_bytes: AtomicU64::new(0),
        }
    }

    pub(super) fn store_segment_bytes(&self, segment_index: usize, bytes: u64) {
        if let Ok(mut segment_bytes) = self.segment_bytes.lock() {
            while segment_bytes.len() <= segment_index {
                segment_bytes.push(AtomicU64::new(0));
            }
            if let Some(segment_bytes) = segment_bytes.get(segment_index) {
                segment_bytes.store(bytes, Ordering::Relaxed);
            }
        }
    }

    pub(super) fn add_sample_bytes(&self, bytes: u64) {
        self.sample_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(super) fn drain_sample_bytes(&self) -> u64 {
        self.sample_bytes.swap(0, Ordering::Relaxed)
    }

    pub(super) fn total_downloaded(&self) -> u64 {
        self.segment_bytes
            .lock()
            .map(|segment_bytes| {
                segment_bytes
                    .iter()
                    .map(|bytes| bytes.load(Ordering::Relaxed))
                    .sum()
            })
            .unwrap_or(0)
    }
}

#[derive(Clone)]
pub(super) struct SegmentWorkerContext {
    pub(super) state: SharedState,
    pub(super) client: Client,
    pub(super) job_id: String,
    pub(super) url: String,
    pub(super) handoff_auth: Option<HandoffAuth>,
    pub(super) temp_path: PathBuf,
    pub(super) total_bytes: u64,
    pub(super) profile: DownloadPerformanceProfile,
    pub(super) validators: EntityValidators,
    pub(super) progress: Arc<SegmentedProgressCounters>,
    pub(super) metadata: Arc<Mutex<SegmentedDownloadState>>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) priority_throttle: Arc<Mutex<DynamicThrottleState>>,
    pub(super) stall_timeout: Option<Duration>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DownloadPerformanceProfile {
    pub(super) max_segments: usize,
    pub(super) min_segmented_size: u64,
    pub(super) target_segment_size: u64,
    pub(super) low_speed_threshold_bytes_per_second: u64,
    pub(super) low_speed_window: Duration,
    pub(super) bulk_hoster_stall_timeout: Duration,
    pub(super) max_low_speed_retries: u32,
    pub(super) speed_smoothing_alpha: f64,
}

#[derive(Debug)]
pub(super) struct RollingSpeed {
    pub(super) smoothed_bytes_per_second: Option<f64>,
    pub(super) alpha: f64,
}

impl Default for RollingSpeed {
    fn default() -> Self {
        Self {
            smoothed_bytes_per_second: None,
            alpha: 0.25,
        }
    }
}

impl RollingSpeed {
    pub(super) fn with_alpha(alpha: f64) -> Self {
        Self {
            smoothed_bytes_per_second: None,
            alpha: alpha.clamp(0.05, 1.0),
        }
    }
}

impl RollingSpeed {
    pub(super) fn record_sample(&mut self, bytes: u64, elapsed: Duration) -> u64 {
        if elapsed.is_zero() {
            return self
                .smoothed_bytes_per_second
                .map(|value| value as u64)
                .unwrap_or(0);
        }

        let sample = bytes as f64 / elapsed.as_secs_f64();
        let next = match self.smoothed_bytes_per_second {
            Some(previous) => previous.mul_add(1.0 - self.alpha, sample * self.alpha),
            None => sample,
        };
        self.smoothed_bytes_per_second = Some(next);
        next.max(0.0) as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LowSpeedDecision {
    Continue,
    Retry,
}

#[derive(Debug)]
pub(super) struct LowSpeedMonitor {
    pub(super) threshold_bytes_per_second: u64,
    pub(super) window: Duration,
    pub(super) max_retries: u32,
    pub(super) retries: u32,
}

impl LowSpeedMonitor {
    pub(super) fn new(profile: DownloadPerformanceProfile) -> Self {
        Self {
            threshold_bytes_per_second: profile.low_speed_threshold_bytes_per_second,
            window: profile.low_speed_window,
            max_retries: profile.max_low_speed_retries,
            retries: 0,
        }
    }

    pub(super) fn observe(
        &mut self,
        bytes: u64,
        elapsed: Duration,
        speed_limited: bool,
    ) -> LowSpeedDecision {
        if speed_limited
            || elapsed < self.window
            || self.threshold_bytes_per_second == 0
            || self.retries >= self.max_retries
        {
            return LowSpeedDecision::Continue;
        }

        let speed = if elapsed.is_zero() {
            0
        } else {
            (bytes as f64 / elapsed.as_secs_f64()) as u64
        };

        if speed < self.threshold_bytes_per_second {
            self.retries += 1;
            LowSpeedDecision::Retry
        } else {
            LowSpeedDecision::Continue
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct TorrentLowThroughputMonitor {
    pub(super) slow_since: Option<Instant>,
    pub(super) last_reported_at: Option<Instant>,
}

impl TorrentLowThroughputMonitor {
    pub(super) fn should_report(&mut self, update: &TorrentRuntimeSnapshot, now: Instant) -> bool {
        if !is_torrent_low_throughput_sample(update) {
            self.slow_since = None;
            return false;
        }

        let slow_since = *self.slow_since.get_or_insert(now);
        if now.duration_since(slow_since) < TORRENT_LOW_THROUGHPUT_REPORT_WINDOW {
            return false;
        }

        if self.last_reported_at.is_some_and(|reported_at| {
            now.duration_since(reported_at) < TORRENT_LOW_THROUGHPUT_REPORT_INTERVAL
        }) {
            return false;
        }

        self.last_reported_at = Some(now);
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TorrentRestoreWatchdogDecision {
    Continue,
    Recheck,
    Stalled,
}

#[derive(Debug)]
pub(super) struct TorrentRestoreWatchdog {
    pub(super) idle_since: Instant,
    pub(super) recheck_attempted: bool,
}

impl TorrentRestoreWatchdog {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            idle_since: now,
            recheck_attempted: false,
        }
    }

    pub(super) fn observe(
        &mut self,
        update: &TorrentRuntimeSnapshot,
        now: Instant,
    ) -> TorrentRestoreWatchdogDecision {
        if torrent_restore_has_validation_signal(update) {
            self.idle_since = now;
            return TorrentRestoreWatchdogDecision::Continue;
        }

        let idle_for = now.duration_since(self.idle_since);
        if !self.recheck_attempted && idle_for >= TORRENT_RESTORE_RECHECK_IDLE_WINDOW {
            self.recheck_attempted = true;
            self.idle_since = now;
            return TorrentRestoreWatchdogDecision::Recheck;
        }

        if self.recheck_attempted && idle_for >= TORRENT_RESTORE_STALLED_IDLE_WINDOW {
            return TorrentRestoreWatchdogDecision::Stalled;
        }

        TorrentRestoreWatchdogDecision::Continue
    }
}

pub(super) fn torrent_restore_has_validation_signal(update: &TorrentRuntimeSnapshot) -> bool {
    update.finished || update.total_bytes > 0 || update.downloaded_bytes > 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TorrentPeerConnectionWatchdogDecision {
    Continue,
    Report,
    RefreshPeers,
    ReaddTorrent,
}

#[derive(Debug)]
pub(super) struct TorrentPeerConnectionWatchdog {
    pub(super) mode: TorrentPeerConnectionWatchdogMode,
    pub(super) unhealthy_since: Option<Instant>,
    pub(super) last_reported_at: Option<Instant>,
    pub(super) refreshed: bool,
    pub(super) readded: bool,
}

impl TorrentPeerConnectionWatchdog {
    pub(super) fn new(mode: TorrentPeerConnectionWatchdogMode, now: Instant) -> Self {
        Self {
            mode,
            unhealthy_since: Some(now),
            last_reported_at: None,
            refreshed: false,
            readded: false,
        }
    }

    pub(super) fn observe(
        &mut self,
        update: &TorrentRuntimeSnapshot,
        now: Instant,
    ) -> TorrentPeerConnectionWatchdogDecision {
        if !is_torrent_low_throughput_sample(update) {
            self.unhealthy_since = None;
            return TorrentPeerConnectionWatchdogDecision::Continue;
        }

        let unhealthy_since = *self.unhealthy_since.get_or_insert(now);
        if now.duration_since(unhealthy_since) < TORRENT_PEER_WATCHDOG_WINDOW {
            return TorrentPeerConnectionWatchdogDecision::Continue;
        }

        if matches!(self.mode, TorrentPeerConnectionWatchdogMode::Experimental) {
            if !self.refreshed {
                self.refreshed = true;
                self.unhealthy_since = Some(now);
                return TorrentPeerConnectionWatchdogDecision::RefreshPeers;
            }
            if !self.readded {
                self.readded = true;
                self.unhealthy_since = Some(now);
                return TorrentPeerConnectionWatchdogDecision::ReaddTorrent;
            }
        }

        if self.last_reported_at.is_some_and(|reported_at| {
            now.duration_since(reported_at) < TORRENT_LOW_THROUGHPUT_REPORT_INTERVAL
        }) {
            return TorrentPeerConnectionWatchdogDecision::Continue;
        }

        self.last_reported_at = Some(now);
        TorrentPeerConnectionWatchdogDecision::Report
    }
}

pub(super) fn is_torrent_low_throughput_sample(update: &TorrentRuntimeSnapshot) -> bool {
    if update.finished || !matches!(update.phase, TorrentRuntimePhase::Live) {
        return false;
    }

    let live_peers = update
        .diagnostics
        .as_ref()
        .map(|diagnostics| diagnostics.live_peers)
        .or(update.peers)
        .unwrap_or(0);

    live_peers >= TORRENT_LOW_THROUGHPUT_LIVE_PEER_THRESHOLD
        && update.download_speed < TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND
}

pub(super) fn torrent_low_throughput_message(update: &TorrentRuntimeSnapshot) -> String {
    let Some(diagnostics) = update.diagnostics.as_ref() else {
        let live_peers = update.peers.unwrap_or(0);
        return format!(
            "Torrent throughput low: {live_peers} live peers, job down {} B/s",
            update.download_speed
        );
    };

    let listen_port = diagnostics
        .listen_port
        .map(|port| format!("listen port {port}"))
        .unwrap_or_else(|| "listen port unavailable".into());
    let listener_state = if diagnostics.listener_fallback {
        "listener fallback active"
    } else {
        "listener fallback inactive"
    };
    let classification = torrent_low_throughput_classification(update);

    format!(
        "Torrent throughput low ({classification}): {} live peers, {} seen, {} queued, {} connecting, {} contributing, {} peer error events across {} peers, {} connection attempts, {} dead, {} not needed, job down {} B/s, session down {} B/s, session up {} B/s, {listen_port}, {listener_state}",
        diagnostics.live_peers,
        diagnostics.seen_peers,
        diagnostics.queued_peers,
        diagnostics.connecting_peers,
        diagnostics.contributing_peers,
        diagnostics.peer_errors,
        diagnostics.peers_with_errors,
        diagnostics.peer_connection_attempts,
        diagnostics.dead_peers,
        diagnostics.not_needed_peers,
        update.download_speed,
        diagnostics.session_download_speed,
        diagnostics.session_upload_speed
    )
}

pub(super) fn torrent_low_throughput_classification(
    update: &TorrentRuntimeSnapshot,
) -> &'static str {
    let Some(diagnostics) = update.diagnostics.as_ref() else {
        return "peer health unknown";
    };

    if diagnostics.listen_port.is_none() || diagnostics.listener_fallback {
        return "listener unavailable or fallback active";
    }

    if diagnostics.contributing_peers == 0
        || diagnostics.contributing_peers.saturating_mul(4) < diagnostics.live_peers
    {
        return "few contributing peers";
    }

    if diagnostics.peers_with_errors.saturating_mul(2) >= diagnostics.live_peers
        || diagnostics.peer_errors >= diagnostics.live_peers
    {
        return "high peer churn";
    }

    if diagnostics.session_upload_speed == 0 && update.upload_speed == 0 {
        return "upload reciprocity risk";
    }

    "peer throughput constrained"
}

#[derive(Debug, Clone)]
pub(super) struct DownloadError {
    pub(super) category: FailureCategory,
    pub(super) message: String,
    pub(super) retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreflightMetadata {
    pub(super) total_bytes: Option<u64>,
    pub(super) resume_support: ResumeSupport,
    pub(super) filename: Option<String>,
    pub(super) validators: EntityValidators,
}

impl From<String> for DownloadError {
    fn from(message: String) -> Self {
        download_error(FailureCategory::Internal, message, false)
    }
}
