use crate::state::{TorrentRuntimePhase, TorrentRuntimeSnapshot};
use crate::storage::TorrentPeerConnectionWatchdogMode;
use std::time::{Duration, Instant};

use super::{
    TORRENT_LOW_THROUGHPUT_REPORT_INTERVAL, TORRENT_LOW_THROUGHPUT_REPORT_WINDOW,
    TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND, TORRENT_PEER_WATCHDOG_WINDOW,
    TORRENT_RESTORE_RECHECK_IDLE_WINDOW, TORRENT_RESTORE_STALLED_IDLE_WINDOW,
};

#[derive(Debug, Default)]
pub(super) struct TorrentThroughputWindow {
    pub(super) started_at: Option<Instant>,
    pub(super) fetched_bytes: Option<u64>,
    pub(super) downloaded_bytes: Option<u64>,
}

impl TorrentThroughputWindow {
    pub(super) fn started(now: Instant) -> Self {
        Self {
            started_at: Some(now),
            fetched_bytes: None,
            downloaded_bytes: None,
        }
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn restart(&mut self, update: &TorrentRuntimeSnapshot, now: Instant) {
        self.started_at = Some(now);
        self.fetched_bytes = Some(update.fetched_bytes);
        self.downloaded_bytes = Some(update.downloaded_bytes);
    }

    pub(super) fn has_sustained_stall(
        &mut self,
        update: &TorrentRuntimeSnapshot,
        now: Instant,
        window: Duration,
    ) -> bool {
        if !is_torrent_low_throughput_sample(update) {
            self.reset();
            return false;
        }

        let Some(started_at) = self.started_at else {
            self.restart(update, now);
            return false;
        };
        let started_fetched_bytes = *self.fetched_bytes.get_or_insert(update.fetched_bytes);
        let started_downloaded_bytes =
            *self.downloaded_bytes.get_or_insert(update.downloaded_bytes);

        let elapsed = now.duration_since(started_at);
        if elapsed < window {
            return false;
        }

        let fetched_progress = update.fetched_bytes.saturating_sub(started_fetched_bytes);
        let downloaded_progress = update
            .downloaded_bytes
            .saturating_sub(started_downloaded_bytes);
        let progress_bytes = fetched_progress.max(downloaded_progress);
        let average_progress_bytes_per_second = if elapsed.is_zero() {
            0
        } else {
            (progress_bytes as f64 / elapsed.as_secs_f64()) as u64
        };

        if average_progress_bytes_per_second
            >= TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND
        {
            self.restart(update, now);
            return false;
        }

        true
    }
}

#[derive(Debug, Default)]
pub(super) struct TorrentLowThroughputMonitor {
    pub(super) stall_window: TorrentThroughputWindow,
    pub(super) last_reported_at: Option<Instant>,
}

impl TorrentLowThroughputMonitor {
    pub(super) fn should_report(&mut self, update: &TorrentRuntimeSnapshot, now: Instant) -> bool {
        if !self
            .stall_window
            .has_sustained_stall(update, now, TORRENT_LOW_THROUGHPUT_REPORT_WINDOW)
        {
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
    ResetEngine,
}

#[derive(Debug)]
pub(super) struct TorrentPeerConnectionWatchdog {
    pub(super) mode: TorrentPeerConnectionWatchdogMode,
    pub(super) stall_window: TorrentThroughputWindow,
    pub(super) last_reported_at: Option<Instant>,
    pub(super) refreshed: bool,
    pub(super) readded: bool,
    pub(super) reset_engine: bool,
}

impl TorrentPeerConnectionWatchdog {
    pub(super) fn new(mode: TorrentPeerConnectionWatchdogMode, now: Instant) -> Self {
        Self {
            mode,
            stall_window: TorrentThroughputWindow::started(now),
            last_reported_at: None,
            refreshed: false,
            readded: false,
            reset_engine: false,
        }
    }

    pub(super) fn observe(
        &mut self,
        update: &TorrentRuntimeSnapshot,
        now: Instant,
    ) -> TorrentPeerConnectionWatchdogDecision {
        if !self
            .stall_window
            .has_sustained_stall(update, now, TORRENT_PEER_WATCHDOG_WINDOW)
        {
            return TorrentPeerConnectionWatchdogDecision::Continue;
        }

        if matches!(self.mode, TorrentPeerConnectionWatchdogMode::Recover) {
            if !self.refreshed {
                self.refreshed = true;
                self.stall_window.restart(update, now);
                return TorrentPeerConnectionWatchdogDecision::RefreshPeers;
            }
            if !self.readded {
                self.readded = true;
                self.stall_window.restart(update, now);
                return TorrentPeerConnectionWatchdogDecision::ReaddTorrent;
            }
            if !self.reset_engine {
                self.reset_engine = true;
                self.stall_window.restart(update, now);
                return TorrentPeerConnectionWatchdogDecision::ResetEngine;
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

    pub(super) fn rearm_engine_reset(&mut self) {
        self.reset_engine = false;
    }
}

pub(super) fn is_torrent_low_throughput_sample(update: &TorrentRuntimeSnapshot) -> bool {
    if update.finished || !matches!(update.phase, TorrentRuntimePhase::Live) {
        return false;
    }

    if update.download_speed >= TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND {
        return false;
    }

    torrent_has_low_throughput_peer_signal(update)
}

fn torrent_has_low_throughput_peer_signal(update: &TorrentRuntimeSnapshot) -> bool {
    let live_peers = update
        .diagnostics
        .as_ref()
        .map(|diagnostics| diagnostics.live_peers)
        .or(update.peers)
        .unwrap_or(0);
    let seeds = update.seeds.unwrap_or(0);
    if live_peers > 0 || seeds > 0 {
        return true;
    }

    update.diagnostics.as_ref().is_some_and(|diagnostics| {
        diagnostics.queued_peers > 0
            || diagnostics.connecting_peers > 0
            || diagnostics.seen_peers > 0
            || diagnostics.dead_peers > 0
            || diagnostics.contributing_peers > 0
            || diagnostics.peer_errors > 0
            || diagnostics.peers_with_errors > 0
            || diagnostics.peer_connection_attempts > 0
            || diagnostics.listen_port.is_none()
            || diagnostics.listener_fallback
    })
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
