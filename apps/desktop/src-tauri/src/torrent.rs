use crate::state::TorrentRuntimeSnapshot;
use librqbit::api::TorrentIdOrHash;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, ManagedTorrent, PeerConnectionOptions, Session, SessionOptions,
    SessionPersistenceConfig,
};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

pub const TORRENT_LISTEN_PORT_RANGE: Range<u16> = 42000..42100;
pub const TORRENT_PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
pub const TORRENT_PEER_READ_WRITE_TIMEOUT: Duration = Duration::from_secs(60);
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
}

pub struct TorrentEngine {
    session: Arc<Session>,
    handles: Arc<Mutex<HashMap<usize, Arc<ManagedTorrent>>>>,
    listener_fallback_message: Option<String>,
    listener_fallback_reported: AtomicBool,
}

impl TorrentEngine {
    pub async fn new(default_output_folder: PathBuf, data_dir: PathBuf) -> Result<Self, String> {
        tokio::fs::create_dir_all(&default_output_folder)
            .await
            .map_err(|error| format!("Could not create torrent download directory: {error}"))?;
        let persistence_dir = data_dir.join("torrent-session");
        tokio::fs::create_dir_all(&persistence_dir)
            .await
            .map_err(|error| format!("Could not create torrent session directory: {error}"))?;

        let (session, listener_fallback_message) = match Session::new_with_opts(
            default_output_folder.clone(),
            torrent_session_options(persistence_dir.clone()),
        )
        .await
        {
            Ok(session) => (session, None),
            Err(error) if is_listen_error(&format!("{error:#}")) => {
                let message = format!(
                        "Torrent listen ports 42000-42099 are unavailable; continuing without inbound peer listener: {error:#}"
                    );
                let fallback_session = Session::new_with_opts(
                    default_output_folder,
                    torrent_session_options_with_listener(persistence_dir, None),
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
    ) -> Result<usize, String> {
        tokio::fs::create_dir_all(output_folder)
            .await
            .map_err(|error| format!("Could not create torrent output directory: {error}"))?;
        self.set_upload_limit(upload_limit_kib_per_second);
        let options = torrent_add_options(
            output_folder,
            upload_limit_kib_per_second,
            &source.fallback_trackers_for_options,
        );
        let add_torrent = AddTorrent::from_cli_argument(&source.source)
            .map_err(|error| format!("Could not read torrent source: {error:#}"))?;
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
        };
    }

    let fallback_trackers = missing_fallback_trackers(std::iter::empty::<&str>());
    PreparedTorrentSource {
        source: source.to_string(),
        source_kind: TorrentSourceKind::TorrentFile,
        fallback_trackers_added: fallback_trackers.len(),
        fallback_trackers_for_options: fallback_trackers,
    }
}

pub(crate) fn torrent_session_options(persistence_dir: PathBuf) -> SessionOptions {
    torrent_session_options_with_listener(persistence_dir, Some(TORRENT_LISTEN_PORT_RANGE))
}

fn torrent_session_options_with_listener(
    persistence_dir: PathBuf,
    listen_port_range: Option<Range<u16>>,
) -> SessionOptions {
    SessionOptions {
        fastresume: true,
        persistence: Some(SessionPersistenceConfig::Json {
            folder: Some(persistence_dir),
        }),
        peer_opts: Some(PeerConnectionOptions {
            connect_timeout: Some(TORRENT_PEER_CONNECT_TIMEOUT),
            read_write_timeout: Some(TORRENT_PEER_READ_WRITE_TIMEOUT),
            keep_alive_interval: None,
        }),
        listen_port_range,
        enable_upnp_port_forwarding: false,
        ..Default::default()
    }
}

fn is_listen_error(message: &str) -> bool {
    message.contains("error listening on TCP") || message.contains("no ports in range")
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
        download_speed: 0,
        finished: stats.finished,
        error: stats.error,
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
        .checked_mul(1024)
        .and_then(NonZeroU32::new)
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
        let options = torrent_session_options(PathBuf::from("session"));

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
}
