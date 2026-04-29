use crate::state::TorrentRuntimeSnapshot;
use crate::storage::TorrentSettings;
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::PersistentDhtConfig;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, PeerConnectionOptions,
    Session, SessionOptions, SessionPersistenceConfig,
};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc::UnboundedSender, Mutex};

pub const TORRENT_LISTEN_PORT_RANGE: Range<u16> = 42000..42100;
pub const TORRENT_PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
pub const TORRENT_PEER_READ_WRITE_TIMEOUT: Duration = Duration::from_secs(60);
pub const TORRENT_DHT_PERSIST_INTERVAL: Duration = Duration::from_secs(60);
pub const TORRENT_DEFER_WRITES_MB: usize = 16;
pub const TORRENT_CONCURRENT_INIT_LIMIT: usize = 2;
pub const MAX_TORRENT_UPLOAD_LIMIT_KIB_PER_SECOND: u32 = 1_048_576;
pub const TORRENT_TRACKER_FIRST_METADATA_TIMEOUT: Duration = Duration::from_secs(15);
const BYTES_PER_MEBIBYTE: f64 = 1024.0 * 1024.0;
pub const FALLBACK_TORRENT_TRACKERS: [&str; 8] = [
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://udp.tracker.projectk.org:23333/announce",
    "udp://tracker.tvunderground.org.ru:3218/announce",
    "udp://tracker.tryhackx.org:6969/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://tracker.theoks.net:6969/announce",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorrentSourceKind {
    Magnet,
    TorrentFile,
}

impl TorrentSourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Magnet => "magnet",
            Self::TorrentFile => "torrent file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTorrentSource {
    pub source: String,
    pub source_kind: TorrentSourceKind,
    pub fallback_trackers_added: usize,
    pub fallback_trackers_for_options: Vec<String>,
    pub tracker_first_metadata: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackerFirstMetadataOutcome {
    Resolved,
    TimedOut,
    Failed(String),
}

impl TrackerFirstMetadataOutcome {
    pub fn should_fallback_to_main_session(&self) -> bool {
        !matches!(self, Self::Resolved)
    }
}

pub struct TorrentEngine {
    session: Arc<Session>,
    handles: Arc<Mutex<HashMap<usize, Arc<ManagedTorrent>>>>,
    listener_fallback_message: Option<String>,
    listener_fallback_reported: AtomicBool,
}

impl TorrentEngine {
    pub async fn new(
        default_output_folder: PathBuf,
        data_dir: PathBuf,
        settings: TorrentSettings,
    ) -> Result<Self, String> {
        tokio::fs::create_dir_all(&default_output_folder)
            .await
            .map_err(|error| format!("Could not create torrent download directory: {error}"))?;
        let persistence_dir = data_dir.join("torrent-session");
        tokio::fs::create_dir_all(&persistence_dir)
            .await
            .map_err(|error| format!("Could not create torrent session directory: {error}"))?;

        let (session, listener_fallback_message) = match Session::new_with_opts(
            default_output_folder.clone(),
            torrent_session_options(persistence_dir.clone(), &settings),
        )
        .await
        {
            Ok(session) => (session, None),
            Err(error) if is_listen_error(&format!("{error:#}")) => {
                let message = torrent_listener_fallback_message(&settings, &format!("{error:#}"));
                let fallback_session = Session::new_with_opts(
                    default_output_folder,
                    torrent_session_options_with_listener(persistence_dir, None, false),
                )
                .await
                .map_err(|fallback_error| {
                    format!("Could not initialize torrent engine: {fallback_error:#}")
                })?;
                (fallback_session, Some(message))
            }
            Err(error) => {
                return Err(format!("Could not initialize torrent engine: {error:#}"));
            }
        };

        Ok(Self {
            session,
            handles: Arc::new(Mutex::new(HashMap::new())),
            listener_fallback_message,
            listener_fallback_reported: AtomicBool::new(false),
        })
    }

    pub fn take_listener_fallback_message(&self) -> Option<String> {
        let message = self.listener_fallback_message.as_ref()?;
        if self.listener_fallback_reported.swap(true, Ordering::SeqCst) {
            None
        } else {
            Some(message.clone())
        }
    }

    pub async fn add_source(
        &self,
        source: &PreparedTorrentSource,
        output_folder: &Path,
        upload_limit_kib_per_second: u32,
        tracker_first_diagnostics: Option<UnboundedSender<TrackerFirstMetadataOutcome>>,
    ) -> Result<usize, String> {
        tokio::fs::create_dir_all(output_folder)
            .await
            .map_err(|error| format!("Could not create torrent output directory: {error}"))?;
        self.set_upload_limit(upload_limit_kib_per_second);

        if source.tracker_first_metadata {
            match self
                .try_add_tracker_first_metadata(source, output_folder, upload_limit_kib_per_second)
                .await?
            {
                Ok(id) => {
                    send_tracker_first_outcome(
                        &tracker_first_diagnostics,
                        TrackerFirstMetadataOutcome::Resolved,
                    );
                    return Ok(id);
                }
                Err(outcome) => {
                    send_tracker_first_outcome(&tracker_first_diagnostics, outcome);
                }
            }
        }

        let options = torrent_add_options(
            output_folder,
            upload_limit_kib_per_second,
            &source.fallback_trackers_for_options,
        );
        let add_torrent = AddTorrent::from_cli_argument(&source.source)
            .map_err(|error| format!("Could not read torrent source: {error:#}"))?;
        self.add_to_main_session(add_torrent, options).await
    }

    async fn try_add_tracker_first_metadata(
        &self,
        source: &PreparedTorrentSource,
        output_folder: &Path,
        upload_limit_kib_per_second: u32,
    ) -> Result<Result<usize, TrackerFirstMetadataOutcome>, String> {
        let add_torrent = AddTorrent::from_cli_argument(&source.source)
            .map_err(|error| format!("Could not read torrent source: {error:#}"))?;
        let tracker_session = match Session::new_with_opts(
            output_folder.to_path_buf(),
            tracker_first_session_options(),
        )
        .await
        {
            Ok(session) => session,
            Err(error) => {
                return Ok(Err(TrackerFirstMetadataOutcome::Failed(format!(
                    "Could not initialize tracker-only metadata session: {error:#}"
                ))));
            }
        };

        let lookup = tracker_session
            .add_torrent(add_torrent, Some(tracker_first_add_options(output_folder)));
        let response =
            match tokio::time::timeout(TORRENT_TRACKER_FIRST_METADATA_TIMEOUT, lookup).await {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => {
                    return Ok(Err(TrackerFirstMetadataOutcome::Failed(format!(
                        "{error:#}"
                    ))));
                }
                Err(_) => return Ok(Err(TrackerFirstMetadataOutcome::TimedOut)),
            };

        let torrent_bytes = match response {
            AddTorrentResponse::ListOnly(list) => list.torrent_bytes,
            AddTorrentResponse::AlreadyManaged(_, _) | AddTorrentResponse::Added(_, _) => {
                return Ok(Err(TrackerFirstMetadataOutcome::Failed(
                    "Tracker-only metadata lookup did not return list-only torrent bytes".into(),
                )));
            }
        };
        let options = torrent_add_options(
            output_folder,
            upload_limit_kib_per_second,
            &source.fallback_trackers_for_options,
        );
        let id = self
            .add_to_main_session(AddTorrent::from_bytes(torrent_bytes), options)
            .await?;
        Ok(Ok(id))
    }

    async fn add_to_main_session(
        &self,
        add_torrent: AddTorrent<'_>,
        options: AddTorrentOptions,
    ) -> Result<usize, String> {
        let handle = self
            .session
            .add_torrent(add_torrent, Some(options))
            .await
            .map_err(|error| format!("Could not add torrent: {error:#}"))?
            .into_handle()
            .ok_or_else(|| "Torrent engine returned list-only response.".to_string())?;

        let id = handle.id();
        self.handles.lock().await.insert(id, handle);
        Ok(id)
    }

    pub async fn resume_existing(
        &self,
        engine_id: Option<usize>,
        info_hash: Option<&str>,
        upload_limit_kib_per_second: u32,
    ) -> Result<Option<usize>, String> {
        self.set_upload_limit(upload_limit_kib_per_second);

        if let Some(engine_id) = engine_id {
            let cached_handle = self.handles.lock().await.get(&engine_id).cloned();
            if let Some(handle) = cached_handle {
                let id = handle.id();
                self.session
                    .unpause(&handle)
                    .await
                    .map_err(|error| format!("Could not resume torrent: {error:#}"))?;
                return Ok(Some(id));
            }
        }

        for candidate in torrent_resume_candidates(engine_id, info_hash) {
            let Some(handle) = self.session.get(candidate) else {
                continue;
            };

            let id = handle.id();
            self.handles.lock().await.insert(id, handle.clone());
            self.session
                .unpause(&handle)
                .await
                .map_err(|error| format!("Could not resume torrent: {error:#}"))?;
            return Ok(Some(id));
        }

        Ok(None)
    }

    pub async fn pause(&self, id: usize) -> Result<(), String> {
        let handle = self.handle(id).await?;
        self.session
            .pause(&handle)
            .await
            .map_err(|error| format!("Could not pause torrent: {error:#}"))
    }

    pub async fn unpause(&self, id: usize) -> Result<(), String> {
        let handle = self.handle(id).await?;
        self.session
            .unpause(&handle)
            .await
            .map_err(|error| format!("Could not resume torrent: {error:#}"))
    }

    pub async fn forget(&self, id: usize) -> Result<(), String> {
        self.handles.lock().await.remove(&id);
        self.session
            .delete(TorrentIdOrHash::Id(id), false)
            .await
            .map_err(|error| format!("Could not forget torrent: {error:#}"))
    }

    pub async fn forget_existing(
        &self,
        engine_id: Option<usize>,
        info_hash: Option<&str>,
    ) -> Result<bool, String> {
        for candidate in torrent_resume_candidates(engine_id, info_hash) {
            let Some(handle) = self.session.get(candidate) else {
                continue;
            };

            let id = handle.id();
            self.handles.lock().await.remove(&id);
            self.session
                .delete(TorrentIdOrHash::Id(id), false)
                .await
                .map_err(|error| format!("Could not forget torrent: {error:#}"))?;
            return Ok(true);
        }

        Ok(false)
    }

    pub async fn forget_by_info_hash(&self, info_hash: &str) -> Result<bool, String> {
        self.forget_existing(None, Some(info_hash)).await
    }

    pub fn set_upload_limit(&self, upload_limit_kib_per_second: u32) {
        self.session
            .ratelimits
            .set_upload_bps(upload_limit_bps(upload_limit_kib_per_second));
    }

    pub async fn snapshot(&self, id: usize) -> Result<TorrentRuntimeSnapshot, String> {
        let handle = self.handle(id).await?;
        Ok(snapshot_from_handle(&handle))
    }

    async fn handle(&self, id: usize) -> Result<Arc<ManagedTorrent>, String> {
        if let Some(handle) = self.handles.lock().await.get(&id).cloned() {
            return Ok(handle);
        }

        let handle = self
            .session
            .get(TorrentIdOrHash::Id(id))
            .ok_or_else(|| format!("Torrent {id} is not managed."))?;
        self.handles.lock().await.insert(id, handle.clone());
        Ok(handle)
    }
}

pub fn prepare_torrent_source(source: &str) -> PreparedTorrentSource {
    if source
        .get(..source.len().min("magnet:".len()))
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("magnet:"))
    {
        let existing_trackers = magnet_tracker_values(source);
        let fallback_trackers =
            missing_fallback_trackers(existing_trackers.iter().map(String::as_str));
        return PreparedTorrentSource {
            source: append_trackers_to_magnet(source, &fallback_trackers),
            source_kind: TorrentSourceKind::Magnet,
            fallback_trackers_added: fallback_trackers.len(),
            fallback_trackers_for_options: Vec::new(),
            tracker_first_metadata: true,
        };
    }

    let fallback_trackers = missing_fallback_trackers(std::iter::empty::<&str>());
    PreparedTorrentSource {
        source: source.to_string(),
        source_kind: TorrentSourceKind::TorrentFile,
        fallback_trackers_added: fallback_trackers.len(),
        fallback_trackers_for_options: fallback_trackers,
        tracker_first_metadata: false,
    }
}

