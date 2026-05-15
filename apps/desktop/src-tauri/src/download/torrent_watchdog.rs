use crate::state::{TorrentRuntimePhase, TorrentRuntimeSnapshot};
use crate::storage::TorrentPeerConnectionWatchdogMode;
use std::time::{Duration, Instant};

use super::{
    TORRENT_LOW_THROUGHPUT_REPORT_INTERVAL, TORRENT_LOW_THROUGHPUT_REPORT_WINDOW,
    TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND, TORRENT_PEER_LOW_RAMP_ASSIST_WINDOW,
    TORRENT_PEER_STARTUP_ACTIVE_CONNECTION_LIMIT, TORRENT_PEER_STARTUP_ASSIST_WINDOW,
    TORRENT_PEER_WATCHDOG_WINDOW, TORRENT_RESTORE_RECHECK_IDLE_WINDOW,
    TORRENT_RESTORE_STALLED_IDLE_WINDOW,
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
}

#[derive(Debug)]
pub(super) struct TorrentPeerConnectionWatchdog {
    pub(super) mode: TorrentPeerConnectionWatchdogMode,
    pub(super) started_at: Instant,
    pub(super) stall_window: TorrentThroughputWindow,
    pub(super) last_reported_at: Option<Instant>,
    pub(super) startup_refreshed: bool,
    pub(super) ramp_refreshed: bool,
    pub(super) recover_refreshed: bool,
    pub(super) last_recovery_action: Option<&'static str>,
}

impl TorrentPeerConnectionWatchdog {
    pub(super) fn new(mode: TorrentPeerConnectionWatchdogMode, now: Instant) -> Self {
        Self {
            mode,
            started_at: now,
            stall_window: TorrentThroughputWindow::started(now),
            last_reported_at: None,
            startup_refreshed: false,
            ramp_refreshed: false,
            recover_refreshed: false,
            last_recovery_action: None,
        }
    }

