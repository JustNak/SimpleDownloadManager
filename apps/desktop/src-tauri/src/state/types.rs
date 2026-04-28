use crate::storage::{
    BulkArchiveInfo, DesktopSnapshot, DownloadSource, HandoffAuth, TorrentInfo, TransferKind,
};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BackendError {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub transfer_kind: TransferKind,
    pub torrent: Option<TorrentInfo>,
    pub handoff_auth: Option<HandoffAuth>,
    pub target_path: PathBuf,
    pub temp_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct BulkArchiveReady {
    pub archive_id: String,
    pub output_path: PathBuf,
    pub entries: Vec<BulkArchiveEntry>,
}

#[derive(Debug, Clone)]
pub struct BulkArchiveEntry {
    pub source_path: PathBuf,
    pub archive_name: String,
}

#[derive(Debug, Clone)]
pub struct ExternalUsePreparation {
    pub paused_torrent: bool,
    pub snapshot: Option<DesktopSnapshot>,
}

#[derive(Debug, Clone)]
pub struct TorrentRuntimeSnapshot {
    pub engine_id: usize,
    pub info_hash: String,
    pub name: Option<String>,
    pub total_files: Option<u32>,
    pub peers: Option<u32>,
    pub seeds: Option<u32>,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub uploaded_bytes: u64,
    pub download_speed: u64,
    pub finished: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueStatus {
    Queued,
    DuplicateExistingJob,
}

impl EnqueueStatus {
    pub fn as_protocol_value(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::DuplicateExistingJob => "duplicate_existing_job",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DuplicatePolicy {
    #[default]
    ReturnExisting,
    Allow,
    ReplaceExisting,
}

#[derive(Debug, Clone, Default)]
pub struct EnqueueOptions {
    pub source: Option<DownloadSource>,
    pub directory_override: Option<String>,
    pub filename_hint: Option<String>,
    pub expected_sha256: Option<String>,
    pub transfer_kind: Option<TransferKind>,
    pub duplicate_policy: DuplicatePolicy,
    pub bulk_archive: Option<BulkArchiveInfo>,
    pub handoff_auth: Option<HandoffAuth>,
}

#[derive(Debug, Clone)]
pub struct EnqueueResult {
    pub snapshot: DesktopSnapshot,
    pub job_id: String,
    pub filename: String,
    pub status: EnqueueStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerControl {
    Continue,
    Paused,
    Canceled,
    Missing,
}

#[derive(Debug, Clone)]
pub enum ExternalReseedAttempt {
    Queued(DesktopSnapshot),
    Pending,
    Stop,
}