pub fn pending_torrent_cleanup_info_hash(source: &PreparedTorrentSource) -> Option<String> {
    if source.source_kind != TorrentSourceKind::Magnet {
        return None;
    }
    magnet_info_hash(&source.source)
}

pub(crate) fn torrent_session_options(
    persistence_dir: PathBuf,
    settings: &TorrentSettings,
) -> SessionOptions {
    torrent_session_options_with_listener(
        persistence_dir,
        Some(torrent_listen_port_range(settings)),
        settings.port_forwarding_enabled,
    )
}

fn torrent_session_options_with_listener(
    persistence_dir: PathBuf,
    listen_port_range: Option<Range<u16>>,
    enable_upnp_port_forwarding: bool,
) -> SessionOptions {
    let enable_upnp_port_forwarding = enable_upnp_port_forwarding && listen_port_range.is_some();

    SessionOptions {
        fastresume: true,
        persistence: Some(SessionPersistenceConfig::Json {
            folder: Some(persistence_dir.clone()),
        }),
        peer_opts: Some(PeerConnectionOptions {
            connect_timeout: Some(TORRENT_PEER_CONNECT_TIMEOUT),
            read_write_timeout: Some(TORRENT_PEER_READ_WRITE_TIMEOUT),
            keep_alive_interval: None,
        }),
        dht_config: Some(PersistentDhtConfig {
            dump_interval: Some(TORRENT_DHT_PERSIST_INTERVAL),
            config_filename: Some(persistence_dir.join("dht.json")),
        }),
        listen_port_range,
        enable_upnp_port_forwarding,
        defer_writes_up_to: Some(TORRENT_DEFER_WRITES_MB),
        concurrent_init_limit: Some(TORRENT_CONCURRENT_INIT_LIMIT),
        ..Default::default()
    }
}

