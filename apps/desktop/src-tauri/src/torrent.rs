use crate::state::TorrentRuntimeSnapshot;
use librqbit::api::TorrentIdOrHash;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, ManagedTorrent, Session, SessionOptions,
    SessionPersistenceConfig,
};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct TorrentEngine {
    session: Arc<Session>,
    handles: Arc<Mutex<HashMap<usize, Arc<ManagedTorrent>>>>,
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

        let session = Session::new_with_opts(
            default_output_folder,
            SessionOptions {
                fastresume: true,
                persistence: Some(SessionPersistenceConfig::Json {
                    folder: Some(persistence_dir),
                }),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| format!("Could not initialize torrent engine: {error:#}"))?;

        Ok(Self {
            session,
            handles: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn add_source(
        &self,
        source: &str,
        output_folder: &Path,
        upload_limit_kib_per_second: u32,
    ) -> Result<usize, String> {
        tokio::fs::create_dir_all(output_folder)
            .await
            .map_err(|error| format!("Could not create torrent output directory: {error}"))?;
        self.set_upload_limit(upload_limit_kib_per_second);
        let options = AddTorrentOptions {
            paused: true,
            output_folder: Some(output_folder.display().to_string()),
            overwrite: true,
            ratelimits: torrent_limits(upload_limit_kib_per_second),
            ..Default::default()
        };
        let add_torrent = AddTorrent::from_cli_argument(source)
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

fn torrent_limits(upload_limit_kib_per_second: u32) -> LimitsConfig {
    LimitsConfig {
        upload_bps: upload_limit_bps(upload_limit_kib_per_second),
        download_bps: None,
    }
}

fn upload_limit_bps(upload_limit_kib_per_second: u32) -> Option<NonZeroU32> {
    upload_limit_kib_per_second
        .checked_mul(1024)
        .and_then(NonZeroU32::new)
}
