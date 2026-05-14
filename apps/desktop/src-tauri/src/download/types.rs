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
    pub(super) segment_pressure_key: String,
    pub(super) handoff_auth: Option<HandoffAuth>,
    pub(super) temp_path: PathBuf,
    pub(super) total_bytes: u64,
    pub(super) profile: DownloadPerformanceProfile,
    pub(super) validators: EntityValidators,
    pub(super) progress: Arc<SegmentedProgressCounters>,
    pub(super) metadata: Arc<Mutex<SegmentedDownloadState>>,
    pub(super) metadata_persisted_at: Arc<Mutex<Instant>>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) control_signal: WorkerControlSignal,
    pub(super) ramp_blocked: Arc<AtomicBool>,
    pub(super) priority_throttle: Arc<Mutex<DynamicThrottleState>>,
    pub(super) priority_throttle_enabled: bool,
    pub(super) stall_timeout: Option<Duration>,
    pub(super) reconnects: Arc<SegmentReconnectTracker>,
    pub(super) target_workers: Arc<AtomicUsize>,
    pub(super) active_workers: Arc<AtomicUsize>,
}

#[derive(Debug)]
pub(super) struct SegmentReconnectTracker {
    attempts: Mutex<HashMap<usize, u32>>,
    reconnecting: AtomicUsize,
    decode_body_reconnects: AtomicUsize,
    decode_body_max_attempt: AtomicUsize,
}

impl Default for SegmentReconnectTracker {
    fn default() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
            reconnecting: AtomicUsize::new(0),
            decode_body_reconnects: AtomicUsize::new(0),
            decode_body_max_attempt: AtomicUsize::new(0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SegmentDecodeBodyReconnectSummary {
    pub(super) reconnects: usize,
    pub(super) max_attempt: u32,
}

impl SegmentReconnectTracker {
    pub(super) async fn record_attempt(&self, segment_index: usize) -> u32 {
        let mut attempts = self.attempts.lock().await;
        let entry = attempts.entry(segment_index).or_insert(0);
        *entry = entry.saturating_add(1);
        *entry
    }

    pub(super) async fn clear_segment(&self, segment_index: usize) {
        self.attempts.lock().await.remove(&segment_index);
    }

    pub(super) fn record_decode_body_reconnect(&self, attempt: u32) {
        self.decode_body_reconnects.fetch_add(1, Ordering::Relaxed);
        let attempt = attempt as usize;
        self.decode_body_max_attempt
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.max(attempt))
            })
            .ok();
    }

    pub(super) fn decode_body_reconnect_summary(
        &self,
    ) -> Option<SegmentDecodeBodyReconnectSummary> {
        let reconnects = self.decode_body_reconnects.load(Ordering::Relaxed);
        if reconnects == 0 {
            return None;
        }

        Some(SegmentDecodeBodyReconnectSummary {
            reconnects,
            max_attempt: self
                .decode_body_max_attempt
                .load(Ordering::Relaxed)
                .min(u32::MAX as usize) as u32,
        })
    }

    pub(super) fn begin_reconnect(&self) {
        self.reconnecting.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn end_reconnect(&self) {
        self.reconnecting
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_sub(1))
            })
            .ok();
    }

    pub(super) fn reconnecting_count(&self) -> usize {
        self.reconnecting.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DownloadPerformanceProfile {
    pub(super) initial_segments: usize,
    pub(super) soft_max_segments: usize,
    pub(super) max_segments: usize,
    pub(super) min_segmented_size: u64,
    pub(super) target_segment_size: u64,
    pub(super) low_speed_threshold_bytes_per_second: u64,
    pub(super) low_speed_window: Duration,
    pub(super) bulk_hoster_stall_timeout: Duration,
    pub(super) max_low_speed_retries: u32,
    pub(super) speed_smoothing_alpha: f64,
    pub(super) adaptive_ramp_step: usize,
    pub(super) adaptive_ramp_interval: Duration,
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

#[derive(Debug, Clone)]
pub(super) struct DownloadError {
    pub(super) category: FailureCategory,
    pub(super) message: String,
    pub(super) retryable: bool,
    pub(super) http_status: Option<StatusCode>,
    pub(super) retry_after: Option<Duration>,
    pub(super) resume_metadata_issue: Option<SegmentResumeMetadataIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SegmentResumeMetadataIssue {
    Missing,
    Corrupt,
    PlanIncompatible,
    ValidatorConflict,
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