fn tracker_first_session_options() -> SessionOptions {
    SessionOptions {
        disable_dht: true,
        disable_dht_persistence: true,
        dht_config: None,
        persistence: None,
        peer_opts: Some(PeerConnectionOptions {
            connect_timeout: Some(TORRENT_PEER_CONNECT_TIMEOUT),
            read_write_timeout: Some(TORRENT_PEER_READ_WRITE_TIMEOUT),
            keep_alive_interval: None,
        }),
        listen_port_range: None,
        enable_upnp_port_forwarding: false,
        concurrent_init_limit: Some(1),
        ..Default::default()
    }
}

fn tracker_first_add_options(output_folder: &Path) -> AddTorrentOptions {
    AddTorrentOptions {
        list_only: true,
        output_folder: Some(output_folder.display().to_string()),
        ..Default::default()
    }
}

fn torrent_listen_port_range(settings: &TorrentSettings) -> Range<u16> {
    if settings.port_forwarding_enabled {
        let port = torrent_forwarding_port(settings);
        return port..port + 1;
    }

    TORRENT_LISTEN_PORT_RANGE
}

fn torrent_forwarding_port(settings: &TorrentSettings) -> u16 {
    match u16::try_from(settings.port_forwarding_port) {
        Ok(port) if (1024..=65534).contains(&settings.port_forwarding_port) => port,
        _ => TORRENT_LISTEN_PORT_RANGE.start,
    }
}

