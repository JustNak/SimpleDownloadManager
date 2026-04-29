use crate::storage::{
    default_download_directory, default_extension_listen_port,
    default_torrent_port_forwarding_port, load_persisted_state, persist_state, BulkArchiveInfo,
    BulkArchiveStatus, ConnectionState, DesktopSnapshot, DiagnosticEvent, DiagnosticLevel,
    DiagnosticsSnapshot, DownloadJob, DownloadPerformanceMode, DownloadPrompt, DownloadSource,
    ExtensionIntegrationSettings, FailureCategory, HandoffAuth, HandoffAuthHeader,
    HostRegistrationDiagnostics, IntegrityAlgorithm, IntegrityCheck, IntegrityStatus, JobState,
    MainWindowState, PersistedState, QueueSummary, ResumeSupport, Settings, TorrentInfo,
    TorrentSeedMode, TorrentSettings, TransferKind,
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
pub(crate) use torrent::should_stop_seeding;
pub use types::{
    BackendError, BulkArchiveEntry, BulkArchiveReady, DownloadTask, DuplicatePolicy,
    EnqueueOptions, EnqueueResult, EnqueueStatus, ExternalReseedAttempt, ExternalUsePreparation,
    TorrentRemovalCleanupInfo, TorrentRuntimePhase, TorrentRuntimeSnapshot,
    TorrentSeedingRestoreFailure, WorkerControl,
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
    active_workers: HashSet<String>,
    external_reseed_jobs: HashSet<String>,
    last_host_contact: Option<Instant>,
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

#[cfg(test)]
mod tests;