    pub(super) fn last_recovery_action_label(&self) -> &'static str {
        self.last_recovery_action.unwrap_or("none")
    }

    pub(super) fn observe(
        &mut self,
        update: &TorrentRuntimeSnapshot,
        now: Instant,
    ) -> TorrentPeerConnectionWatchdogDecision {
        if let Some(decision) = self.observe_fast_start(update, now) {
            return decision;
        }

        if !self
            .stall_window
            .has_sustained_stall(update, now, TORRENT_PEER_WATCHDOG_WINDOW)
        {
            return TorrentPeerConnectionWatchdogDecision::Continue;
        }

        match self.mode {
            TorrentPeerConnectionWatchdogMode::Assist => {
                if !self.startup_refreshed && torrent_is_before_first_payload(update) {
                    self.startup_refreshed = true;
                    self.last_recovery_action = Some("startup_refresh_peers");
                    self.stall_window.restart(update, now);
                    return TorrentPeerConnectionWatchdogDecision::RefreshPeers;
                }
                if !self.ramp_refreshed && torrent_needs_low_ramp_assist(update) {
                    self.ramp_refreshed = true;
                    self.last_recovery_action = Some("low_ramp_refresh_peers");
                    self.stall_window.restart(update, now);
                    return TorrentPeerConnectionWatchdogDecision::RefreshPeers;
                }
            }
            TorrentPeerConnectionWatchdogMode::Recover => {
                if !self.recover_refreshed {
                    self.recover_refreshed = true;
                    self.last_recovery_action = Some("refresh_peers");
                    self.stall_window.restart(update, now);
                    return TorrentPeerConnectionWatchdogDecision::RefreshPeers;
                }
            }
            TorrentPeerConnectionWatchdogMode::Diagnose => {}
        }

        if self.last_reported_at.is_some_and(|reported_at| {
            now.duration_since(reported_at) < TORRENT_LOW_THROUGHPUT_REPORT_INTERVAL
        }) {
            return TorrentPeerConnectionWatchdogDecision::Continue;
        }

        self.last_reported_at = Some(now);
        TorrentPeerConnectionWatchdogDecision::Report
    }

    fn observe_fast_start(
        &mut self,
        update: &TorrentRuntimeSnapshot,
        now: Instant,
    ) -> Option<TorrentPeerConnectionWatchdogDecision> {
        if !matches!(update.phase, TorrentRuntimePhase::Live) {
            return None;
        }
        let elapsed = now.duration_since(self.started_at);
        match self.mode {
            TorrentPeerConnectionWatchdogMode::Assist => {
                if !self.startup_refreshed
                    && elapsed >= TORRENT_PEER_STARTUP_ASSIST_WINDOW
                    && torrent_needs_startup_peer_assist(update)
                {
                    self.startup_refreshed = true;
                    self.last_recovery_action = Some("startup_refresh_peers");
                    self.stall_window.restart(update, now);
                    return Some(TorrentPeerConnectionWatchdogDecision::RefreshPeers);
                }
                if !self.ramp_refreshed
                    && elapsed >= TORRENT_PEER_LOW_RAMP_ASSIST_WINDOW
                    && torrent_needs_low_ramp_assist(update)
                {
                    self.ramp_refreshed = true;
                    self.last_recovery_action = Some("low_ramp_refresh_peers");
                    self.stall_window.restart(update, now);
                    return Some(TorrentPeerConnectionWatchdogDecision::RefreshPeers);
                }
            }
            TorrentPeerConnectionWatchdogMode::Diagnose => {
                if !self.startup_refreshed
                    && elapsed >= TORRENT_PEER_STARTUP_ASSIST_WINDOW
                    && torrent_needs_startup_peer_assist(update)
                {
                    self.startup_refreshed = true;
                    return Some(TorrentPeerConnectionWatchdogDecision::Report);
                }
                if !self.ramp_refreshed
                    && elapsed >= TORRENT_PEER_LOW_RAMP_ASSIST_WINDOW
                    && torrent_needs_low_ramp_assist(update)
                {
                    self.ramp_refreshed = true;
                    return Some(TorrentPeerConnectionWatchdogDecision::Report);
                }
            }
            TorrentPeerConnectionWatchdogMode::Recover => {}
        }

        None
    }
}

fn torrent_is_before_first_payload(update: &TorrentRuntimeSnapshot) -> bool {
    update.fetched_bytes == 0 && update.downloaded_bytes == 0
}

fn torrent_needs_low_ramp_assist(update: &TorrentRuntimeSnapshot) -> bool {
    if torrent_is_before_first_payload(update) {
        return false;
    }

    update.diagnostics.as_ref().is_some_and(|diagnostics| {
        diagnostics.contributing_peers <= 1
            && diagnostics.live_peers <= 2
            && update.download_speed < TORRENT_LOW_THROUGHPUT_SPEED_THRESHOLD_BYTES_PER_SECOND
    })
}

fn torrent_needs_startup_peer_assist(update: &TorrentRuntimeSnapshot) -> bool {
    if !torrent_is_before_first_payload(update) {
        return false;
    }

    update.diagnostics.as_ref().is_some_and(|diagnostics| {
        diagnostics.live_peers == 0
            && diagnostics.contributing_peers == 0
            && diagnostics.connecting_peers <= TORRENT_PEER_STARTUP_ACTIVE_CONNECTION_LIMIT
    })
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
    let dht_nodes = diagnostics
        .dht_nodes
        .map(|nodes| format!(", DHT nodes {nodes}"))
        .unwrap_or_default();
    let listener_state = if diagnostics.listener_fallback {
        "listener fallback active"
    } else {
        "listener fallback inactive"
    };
    let classification = torrent_low_throughput_classification(update);

    format!(
        "Torrent throughput low ({classification}): {} live peers, {} seen, {} queued, {} connecting, {} contributing, {} peer error events across {} peers, {} connection attempts, {} dead, {} not needed, job down {} B/s, session down {} B/s, session up {} B/s, {listen_port}{dht_nodes}, {listener_state}",
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