fn torrent_listen_port_description(settings: &TorrentSettings) -> String {
    if settings.port_forwarding_enabled {
        return format!("port {}", torrent_forwarding_port(settings));
    }

    format!(
        "ports {}-{}",
        TORRENT_LISTEN_PORT_RANGE.start,
        TORRENT_LISTEN_PORT_RANGE.end - 1
    )
}

fn is_listen_error(message: &str) -> bool {
    message.contains("error listening on TCP") || message.contains("no ports in range")
}

fn torrent_listener_fallback_message(settings: &TorrentSettings, error: &str) -> String {
    let listen_ports = torrent_listen_port_description(settings);
    format!(
        "Torrent listen {listen_ports} unavailable; continuing without inbound peer listener or UPnP forwarding: {error}"
    )
}

fn send_tracker_first_outcome(
    diagnostics: &Option<UnboundedSender<TrackerFirstMetadataOutcome>>,
    outcome: TrackerFirstMetadataOutcome,
) {
    if let Some(sender) = diagnostics {
        let _ = sender.send(outcome);
    }
}

fn snapshot_from_handle(handle: &ManagedTorrent) -> TorrentRuntimeSnapshot {
    let stats = handle.stats();
    let peers = stats
        .live
        .as_ref()
        .map(|live| live.snapshot.peer_stats.live as u32);
    TorrentRuntimeSnapshot {
        engine_id: handle.id(),
        info_hash: handle.info_hash().as_string(),
        name: handle.name(),
        total_files: (!stats.file_progress.is_empty()).then_some(stats.file_progress.len() as u32),
        peers,
        seeds: None,
        downloaded_bytes: stats.progress_bytes,
        total_bytes: stats.total_bytes,
        uploaded_bytes: stats.uploaded_bytes,
        fetched_bytes: torrent_fetched_bytes(&stats),
        download_speed: torrent_download_speed_bytes_per_second(&stats),
        upload_speed: torrent_upload_speed_bytes_per_second(&stats),
        eta: torrent_eta_seconds(&stats),
        finished: stats.finished,
        error: stats.error,
    }
}

