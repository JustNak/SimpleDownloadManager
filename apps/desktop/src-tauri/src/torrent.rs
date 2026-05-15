use crate::state::{TorrentRuntimePhase, TorrentRuntimeSnapshot};
use crate::storage::{TorrentPeerDiagnostics, TorrentRuntimeDiagnostics, TorrentSettings};
use librqbit::api::{Api, TorrentIdOrHash};
use librqbit::dht::PersistentDhtConfig;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, PeerConnectionOptions,
    Session, SessionOptions, SessionPersistenceConfig,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc::UnboundedSender, Mutex};

pub const TORRENT_LISTEN_PORT_RANGE: Range<u16> = 42000..42100;
pub const TORRENT_PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(7);
pub const TORRENT_PEER_READ_WRITE_TIMEOUT: Duration = Duration::from_secs(60);
pub const TORRENT_DHT_PERSIST_INTERVAL: Duration = Duration::from_secs(60);
pub const TORRENT_DEFER_WRITES_MB: usize = 16;
pub const TORRENT_CONCURRENT_INIT_LIMIT: usize = 2;
pub const MAX_TORRENT_UPLOAD_LIMIT_KIB_PER_SECOND: u32 = 1_048_576;
pub const TORRENT_TRACKER_FIRST_METADATA_TIMEOUT: Duration = Duration::from_secs(8);
pub const TORRENT_METADATA_CACHE_DIR: &str = "torrent-metadata";
pub const TORRENT_PEER_CACHE_FILE: &str = "torrent-peer-cache.json";
pub const TORRENT_PEER_CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub const MAX_TORRENT_PEER_CACHE_PEERS: usize = 64;
pub const MAX_CUSTOM_TORRENT_TRACKERS: usize = 64;
const BYTES_PER_MEBIBYTE: f64 = 1024.0 * 1024.0;
pub const FALLBACK_TORRENT_TRACKERS: [&str; 24] = [
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://wepzone.net:6969/announce",
    "udp://vito-tracker.space:6969/announce",
    "udp://vito-tracker.duckdns.org:6969/announce",
    "udp://udp.tracker.projectk.org:23333/announce",
    "udp://tracker.tryhackx.org:6969/announce",
    "udp://tracker.t-1.org:6969/announce",
    "udp://tracker.srv00.com:6969/announce",
    "udp://tracker.qu.ax:6969/announce",
    "udp://tracker.plx.im:6969/announce",
    "udp://tracker.opentorrent.top:6969/announce",
    "udp://tracker.gmi.gd:6969/announce",
    "udp://tracker.ducks.party:1984/announce",
    "udp://tracker.bluefrog.pw:2710/announce",
    "udp://tracker.bittor.pw:1337/announce",
    "udp://tracker.1h.is:1337/announce",
    "udp://tracker.004430.xyz:1337/announce",
    "udp://tr4ck3r.duckdns.org:6969/announce",
    "udp://torrents.tmtime.dev:6969/announce",
    "https://tracker.zhuqiy.com:443/announce",
    "https://tracker.bt4g.com:443/announce",
    "https://torrents.tmtime.dev:443/announce",
    "https://open.ftorrent.com:443/announce",
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
    pub info_hash_hint: Option<String>,
    pub original_tracker_count: usize,
    pub custom_trackers_added: usize,
    pub fallback_trackers_added: usize,
    pub fallback_trackers_for_options: Vec<String>,
    pub tracker_protocol_counts: TorrentTrackerProtocolCounts,
    pub tracker_first_metadata: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TorrentTrackerProtocolCounts {
    pub http: usize,
    pub https: usize,
    pub udp: usize,
}

impl PreparedTorrentSource {
    pub fn tracker_source_summary(&self) -> String {
        format!(
            "original {}, custom {}, bundled {}, protocols http={}, https={}, udp={}",
            self.original_tracker_count,
            self.custom_trackers_added,
            self.fallback_trackers_added,
            self.tracker_protocol_counts.http,
            self.tracker_protocol_counts.https,
            self.tracker_protocol_counts.udp
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackerFirstMetadataOutcome {
    Resolved { initial_peers: usize },
    SupersededByMainSession,
    TimedOut,
    Failed(String),
}

impl TrackerFirstMetadataOutcome {
    pub fn should_fallback_to_main_session(&self) -> bool {
        matches!(self, Self::TimedOut | Self::Failed(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TorrentAddSessionOutcome {
    pub engine_id: usize,
    pub reused_existing_session: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrackerFirstMetadataResult {
    add_session: TorrentAddSessionOutcome,
    initial_peers: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TorrentPeerCacheFile {
    #[serde(default)]
    entries: HashMap<String, TorrentPeerCacheEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TorrentPeerCacheEntry {
    #[serde(default)]
    peers: Vec<TorrentPeerCachePeer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TorrentPeerCachePeer {
    address: String,
    last_seen_unix_seconds: u64,
    #[serde(default)]
    failures: u32,
    #[serde(default)]
    contributing: bool,
    #[serde(default)]
    live: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TorrentPeerCacheObservation {
    address: SocketAddr,
    contributing: bool,
    live: bool,
    failures: u32,
}

pub struct TorrentEngine {
    session: Arc<Session>,
    api: Api,
    handles: Arc<Mutex<HashMap<usize, Arc<ManagedTorrent>>>>,
    data_dir: PathBuf,
    created_at: Instant,
    peer_cache_hits: Arc<Mutex<HashMap<usize, u32>>>,
    last_peer_discovery_assist_actions: Arc<Mutex<HashMap<usize, String>>>,
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
            api: Api::new(session.clone(), None),
            session,
            handles: Arc::new(Mutex::new(HashMap::new())),
            data_dir,
            created_at: Instant::now(),
            peer_cache_hits: Arc::new(Mutex::new(HashMap::new())),
            last_peer_discovery_assist_actions: Arc::new(Mutex::new(HashMap::new())),
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
        start_paused: bool,
        tracker_first_diagnostics: Option<UnboundedSender<TrackerFirstMetadataOutcome>>,
    ) -> Result<TorrentAddSessionOutcome, String> {
        tokio::fs::create_dir_all(output_folder)
            .await
            .map_err(|error| format!("Could not create torrent output directory: {error}"))?;
        self.set_upload_limit(upload_limit_kib_per_second);
        let cached_initial_peers =
            read_cached_torrent_peers(&self.data_dir, source.info_hash_hint.as_deref())
                .await
                .unwrap_or_default();
        let cached_peer_count = usize_to_u32(cached_initial_peers.len());

        let options = torrent_add_options(
            output_folder,
            upload_limit_kib_per_second,
            start_paused,
            &source.fallback_trackers_for_options,
            optional_initial_peers(cached_initial_peers.clone()),
        );
        let add_torrent = AddTorrent::from_cli_argument(&source.source)
            .map_err(|error| format!("Could not read torrent source: {error:#}"))?;

        let outcome = if source.tracker_first_metadata {
            race_tracker_first_metadata_resolution(
                self.add_to_main_session(add_torrent, options),
                self.try_add_tracker_first_metadata(
                    source,
                    output_folder,
                    upload_limit_kib_per_second,
                    start_paused,
                    cached_initial_peers,
                ),
                tracker_first_diagnostics,
            )
            .await?
        } else {
            self.add_to_main_session(add_torrent, options).await?
        };

        self.peer_cache_hits
            .lock()
            .await
            .insert(outcome.engine_id, cached_peer_count);
        Ok(outcome)
    }

    async fn try_add_tracker_first_metadata(
        &self,
        source: &PreparedTorrentSource,
        output_folder: &Path,
        upload_limit_kib_per_second: u32,
        start_paused: bool,
        cached_initial_peers: Vec<SocketAddr>,
    ) -> Result<Result<TrackerFirstMetadataResult, TrackerFirstMetadataOutcome>, String> {
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

        let (torrent_bytes, seen_peers) = match response {
            AddTorrentResponse::ListOnly(list) => (list.torrent_bytes, list.seen_peers),
            AddTorrentResponse::AlreadyManaged(_, _) | AddTorrentResponse::Added(_, _) => {
                return Ok(Err(TrackerFirstMetadataOutcome::Failed(
                    "Tracker-only metadata lookup did not return list-only torrent bytes".into(),
                )));
            }
        };
        let initial_peer_count = seen_peers.len();
        let initial_peers = merge_initial_torrent_peers(cached_initial_peers, seen_peers);
        let options = torrent_add_options(
            output_folder,
            upload_limit_kib_per_second,
            start_paused,
            &source.fallback_trackers_for_options,
            optional_initial_peers(initial_peers),
        );
        let id = self
            .add_to_main_session(AddTorrent::from_bytes(torrent_bytes), options)
            .await?;
        Ok(Ok(TrackerFirstMetadataResult {
            add_session: id,
            initial_peers: initial_peer_count,
        }))
    }

    async fn add_to_main_session(
        &self,
        add_torrent: AddTorrent<'_>,
        options: AddTorrentOptions,
    ) -> Result<TorrentAddSessionOutcome, String> {
        let response = self
            .session
            .add_torrent(add_torrent, Some(options))
            .await
            .map_err(|error| format!("Could not add torrent: {error:#}"))?;
        let (handle, reused_existing_session) = match response {
            AddTorrentResponse::AlreadyManaged(_, handle) => (handle, true),
            AddTorrentResponse::Added(_, handle) => (handle, false),
            AddTorrentResponse::ListOnly(_) => {
                return Err("Torrent engine returned list-only response.".to_string());
            }
        };

        let id = handle.id();
        self.handles.lock().await.insert(id, handle);
        Ok(TorrentAddSessionOutcome {
            engine_id: id,
            reused_existing_session,
        })
    }

    pub async fn resume_existing(
        &self,
        engine_id: Option<usize>,
        info_hash: Option<&str>,
        upload_limit_kib_per_second: u32,
        validate_only: bool,
    ) -> Result<Option<usize>, String> {
        self.set_upload_limit(upload_limit_kib_per_second);

        if let Some(engine_id) = engine_id {
            let cached_handle = self.handles.lock().await.get(&engine_id).cloned();
            if let Some(handle) = cached_handle {
                let id = handle.id();
                self.prepare_existing_for_resume(&handle, validate_only)
                    .await?;
                return Ok(Some(id));
            }
        }

        for candidate in torrent_resume_candidates(engine_id, info_hash) {
            let Some(handle) = self.session.get(candidate) else {
                continue;
            };

            let id = handle.id();
            self.handles.lock().await.insert(id, handle.clone());
            self.prepare_existing_for_resume(&handle, validate_only)
                .await?;
            return Ok(Some(id));
        }

        Ok(None)
    }

    async fn prepare_existing_for_resume(
        &self,
        handle: &Arc<ManagedTorrent>,
        validate_only: bool,
    ) -> Result<(), String> {
        if validate_only {
            if matches!(handle.stats().state, librqbit::TorrentStatsState::Live) {
                self.session.pause(handle).await.map_err(|error| {
                    format!("Could not pause torrent for seeding restore validation: {error:#}")
                })?;
            }
            return Ok(());
        }

        self.session
            .unpause(handle)
            .await
            .map_err(|error| format!("Could not resume torrent: {error:#}"))
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
        self.peer_cache_hits.lock().await.remove(&id);
        self.last_peer_discovery_assist_actions
            .lock()
            .await
            .remove(&id);
        self.session
            .delete(TorrentIdOrHash::Id(id), false)
            .await
            .map_err(|error| format!("Could not forget torrent: {error:#}"))
    }

    pub async fn cache_metadata(
        &self,
        id: usize,
        app_data_dir: &Path,
    ) -> Result<Option<PathBuf>, String> {
        let handle = self.handle(id).await?;
        let info_hash = handle.info_hash().as_string();
        let Some(path) = torrent_metadata_cache_path(app_data_dir, &info_hash) else {
            return Ok(None);
        };
        let torrent_bytes = handle
            .with_metadata(|metadata| metadata.torrent_bytes.clone())
            .map_err(|error| format!("Could not extract torrent metadata: {error:#}"))?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| format!("Could not create torrent metadata cache: {error}"))?;
        }
        tokio::fs::write(&path, torrent_bytes)
            .await
            .map_err(|error| format!("Could not cache torrent metadata: {error}"))?;
        Ok(Some(path))
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
            self.peer_cache_hits.lock().await.remove(&id);
            self.last_peer_discovery_assist_actions
                .lock()
                .await
                .remove(&id);
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
        let session_stats = self.session.stats_snapshot();
        let dht_nodes = self.dht_node_count();
        let mut snapshot = snapshot_from_handle(
            &handle,
            mib_per_second_to_bytes_per_second(session_stats.download_speed.mbps),
            mib_per_second_to_bytes_per_second(session_stats.upload_speed.mbps),
            self.session.tcp_listen_port(),
            self.listener_fallback_message.is_some(),
            dht_nodes,
            torrent_peer_diagnostics_from_api(&self.api, id),
        );
        if let Some(diagnostics) = snapshot.diagnostics.as_mut() {
            diagnostics.dht_warmup_age_millis = Some(
                self.created_at
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64,
            );
            diagnostics.peer_cache_hits = self.peer_cache_hits.lock().await.get(&id).copied();
            diagnostics.last_peer_discovery_assist_action = self
                .last_peer_discovery_assist_actions
                .lock()
                .await
                .get(&id)
                .cloned();
        }
        Ok(snapshot)
    }

    pub async fn record_peer_discovery_assist_action(&self, id: usize, action: &str) {
        self.last_peer_discovery_assist_actions
            .lock()
            .await
            .insert(id, action.to_string());
    }

    pub async fn cache_peers(
        &self,
        id: usize,
        app_data_dir: &Path,
    ) -> Result<Option<usize>, String> {
        let handle = self.handle(id).await?;
        let is_private = handle
            .with_metadata(|metadata| metadata.info.private)
            .map_err(|error| format!("Could not inspect torrent metadata privacy: {error:#}"))?;
        let snapshot = self
            .api
            .api_peer_stats(
                TorrentIdOrHash::Id(id),
                serde_json::from_value(serde_json::json!({ "state": "All" })).unwrap_or_default(),
            )
            .map_err(|error| format!("Could not inspect torrent peers for cache: {error:#}"))?;
        let observations = snapshot
            .peers
            .into_iter()
            .filter_map(|(address, peer)| {
                let address = address.parse().ok()?;
                Some(TorrentPeerCacheObservation {
                    address,
                    contributing: peer.counters.fetched_bytes > 0
                        || peer.counters.downloaded_and_checked_pieces > 0,
                    live: peer.state == "live",
                    failures: peer.counters.errors,
                })
            })
            .collect::<Vec<_>>();
        if observations.is_empty() && !is_private {
            return Ok(Some(0));
        }
        write_torrent_peer_cache_observations(
            app_data_dir,
            &handle.info_hash().as_string(),
            observations,
            current_unix_seconds(),
            is_private,
        )
        .await
    }

    pub fn dht_node_count(&self) -> Option<u32> {
        self.api
            .api_dht_stats()
            .ok()
            .map(|stats| usize_to_u32(stats.routing_table_size))
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

async fn race_tracker_first_metadata_resolution<MainLookup, TrackerLookup>(
    main_lookup: MainLookup,
    tracker_lookup: TrackerLookup,
    tracker_first_diagnostics: Option<UnboundedSender<TrackerFirstMetadataOutcome>>,
) -> Result<TorrentAddSessionOutcome, String>
where
    MainLookup: Future<Output = Result<TorrentAddSessionOutcome, String>>,
    TrackerLookup: Future<
        Output = Result<Result<TrackerFirstMetadataResult, TrackerFirstMetadataOutcome>, String>,
    >,
{
    tokio::pin!(main_lookup);
    tokio::pin!(tracker_lookup);

    tokio::select! {
        main_result = &mut main_lookup => {
            send_tracker_first_outcome(
                &tracker_first_diagnostics,
                TrackerFirstMetadataOutcome::SupersededByMainSession,
            );
            main_result
        }
        tracker_result = &mut tracker_lookup => {
            match tracker_result? {
                Ok(outcome) => {
                    send_tracker_first_outcome(
                        &tracker_first_diagnostics,
                        TrackerFirstMetadataOutcome::Resolved {
                            initial_peers: outcome.initial_peers,
                        },
                    );
                    Ok(outcome.add_session)
                }
                Err(outcome) => {
                    send_tracker_first_outcome(&tracker_first_diagnostics, outcome);
                    main_lookup.await
                }
            }
        }
    }
}

pub fn prepare_torrent_source(source: &str) -> PreparedTorrentSource {
    prepare_torrent_source_with_custom_trackers(source, &[])
}

pub fn prepare_torrent_source_with_custom_trackers(
    source: &str,
    custom_trackers: &[String],
) -> PreparedTorrentSource {
    let custom_trackers = normalize_custom_torrent_trackers(custom_trackers);
    if source
        .get(..source.len().min("magnet:".len()))
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("magnet:"))
    {
        let info_hash_hint = magnet_info_hash(source);
        let existing_trackers = magnet_tracker_values(source);
        let mut seen = tracker_seen_set(existing_trackers.iter().map(String::as_str));
        let custom_trackers =
            missing_trackers(&mut seen, custom_trackers.iter().map(String::as_str));
        let fallback_trackers =
            missing_trackers(&mut seen, FALLBACK_TORRENT_TRACKERS.iter().copied());
        let mut effective_trackers = existing_trackers.clone();
        effective_trackers.extend(custom_trackers.iter().cloned());
        effective_trackers.extend(fallback_trackers.iter().cloned());
        return PreparedTorrentSource {
            source: append_trackers_to_magnet(
                source,
                &custom_trackers
                    .iter()
                    .chain(fallback_trackers.iter())
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            source_kind: TorrentSourceKind::Magnet,
            info_hash_hint,
            original_tracker_count: existing_trackers.len(),
            custom_trackers_added: custom_trackers.len(),
            fallback_trackers_added: fallback_trackers.len(),
            fallback_trackers_for_options: Vec::new(),
            tracker_protocol_counts: tracker_protocol_counts(
                effective_trackers.iter().map(String::as_str),
            ),
            tracker_first_metadata: true,
        };
    }

    let mut seen = HashSet::new();
    let custom_trackers = missing_trackers(&mut seen, custom_trackers.iter().map(String::as_str));
    let fallback_trackers = missing_trackers(&mut seen, FALLBACK_TORRENT_TRACKERS.iter().copied());
    let mut trackers_for_options = custom_trackers.clone();
    trackers_for_options.extend(fallback_trackers.iter().cloned());
    PreparedTorrentSource {
        source: source.to_string(),
        source_kind: TorrentSourceKind::TorrentFile,
        info_hash_hint: None,
        original_tracker_count: 0,
        custom_trackers_added: custom_trackers.len(),
        fallback_trackers_added: fallback_trackers.len(),
        fallback_trackers_for_options: trackers_for_options.clone(),
        tracker_protocol_counts: tracker_protocol_counts(
            trackers_for_options.iter().map(String::as_str),
        ),
        tracker_first_metadata: false,
    }
}

pub(crate) fn cached_torrent_metadata_source(
    app_data_dir: &Path,
    info_hash: Option<&str>,
) -> Option<String> {
    let path = torrent_metadata_cache_path(app_data_dir, info_hash?)?;
    path.is_file().then(|| path.display().to_string())
}

pub(crate) fn torrent_metadata_cache_path(app_data_dir: &Path, info_hash: &str) -> Option<PathBuf> {
    let info_hash = normalized_torrent_metadata_cache_info_hash(info_hash)?;
    Some(
        app_data_dir
            .join(TORRENT_METADATA_CACHE_DIR)
            .join(format!("{info_hash}.torrent")),
    )
}

pub(crate) fn torrent_peer_cache_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(TORRENT_PEER_CACHE_FILE)
}

fn normalized_torrent_metadata_cache_info_hash(info_hash: &str) -> Option<String> {
    let info_hash = info_hash.trim();
    if info_hash.len() == 40 && info_hash.chars().all(|char| char.is_ascii_hexdigit()) {
        Some(info_hash.to_ascii_lowercase())
    } else {
        None
    }
}

fn optional_initial_peers(peers: Vec<SocketAddr>) -> Option<Vec<SocketAddr>> {
    (!peers.is_empty()).then_some(peers)
}

fn merge_initial_torrent_peers(
    cached_peers: Vec<SocketAddr>,
    tracker_peers: Vec<SocketAddr>,
) -> Vec<SocketAddr> {
    let mut seen = HashSet::new();
    cached_peers
        .into_iter()
        .chain(tracker_peers)
        .filter(|peer| seen.insert(*peer))
        .collect()
}

async fn read_cached_torrent_peers(
    app_data_dir: &Path,
    info_hash: Option<&str>,
) -> Result<Vec<SocketAddr>, String> {
    let Some(info_hash) = info_hash.and_then(normalized_torrent_metadata_cache_info_hash) else {
        return Ok(Vec::new());
    };
    let cache = read_torrent_peer_cache(app_data_dir).await?;
    Ok(cached_torrent_peer_addrs(
        &cache,
        &info_hash,
        current_unix_seconds(),
    ))
}

async fn write_torrent_peer_cache_observations(
    app_data_dir: &Path,
    info_hash: &str,
    observations: Vec<TorrentPeerCacheObservation>,
    now_unix_seconds: u64,
    is_private: bool,
) -> Result<Option<usize>, String> {
    let Some(info_hash) = normalized_torrent_metadata_cache_info_hash(info_hash) else {
        return Ok(None);
    };
    let mut cache = read_torrent_peer_cache(app_data_dir).await?;
    update_torrent_peer_cache_entry(
        &mut cache,
        &info_hash,
        observations,
        now_unix_seconds,
        is_private,
    );
    let Some(parent) = torrent_peer_cache_path(app_data_dir)
        .parent()
        .map(Path::to_path_buf)
    else {
        return Ok(None);
    };
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| format!("Could not create torrent peer cache directory: {error}"))?;
    let body = serde_json::to_vec_pretty(&cache)
        .map_err(|error| format!("Could not serialize torrent peer cache: {error}"))?;
    tokio::fs::write(torrent_peer_cache_path(app_data_dir), body)
        .await
        .map_err(|error| format!("Could not write torrent peer cache: {error}"))?;
    Ok(cache.entries.get(&info_hash).map(|entry| entry.peers.len()))
}

async fn read_torrent_peer_cache(app_data_dir: &Path) -> Result<TorrentPeerCacheFile, String> {
    let path = torrent_peer_cache_path(app_data_dir);
    match tokio::fs::read(&path).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("Could not parse torrent peer cache: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(TorrentPeerCacheFile::default())
        }
        Err(error) => Err(format!("Could not read torrent peer cache: {error}")),
    }
}

fn update_torrent_peer_cache_entry(
    cache: &mut TorrentPeerCacheFile,
    info_hash: &str,
    observations: Vec<TorrentPeerCacheObservation>,
    now_unix_seconds: u64,
    is_private: bool,
) {
    let Some(info_hash) = normalized_torrent_metadata_cache_info_hash(info_hash) else {
        return;
    };
    if is_private {
        cache.entries.remove(&info_hash);
        return;
    }

    let expires_before = now_unix_seconds.saturating_sub(TORRENT_PEER_CACHE_TTL.as_secs());
    let entry = cache.entries.entry(info_hash).or_default();
    let mut peers_by_addr = entry
        .peers
        .drain(..)
        .filter(|peer| {
            peer.last_seen_unix_seconds >= expires_before
                && peer.failures < 3
                && peer.address.parse::<SocketAddr>().is_ok()
        })
        .map(|peer| (peer.address.clone(), peer))
        .collect::<HashMap<_, _>>();

    for observation in observations {
        let address = observation.address.to_string();
        if observation.failures >= 3 && !observation.live && !observation.contributing {
            peers_by_addr.remove(&address);
            continue;
        }
        if !observation.live && !observation.contributing {
            continue;
        }
        let peer = peers_by_addr
            .entry(address.clone())
            .or_insert(TorrentPeerCachePeer {
                address,
                last_seen_unix_seconds: now_unix_seconds,
                failures: 0,
                contributing: false,
                live: false,
            });
        peer.last_seen_unix_seconds = now_unix_seconds;
        peer.failures = observation.failures.min(2);
        peer.contributing |= observation.contributing;
        peer.live |= observation.live;
    }

    entry.peers = peers_by_addr.into_values().collect();
    sort_and_cap_torrent_peer_cache_entry(entry);
}

fn cached_torrent_peer_addrs(
    cache: &TorrentPeerCacheFile,
    info_hash: &str,
    now_unix_seconds: u64,
) -> Vec<SocketAddr> {
    let Some(info_hash) = normalized_torrent_metadata_cache_info_hash(info_hash) else {
        return Vec::new();
    };
    let expires_before = now_unix_seconds.saturating_sub(TORRENT_PEER_CACHE_TTL.as_secs());
    let mut peers = cache
        .entries
        .get(&info_hash)
        .map(|entry| entry.peers.clone())
        .unwrap_or_default();
    peers.retain(|peer| peer.last_seen_unix_seconds >= expires_before && peer.failures < 3);
    let mut entry = TorrentPeerCacheEntry { peers };
    sort_and_cap_torrent_peer_cache_entry(&mut entry);
    entry
        .peers
        .into_iter()
        .filter_map(|peer| peer.address.parse().ok())
        .collect()
}

fn sort_and_cap_torrent_peer_cache_entry(entry: &mut TorrentPeerCacheEntry) {
    entry.peers.sort_by(|left, right| {
        right
            .contributing
            .cmp(&left.contributing)
            .then_with(|| right.live.cmp(&left.live))
            .then_with(|| left.failures.cmp(&right.failures))
            .then_with(|| {
                right
                    .last_seen_unix_seconds
                    .cmp(&left.last_seen_unix_seconds)
            })
            .then_with(|| left.address.cmp(&right.address))
    });
    entry.peers.truncate(MAX_TORRENT_PEER_CACHE_PEERS);
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TorrentPeerDiagnosticsSummary {
    contributing_peers: u32,
    peer_errors: u32,
    peers_with_errors: u32,
    peer_connection_attempts: u32,
    peer_samples: Vec<TorrentPeerDiagnostics>,
}

fn snapshot_from_handle(
    handle: &ManagedTorrent,
    session_download_speed: u64,
    session_upload_speed: u64,
    listen_port: Option<u16>,
    listener_fallback: bool,
    dht_nodes: Option<u32>,
    peer_summary: TorrentPeerDiagnosticsSummary,
) -> TorrentRuntimeSnapshot {
    let stats = handle.stats();
    let diagnostics = torrent_runtime_diagnostics(
        &stats,
        session_download_speed,
        session_upload_speed,
        listen_port,
        listener_fallback,
        dht_nodes,
        peer_summary,
    );
    let peers = diagnostics
        .as_ref()
        .map(|diagnostics| diagnostics.live_peers);
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
        phase: torrent_runtime_phase(&stats),
        finished: stats.finished,
        error: stats.error,
        diagnostics,
    }
}

fn torrent_peer_diagnostics_from_api(api: &Api, id: usize) -> TorrentPeerDiagnosticsSummary {
    let all_peer_states_filter = serde_json::from_value(serde_json::json!({
        "state": "All",
    }))
    .unwrap_or_default();
    let Ok(snapshot) = api.api_peer_stats(TorrentIdOrHash::Id(id), all_peer_states_filter) else {
        return TorrentPeerDiagnosticsSummary::default();
    };

    let peer_samples = snapshot
        .peers
        .values()
        .map(|peer| TorrentPeerDiagnostics {
            state: peer.state.to_string(),
            fetched_bytes: peer.counters.fetched_bytes,
            errors: peer.counters.errors,
            downloaded_pieces: peer.counters.downloaded_and_checked_pieces,
            connection_attempts: peer.counters.connection_attempts,
        })
        .collect::<Vec<_>>();
    torrent_peer_diagnostics_summary_from_samples(peer_samples)
}

fn torrent_peer_diagnostics_summary_from_samples(
    mut peer_samples: Vec<TorrentPeerDiagnostics>,
) -> TorrentPeerDiagnosticsSummary {
    let contributing_peers = usize_to_u32(
        peer_samples
            .iter()
            .filter(|peer| peer.fetched_bytes > 0)
            .count(),
    );
    let peer_errors = peer_samples
        .iter()
        .fold(0_u32, |total, peer| total.saturating_add(peer.errors));
    let peers_with_errors =
        usize_to_u32(peer_samples.iter().filter(|peer| peer.errors > 0).count());
    let peer_connection_attempts = peer_samples.iter().fold(0_u32, |total, peer| {
        total.saturating_add(peer.connection_attempts)
    });

    peer_samples.sort_by(|left, right| {
        right
            .fetched_bytes
            .cmp(&left.fetched_bytes)
            .then_with(|| right.errors.cmp(&left.errors))
            .then_with(|| left.state.cmp(&right.state))
    });
    peer_samples.truncate(5);

    TorrentPeerDiagnosticsSummary {
        contributing_peers,
        peer_errors,
        peers_with_errors,
        peer_connection_attempts,
        peer_samples,
    }
}

fn torrent_runtime_diagnostics(
    stats: &librqbit::TorrentStats,
    session_download_speed: u64,
    session_upload_speed: u64,
    listen_port: Option<u16>,
    listener_fallback: bool,
    dht_nodes: Option<u32>,
    peer_summary: TorrentPeerDiagnosticsSummary,
) -> Option<TorrentRuntimeDiagnostics> {
    let live = stats.live.as_ref()?;
    let peers = &live.snapshot.peer_stats;

    Some(TorrentRuntimeDiagnostics {
        queued_peers: usize_to_u32(peers.queued),
        connecting_peers: usize_to_u32(peers.connecting),
        live_peers: usize_to_u32(peers.live),
        seen_peers: usize_to_u32(peers.seen),
        dead_peers: usize_to_u32(peers.dead),
        not_needed_peers: usize_to_u32(peers.not_needed),
        contributing_peers: peer_summary.contributing_peers,
        peer_errors: peer_summary.peer_errors,
        peers_with_errors: peer_summary.peers_with_errors,
        peer_connection_attempts: peer_summary.peer_connection_attempts,
        session_download_speed,
        session_upload_speed,
        dht_nodes,
        dht_warmup_age_millis: None,
        peer_cache_hits: None,
        milliseconds_since_metadata_resolved: None,
        first_live_peer_millis: None,
        first_contributing_peer_millis: None,
        first_payload_millis: None,
        dht_nodes_at_metadata_resolved: None,
        last_peer_discovery_assist_action: None,
        average_piece_download_millis: average_piece_download_millis(live),
        listen_port,
        listener_fallback,
        peer_samples: peer_summary.peer_samples,
    })
}

fn average_piece_download_millis(live: &librqbit::api::LiveStats) -> Option<u64> {
    live.average_piece_download_time
        .or_else(|| live.snapshot.average_piece_download_time())
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

fn usize_to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn torrent_runtime_phase(stats: &librqbit::TorrentStats) -> TorrentRuntimePhase {
    match stats.state {
        librqbit::TorrentStatsState::Initializing => TorrentRuntimePhase::Initializing,
        librqbit::TorrentStatsState::Paused => TorrentRuntimePhase::Paused,
        librqbit::TorrentStatsState::Live => TorrentRuntimePhase::Live,
        librqbit::TorrentStatsState::Error => TorrentRuntimePhase::Error,
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
    start_paused: bool,
    fallback_trackers: &[String],
    initial_peers: Option<Vec<SocketAddr>>,
) -> AddTorrentOptions {
    AddTorrentOptions {
        paused: start_paused,
        output_folder: Some(output_folder.display().to_string()),
        overwrite: true,
        ratelimits: torrent_limits(upload_limit_kib_per_second),
        trackers: (!fallback_trackers.is_empty()).then(|| fallback_trackers.to_vec()),
        initial_peers,
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

pub(crate) fn normalize_custom_torrent_trackers(trackers: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for tracker in trackers {
        let Some(tracker) = normalized_tracker_url(tracker) else {
            continue;
        };
        let key = tracker.to_ascii_lowercase();
        if seen.insert(key) {
            normalized.push(tracker);
            if normalized.len() >= MAX_CUSTOM_TORRENT_TRACKERS {
                break;
            }
        }
    }
    normalized
}

fn normalized_tracker_url(tracker: &str) -> Option<String> {
    let trimmed = tracker.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = url::Url::parse(trimmed).ok()?;
    if !matches!(parsed.scheme(), "udp" | "http" | "https") {
        return None;
    }
    parsed.host_str()?;
    let mut normalized = parsed;
    normalized.set_fragment(None);
    Some(normalized.to_string())
}

fn tracker_seen_set<'a>(trackers: impl Iterator<Item = &'a str>) -> HashSet<String> {
    trackers
        .filter_map(normalized_tracker_url)
        .map(|tracker| tracker.to_ascii_lowercase())
        .collect()
}

fn missing_trackers<'a>(
    seen: &mut HashSet<String>,
    trackers: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    trackers
        .filter_map(normalized_tracker_url)
        .filter(|tracker| seen.insert(tracker.to_ascii_lowercase()))
        .collect()
}

fn tracker_protocol_counts<'a>(
    trackers: impl Iterator<Item = &'a str>,
) -> TorrentTrackerProtocolCounts {
    let mut counts = TorrentTrackerProtocolCounts::default();
    for tracker in trackers {
        let Some(tracker) = normalized_tracker_url(tracker) else {
            continue;
        };
        match url::Url::parse(&tracker)
            .ok()
            .map(|url| url.scheme().to_string())
        {
            Some(scheme) if scheme == "http" => counts.http += 1,
            Some(scheme) if scheme == "https" => counts.https += 1,
            Some(scheme) if scheme == "udp" => counts.udp += 1,
            _ => {}
        }
    }
    counts
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
            "magnet:?xt=urn:btih:a634dc946d49989526058626caa3bbabba4607b6&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&tr=udp%3A%2F%2Fcustom.example%3A1337%2Fannounce",
        );

        let parsed = url::Url::parse(&prepared.source).expect("prepared magnet should parse");
        let trackers = parsed
            .query_pairs()
            .filter_map(|(key, value)| (key == "tr").then(|| value.into_owned()))
            .collect::<Vec<_>>();

        assert_eq!(trackers[0], "udp://tracker.opentrackr.org:1337/announce");
        assert_eq!(trackers[1], "udp://custom.example:1337/announce");
        assert_eq!(
            trackers
                .iter()
                .filter(|tracker| tracker.as_str() == "udp://tracker.opentrackr.org:1337/announce")
                .count(),
            1
        );
        assert_eq!(
            prepared.fallback_trackers_added,
            FALLBACK_TORRENT_TRACKERS.len() - 1
        );
    }

    #[test]
    fn custom_trackers_are_trimmed_validated_deduped_and_capped() {
        let mut trackers = vec![
            " https://tracker.example/announce ".to_string(),
            "HTTPS://tracker.example/announce".to_string(),
            "udp://tracker.example:1337/announce".to_string(),
            "ftp://tracker.example/announce".to_string(),
            "".to_string(),
        ];
        for index in 0..70 {
            trackers.push(format!("http://tracker-{index}.example/announce"));
        }

        let normalized = normalize_custom_torrent_trackers(&trackers);

        assert_eq!(normalized.len(), 64);
        assert_eq!(normalized[0], "https://tracker.example/announce");
        assert_eq!(normalized[1], "udp://tracker.example:1337/announce");
        assert!(!normalized
            .iter()
            .any(|tracker| tracker.starts_with("ftp://")));
        assert_eq!(
            normalized
                .iter()
                .filter(|tracker| tracker.eq_ignore_ascii_case("https://tracker.example/announce"))
                .count(),
            1
        );
    }

    #[test]
    fn magnet_preparation_preserves_original_then_custom_then_bundled_trackers() {
        let prepared = prepare_torrent_source_with_custom_trackers(
            "magnet:?xt=urn:btih:a634dc946d49989526058626caa3bbabba4607b6&tr=udp%3A%2F%2Foriginal.example%3A1337%2Fannounce",
            &[
                "https://custom.example/announce".to_string(),
                "udp://tracker.opentrackr.org:1337/announce".to_string(),
            ],
        );

        let trackers = magnet_tracker_values(&prepared.source);

        assert_eq!(trackers[0], "udp://original.example:1337/announce");
        assert_eq!(trackers[1], "https://custom.example/announce");
        assert_eq!(trackers[2], "udp://tracker.opentrackr.org:1337/announce");
        assert_eq!(prepared.original_tracker_count, 1);
        assert_eq!(prepared.custom_trackers_added, 2);
        assert_eq!(
            prepared.fallback_trackers_added,
            FALLBACK_TORRENT_TRACKERS.len() - 1
        );
        assert!(prepared.tracker_source_summary().contains("original 1"));
        assert!(prepared.tracker_source_summary().contains("custom 2"));
        assert!(!prepared.tracker_source_summary().contains("custom.example"));
    }

    #[test]
    fn torrent_file_options_include_fallback_trackers() {
        let prepared = prepare_torrent_source_with_custom_trackers(
            "https://example.com/releases/file.torrent",
            &["https://custom.example/announce".to_string()],
        );
        let options = torrent_add_options(
            Path::new("C:/Downloads/file"),
            0,
            false,
            &prepared.fallback_trackers_for_options,
            None,
        );

        assert_eq!(prepared.source, "https://example.com/releases/file.torrent");
        assert_eq!(prepared.source_kind, TorrentSourceKind::TorrentFile);
        assert!(!prepared.tracker_first_metadata);
        let trackers = options.trackers.as_ref().expect("fallback trackers");
        assert_eq!(trackers[0], "https://custom.example/announce");
        assert_eq!(trackers[1], FALLBACK_TORRENT_TRACKERS[0]);
    }

    #[test]
    fn torrent_add_options_forward_tracker_first_seen_peers_as_initial_peers() {
        let peer = "127.0.0.1:6881".parse().expect("socket address");
        let options = torrent_add_options(
            Path::new("C:/Downloads/file"),
            0,
            false,
            &[],
            Some(vec![peer]),
        );

        assert_eq!(options.initial_peers, Some(vec![peer]));
    }

    #[test]
    fn initial_peer_merge_dedupes_cached_then_tracker_peers() {
        let cached_a = "127.0.0.1:6881".parse().expect("cached peer");
        let cached_b = "127.0.0.2:6881".parse().expect("cached peer");
        let tracker_b = cached_b;
        let tracker_c = "127.0.0.3:6881".parse().expect("tracker peer");

        assert_eq!(
            merge_initial_torrent_peers(vec![cached_a, cached_b], vec![tracker_b, tracker_c]),
            vec![cached_a, cached_b, tracker_c],
            "cached peers should be tried first while tracker-first seen peers still fill gaps"
        );
    }

    #[test]
    fn peer_cache_prefers_contributing_live_peers_and_expires_old_entries() {
        let info_hash = "a634dc946d49989526058626caa3bbabba4607b6";
        let now = 1_800_000_000;
        let stale = now - TORRENT_PEER_CACHE_TTL.as_secs() - 1;
        let mut cache = TorrentPeerCacheFile::default();
        cache.entries.insert(
            info_hash.into(),
            TorrentPeerCacheEntry {
                peers: vec![TorrentPeerCachePeer {
                    address: "127.0.0.9:6881".into(),
                    last_seen_unix_seconds: stale,
                    failures: 0,
                    contributing: true,
                    live: true,
                }],
            },
        );

        update_torrent_peer_cache_entry(
            &mut cache,
            info_hash,
            vec![
                TorrentPeerCacheObservation {
                    address: "127.0.0.1:6881".parse().expect("peer"),
                    contributing: false,
                    live: true,
                    failures: 0,
                },
                TorrentPeerCacheObservation {
                    address: "127.0.0.2:6881".parse().expect("peer"),
                    contributing: true,
                    live: true,
                    failures: 0,
                },
                TorrentPeerCacheObservation {
                    address: "127.0.0.3:6881".parse().expect("peer"),
                    contributing: false,
                    live: false,
                    failures: 3,
                },
            ],
            now,
            false,
        );

        let cached = cached_torrent_peer_addrs(&cache, info_hash, now);

        assert_eq!(cached[0], "127.0.0.2:6881".parse().unwrap());
        assert_eq!(cached[1], "127.0.0.1:6881".parse().unwrap());
        assert!(!cached.contains(&"127.0.0.9:6881".parse().unwrap()));
        assert!(!cached.contains(&"127.0.0.3:6881".parse().unwrap()));
    }

    #[test]
    fn peer_cache_skips_private_torrents_and_caps_entries() {
        let info_hash = "a634dc946d49989526058626caa3bbabba4607b6";
        let now = 1_800_000_000;
        let mut private_cache = TorrentPeerCacheFile::default();
        update_torrent_peer_cache_entry(
            &mut private_cache,
            info_hash,
            vec![TorrentPeerCacheObservation {
                address: "127.0.0.1:6881".parse().expect("peer"),
                contributing: true,
                live: true,
                failures: 0,
            }],
            now,
            true,
        );
        assert!(!private_cache.entries.contains_key(info_hash));

        let mut public_cache = TorrentPeerCacheFile::default();
        let observations = (0..80)
            .map(|index| TorrentPeerCacheObservation {
                address: format!("127.0.1.{index}:6881").parse().expect("peer"),
                contributing: index % 2 == 0,
                live: true,
                failures: 0,
            })
            .collect::<Vec<_>>();
        update_torrent_peer_cache_entry(&mut public_cache, info_hash, observations, now, false);

        assert_eq!(
            cached_torrent_peer_addrs(&public_cache, info_hash, now).len(),
            64
        );
    }

    #[test]
    fn session_options_enable_listen_range_and_peer_timeouts() {
        let options =
            torrent_session_options(PathBuf::from("session"), &TorrentSettings::default());

        assert_eq!(TORRENT_PEER_CONNECT_TIMEOUT, Duration::from_secs(7));
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
        assert!(!TrackerFirstMetadataOutcome::Resolved { initial_peers: 4 }
            .should_fallback_to_main_session());
        assert!(
            !TrackerFirstMetadataOutcome::SupersededByMainSession.should_fallback_to_main_session()
        );
    }

    #[test]
    fn tracker_first_timeout_is_shorter_than_outer_metadata_timeout() {
        assert_eq!(
            TORRENT_TRACKER_FIRST_METADATA_TIMEOUT,
            Duration::from_secs(8)
        );
    }

    #[tokio::test]
    async fn metadata_race_returns_main_result_without_waiting_for_tracker_timeout() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let expected = TorrentAddSessionOutcome {
            engine_id: 7,
            reused_existing_session: false,
        };

        let result = race_tracker_first_metadata_resolution(
            async { Ok(expected) },
            std::future::pending::<
                Result<Result<TrackerFirstMetadataResult, TrackerFirstMetadataOutcome>, String>,
            >(),
            Some(tx),
        )
        .await
        .expect("main metadata result");

        assert_eq!(result, expected);
        assert_eq!(
            rx.try_recv().expect("tracker-first outcome"),
            TrackerFirstMetadataOutcome::SupersededByMainSession
        );
    }

    #[tokio::test]
    async fn metadata_race_returns_tracker_result_when_tracker_resolves_first() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let expected = TorrentAddSessionOutcome {
            engine_id: 11,
            reused_existing_session: true,
        };

        let result = race_tracker_first_metadata_resolution(
            std::future::pending::<Result<TorrentAddSessionOutcome, String>>(),
            async {
                Ok(Ok(TrackerFirstMetadataResult {
                    add_session: expected,
                    initial_peers: 3,
                }))
            },
            Some(tx),
        )
        .await
        .expect("tracker-first metadata result");

        assert_eq!(result, expected);
        assert_eq!(
            rx.try_recv().expect("tracker-first outcome"),
            TrackerFirstMetadataOutcome::Resolved { initial_peers: 3 }
        );
    }

    #[test]
    fn peer_diagnostics_summary_counts_non_live_peer_errors_and_attempts() {
        let summary = torrent_peer_diagnostics_summary_from_samples(vec![
            TorrentPeerDiagnostics {
                state: "live".into(),
                fetched_bytes: 4096,
                errors: 0,
                downloaded_pieces: 1,
                connection_attempts: 1,
            },
            TorrentPeerDiagnostics {
                state: "dead".into(),
                fetched_bytes: 0,
                errors: 2,
                downloaded_pieces: 0,
                connection_attempts: 3,
            },
            TorrentPeerDiagnostics {
                state: "connecting".into(),
                fetched_bytes: 0,
                errors: 1,
                downloaded_pieces: 0,
                connection_attempts: 2,
            },
        ]);

        assert_eq!(summary.contributing_peers, 1);
        assert_eq!(summary.peer_errors, 3);
        assert_eq!(summary.peers_with_errors, 2);
        assert_eq!(summary.peer_connection_attempts, 6);
        assert!(summary
            .peer_samples
            .iter()
            .any(|sample| sample.state == "dead" && sample.errors == 2));
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
        let live = librqbit::api::LiveStats {
            download_speed: 1.5.into(),
            ..Default::default()
        };
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
        let live = librqbit::api::LiveStats {
            upload_speed: 0.75.into(),
            ..Default::default()
        };
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
    fn runtime_diagnostics_use_peer_session_and_listener_counters() {
        let mut live = librqbit::api::LiveStats::default();
        live.snapshot.peer_stats.queued = 3;
        live.snapshot.peer_stats.connecting = 2;
        live.snapshot.peer_stats.live = 12;
        live.snapshot.peer_stats.seen = 28;
        live.snapshot.peer_stats.dead = 4;
        live.snapshot.peer_stats.not_needed = 5;
        live.snapshot.downloaded_and_checked_pieces = 3;
        live.snapshot.total_piece_download_ms = 1_500;
        let stats = librqbit::TorrentStats {
            state: librqbit::TorrentStatsState::Live,
            file_progress: vec![1024],
            error: None,
            progress_bytes: 1024,
            uploaded_bytes: 128,
            total_bytes: 2048,
            finished: false,
            live: Some(live),
        };

        let diagnostics = torrent_runtime_diagnostics(
            &stats,
            512 * 1024,
            64 * 1024,
            Some(42000),
            true,
            Some(96),
            TorrentPeerDiagnosticsSummary {
                contributing_peers: 4,
                peer_errors: 2,
                peers_with_errors: 1,
                peer_connection_attempts: 3,
                peer_samples: vec![crate::storage::TorrentPeerDiagnostics {
                    state: "live".into(),
                    fetched_bytes: 256 * 1024,
                    errors: 1,
                    downloaded_pieces: 2,
                    connection_attempts: 1,
                }],
            },
        )
        .expect("live torrent should have diagnostics");

        assert_eq!(diagnostics.live_peers, 12);
        assert_eq!(diagnostics.queued_peers, 3);
        assert_eq!(diagnostics.connecting_peers, 2);
        assert_eq!(diagnostics.seen_peers, 28);
        assert_eq!(diagnostics.dead_peers, 4);
        assert_eq!(diagnostics.not_needed_peers, 5);
        assert_eq!(diagnostics.session_download_speed, 512 * 1024);
        assert_eq!(diagnostics.session_upload_speed, 64 * 1024);
        assert_eq!(diagnostics.average_piece_download_millis, Some(500));
        assert_eq!(diagnostics.listen_port, Some(42000));
        assert!(diagnostics.listener_fallback);
        assert_eq!(diagnostics.dht_nodes, Some(96));
        assert_eq!(diagnostics.contributing_peers, 4);
        assert_eq!(diagnostics.peer_errors, 2);
        assert_eq!(diagnostics.peers_with_errors, 1);
        assert_eq!(diagnostics.peer_connection_attempts, 3);
        assert_eq!(diagnostics.peer_samples.len(), 1);
    }

    #[test]
    fn runtime_diagnostics_are_none_without_live_stats() {
        let stats = librqbit::TorrentStats {
            state: librqbit::TorrentStatsState::Paused,
            file_progress: vec![1024],
            error: None,
            progress_bytes: 1024,
            uploaded_bytes: 128,
            total_bytes: 2048,
            finished: false,
            live: None,
        };

        assert!(torrent_runtime_diagnostics(
            &stats,
            512 * 1024,
            64 * 1024,
            Some(42000),
            false,
            None,
            TorrentPeerDiagnosticsSummary::default(),
        )
        .is_none());
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