fn torrent_fetched_bytes(stats: &librqbit::TorrentStats) -> u64 {
    stats
        .live
        .as_ref()
        .map(|live| live.snapshot.fetched_bytes)
        .unwrap_or(0)
}

fn torrent_download_speed_bytes_per_second(stats: &librqbit::TorrentStats) -> u64 {
    stats
        .live
        .as_ref()
        .map(|live| mib_per_second_to_bytes_per_second(live.download_speed.mbps))
        .unwrap_or(0)
}

fn torrent_upload_speed_bytes_per_second(stats: &librqbit::TorrentStats) -> u64 {
    stats
        .live
        .as_ref()
        .map(|live| mib_per_second_to_bytes_per_second(live.upload_speed.mbps))
        .unwrap_or(0)
}

fn torrent_eta_seconds(stats: &librqbit::TorrentStats) -> Option<u64> {
    stats
        .live
        .as_ref()
        .and_then(|live| live.time_remaining.as_ref())
        .and_then(duration_with_human_readable_seconds)
}

fn duration_with_human_readable_seconds<T: serde::Serialize>(duration: &T) -> Option<u64> {
    let value = serde_json::to_value(duration).ok()?;
    duration_with_human_readable_value_seconds(&value)
}

fn duration_with_human_readable_value_seconds(value: &serde_json::Value) -> Option<u64> {
    value.get("duration")?.get("secs")?.as_u64()
}

fn mib_per_second_to_bytes_per_second(mib_per_second: f64) -> u64 {
    if !mib_per_second.is_finite() || mib_per_second <= 0.0 {
        return 0;
    }

    let bytes_per_second = mib_per_second * BYTES_PER_MEBIBYTE;
    if bytes_per_second >= u64::MAX as f64 {
        u64::MAX
    } else {
        bytes_per_second.round() as u64
    }
}

fn torrent_add_options(
    output_folder: &Path,
    upload_limit_kib_per_second: u32,
    fallback_trackers: &[String],
) -> AddTorrentOptions {
    AddTorrentOptions {
        paused: false,
        output_folder: Some(output_folder.display().to_string()),
        overwrite: true,
        ratelimits: torrent_limits(upload_limit_kib_per_second),
        trackers: (!fallback_trackers.is_empty()).then(|| fallback_trackers.to_vec()),
        ..Default::default()
    }
}

fn torrent_limits(upload_limit_kib_per_second: u32) -> LimitsConfig {
    LimitsConfig {
        upload_bps: upload_limit_bps(upload_limit_kib_per_second),
        download_bps: None,
    }
}

fn magnet_tracker_values(source: &str) -> Vec<String> {
    url::Url::parse(source)
        .ok()
        .map(|url| {
            url.query_pairs()
                .filter_map(|(key, value)| (key == "tr").then(|| value.into_owned()))
                .collect()
        })
        .unwrap_or_default()
}

fn magnet_info_hash(source: &str) -> Option<String> {
    let parsed = url::Url::parse(source).ok()?;
    parsed.query_pairs().find_map(|(key, value)| {
        if !key.eq_ignore_ascii_case("xt") {
            return None;
        }
        let value = value.into_owned();
        let (prefix, hash) = value.split_at(value.len().min("urn:btih:".len()));
        if !prefix.eq_ignore_ascii_case("urn:btih:") || hash.is_empty() {
            return None;
        }
        Some(hash.to_string())
    })
}

fn missing_fallback_trackers<'a>(existing_trackers: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = existing_trackers
        .map(|tracker| tracker.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    FALLBACK_TORRENT_TRACKERS
        .iter()
        .filter(|tracker| seen.insert(tracker.to_ascii_lowercase()))
        .map(|tracker| (*tracker).to_string())
        .collect()
}

fn append_trackers_to_magnet(source: &str, trackers: &[String]) -> String {
    let mut output = source.to_string();
    for tracker in trackers {
        if !output.contains('?') {
            output.push('?');
        } else if !output.ends_with('?') && !output.ends_with('&') {
            output.push('&');
        }
        output.push_str("tr=");
        output.push_str(&encode_query_value(tracker));
    }
    output
}

fn encode_query_value(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn upload_limit_bps(upload_limit_kib_per_second: u32) -> Option<NonZeroU32> {
    upload_limit_kib_per_second
        .min(MAX_TORRENT_UPLOAD_LIMIT_KIB_PER_SECOND)
        .checked_mul(1024)
        .and_then(NonZeroU32::new)
}

fn torrent_resume_candidates(
    engine_id: Option<usize>,
    info_hash: Option<&str>,
) -> Vec<TorrentIdOrHash> {
    let mut candidates = Vec::new();
    if let Some(engine_id) = engine_id {
        candidates.push(TorrentIdOrHash::Id(engine_id));
    }

    if let Some(info_hash) = info_hash.and_then(|hash| TorrentIdOrHash::parse(hash).ok()) {
        candidates.push(info_hash);
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_magnet_appends_fallback_trackers_encoded() {
        let prepared =
            prepare_torrent_source("magnet:?xt=urn:btih:a634dc946d49989526058626caa3bbabba4607b6");

        let parsed = url::Url::parse(&prepared.source).expect("prepared magnet should parse");
        let trackers = parsed
            .query_pairs()
            .filter_map(|(key, value)| (key == "tr").then(|| value.into_owned()))
            .collect::<Vec<_>>();

        assert_eq!(prepared.source_kind, TorrentSourceKind::Magnet);
        assert!(prepared.tracker_first_metadata);
        assert_eq!(
            prepared.fallback_trackers_added,
            FALLBACK_TORRENT_TRACKERS.len()
        );
        assert_eq!(trackers.len(), FALLBACK_TORRENT_TRACKERS.len());
        assert_eq!(trackers[0], FALLBACK_TORRENT_TRACKERS[0]);
        assert!(prepared
            .source
            .contains("tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce"));
    }

    #[test]
    fn magnet_preserves_existing_trackers_first_and_dedupes_fallbacks() {
        let prepared = prepare_torrent_source(
            "magnet:?xt=urn:btih:a634dc946d49989526058626caa3bbabba4607b6&tr=udp%3A%2F%2Ftracker.torrent.eu.org%3A451%2Fannounce&tr=udp%3A%2F%2Fcustom.example%3A1337%2Fannounce",
        );

        let parsed = url::Url::parse(&prepared.source).expect("prepared magnet should parse");
        let trackers = parsed
            .query_pairs()
            .filter_map(|(key, value)| (key == "tr").then(|| value.into_owned()))
            .collect::<Vec<_>>();

        assert_eq!(trackers[0], "udp://tracker.torrent.eu.org:451/announce");
        assert_eq!(trackers[1], "udp://custom.example:1337/announce");
        assert_eq!(
            trackers
                .iter()
                .filter(|tracker| tracker.as_str() == "udp://tracker.torrent.eu.org:451/announce")
                .count(),
            1
        );
        assert_eq!(
            prepared.fallback_trackers_added,
            FALLBACK_TORRENT_TRACKERS.len() - 1
        );
    }

    #[test]
    fn torrent_file_options_include_fallback_trackers() {
        let prepared = prepare_torrent_source("https://example.com/releases/file.torrent");
        let options = torrent_add_options(
            Path::new("C:/Downloads/file"),
            0,
            &prepared.fallback_trackers_for_options,
        );

        assert_eq!(prepared.source, "https://example.com/releases/file.torrent");
        assert_eq!(prepared.source_kind, TorrentSourceKind::TorrentFile);
        assert!(!prepared.tracker_first_metadata);
        assert_eq!(
            options
                .trackers
                .as_ref()
                .expect("fallback trackers")
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            FALLBACK_TORRENT_TRACKERS
        );
    }

    #[test]
    fn session_options_enable_listen_range_and_peer_timeouts() {
        let options =
            torrent_session_options(PathBuf::from("session"), &TorrentSettings::default());

        assert_eq!(options.listen_port_range, Some(TORRENT_LISTEN_PORT_RANGE));
        assert!(!options.enable_upnp_port_forwarding);
        let peer_options = options.peer_opts.expect("peer options");
        assert_eq!(
            peer_options.connect_timeout,
            Some(TORRENT_PEER_CONNECT_TIMEOUT)
        );
        assert_eq!(
            peer_options.read_write_timeout,
            Some(TORRENT_PEER_READ_WRITE_TIMEOUT)
        );
    }

    #[test]
    fn session_options_use_app_local_dht_persistence_and_deferred_writes() {
        let persistence_dir = PathBuf::from("session");
        let options = torrent_session_options(persistence_dir.clone(), &TorrentSettings::default());
        let dht_config = options.dht_config.expect("app-local DHT config");

        assert_eq!(
            dht_config.config_filename,
            Some(persistence_dir.join("dht.json"))
        );
        assert_eq!(dht_config.dump_interval, Some(Duration::from_secs(60)));
        assert_eq!(options.defer_writes_up_to, Some(16));
        assert_eq!(options.concurrent_init_limit, Some(2));
    }

    #[test]
    fn session_options_use_exact_forwarded_port_when_opted_in() {
        let settings = TorrentSettings {
            port_forwarding_enabled: true,
            port_forwarding_port: 43000,
            ..TorrentSettings::default()
        };
        let options = torrent_session_options(PathBuf::from("session"), &settings);

        assert_eq!(options.listen_port_range, Some(43000..43001));
        assert!(options.enable_upnp_port_forwarding);
    }

    #[test]
    fn listener_fallback_message_mentions_inbound_listener_and_upnp() {
        let settings = TorrentSettings {
            port_forwarding_enabled: true,
            port_forwarding_port: 43000,
            ..TorrentSettings::default()
        };

        let message = torrent_listener_fallback_message(&settings, "listen failed");

        assert!(message.contains("port 43000"));
        assert!(message.contains("without inbound peer listener or UPnP forwarding"));
        assert!(message.contains("listen failed"));
    }

    #[test]
    fn tracker_first_session_options_disable_dht_persistence_and_listener() {
        let options = tracker_first_session_options();

        assert!(options.disable_dht);
        assert!(options.disable_dht_persistence);
        assert!(options.dht_config.is_none());
        assert!(options.persistence.is_none());
        assert!(options.listen_port_range.is_none());
        assert!(!options.enable_upnp_port_forwarding);
    }

    #[test]
    fn tracker_first_add_options_list_metadata_without_starting_managed_download() {
        let options = tracker_first_add_options(Path::new("C:/Downloads/tracker-first"));

        assert!(options.list_only);
        assert!(!options.paused);
        assert_eq!(
            options.output_folder.as_deref(),
            Some("C:/Downloads/tracker-first")
        );
    }

    #[test]
    fn tracker_first_fallback_outcomes_continue_with_main_dht_session() {
        assert!(TrackerFirstMetadataOutcome::TimedOut.should_fallback_to_main_session());
        assert!(TrackerFirstMetadataOutcome::Failed("tracker error".into())
            .should_fallback_to_main_session());
        assert!(!TrackerFirstMetadataOutcome::Resolved.should_fallback_to_main_session());
    }

    #[test]
    fn tracker_first_timeout_is_shorter_than_outer_metadata_timeout() {
        assert_eq!(
            TORRENT_TRACKER_FIRST_METADATA_TIMEOUT,
            Duration::from_secs(15)
        );
    }

    #[test]
    fn upload_limit_bps_clamps_large_values_instead_of_disabling_limit() {
        assert_eq!(
            upload_limit_bps(10_000_000),
            NonZeroU32::new(1_048_576 * 1024)
        );
    }

    #[test]
    fn runtime_download_speed_uses_live_estimator_not_progress_bytes() {
        let mut live = librqbit::api::LiveStats::default();
        live.download_speed = 1.5.into();
        let stats = librqbit::TorrentStats {
            state: librqbit::TorrentStatsState::Live,
            file_progress: vec![10 * 1024 * 1024 * 1024],
            error: None,
            progress_bytes: 10 * 1024 * 1024 * 1024,
            uploaded_bytes: 0,
            total_bytes: 20 * 1024 * 1024 * 1024,
            finished: false,
            live: Some(live),
        };

        assert_eq!(torrent_download_speed_bytes_per_second(&stats), 1_572_864);
    }

    #[test]
    fn runtime_download_speed_is_zero_without_live_estimator() {
        let stats = librqbit::TorrentStats {
            state: librqbit::TorrentStatsState::Paused,
            file_progress: vec![10 * 1024 * 1024 * 1024],
            error: None,
            progress_bytes: 10 * 1024 * 1024 * 1024,
            uploaded_bytes: 0,
            total_bytes: 20 * 1024 * 1024 * 1024,
            finished: false,
            live: None,
        };

        assert_eq!(torrent_download_speed_bytes_per_second(&stats), 0);
    }

    #[test]
    fn runtime_upload_speed_uses_live_estimator() {
        let mut live = librqbit::api::LiveStats::default();
        live.upload_speed = 0.75.into();
        let stats = librqbit::TorrentStats {
            state: librqbit::TorrentStatsState::Live,
            file_progress: vec![],
            error: None,
            progress_bytes: 1024,
            uploaded_bytes: 0,
            total_bytes: 1024,
            finished: true,
            live: Some(live),
        };

        assert_eq!(torrent_upload_speed_bytes_per_second(&stats), 786_432);
    }

    #[test]
    fn runtime_eta_uses_serialized_librqbit_time_remaining_shape() {
        let value = serde_json::json!({
            "duration": { "secs": 125, "nanos": 500_000_000u32 },
            "human_readable": "2m 5s"
        });

        assert_eq!(
            duration_with_human_readable_value_seconds(&value),
            Some(125)
        );
    }

    #[test]
    fn runtime_eta_is_none_without_serialized_duration_seconds() {
        let value = serde_json::json!({ "human_readable": "unknown" });

        assert_eq!(duration_with_human_readable_value_seconds(&value), None);
    }

    #[test]
    fn runtime_fetched_bytes_uses_live_snapshot_not_progress_bytes() {
        let mut live = librqbit::api::LiveStats::default();
        live.snapshot.fetched_bytes = 512 * 1024;
        let stats = librqbit::TorrentStats {
            state: librqbit::TorrentStatsState::Live,
            file_progress: vec![10 * 1024 * 1024 * 1024],
            error: None,
            progress_bytes: 10 * 1024 * 1024 * 1024,
            uploaded_bytes: 0,
            total_bytes: 20 * 1024 * 1024 * 1024,
            finished: false,
            live: Some(live),
        };

        assert_eq!(torrent_fetched_bytes(&stats), 512 * 1024);
    }

    #[test]
    fn resume_candidates_prefer_engine_id_then_info_hash() {
        let candidates =
            torrent_resume_candidates(Some(7), Some("a634dc946d49989526058626caa3bbabba4607b6"));

        assert!(matches!(candidates[0], TorrentIdOrHash::Id(7)));
        assert!(matches!(candidates[1], TorrentIdOrHash::Hash(_)));
    }

    #[test]
    fn pending_cleanup_hash_is_derived_from_magnet_only() {
        let prepared =
            prepare_torrent_source("magnet:?xt=urn:btih:a634dc946d49989526058626caa3bbabba4607b6");
        assert_eq!(
            pending_torrent_cleanup_info_hash(&prepared).as_deref(),
            Some("a634dc946d49989526058626caa3bbabba4607b6")
        );

        let torrent_file = prepare_torrent_source("https://example.com/releases/file.torrent");
        assert_eq!(pending_torrent_cleanup_info_hash(&torrent_file), None);
    }
}
