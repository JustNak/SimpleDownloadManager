use crate::storage::{DesktopSnapshot, DiagnosticsSnapshot, DownloadPrompt, Settings};
use std::future::Future;
use std::pin::Pin;

pub const STATE_CHANGED_EVENT: &str = "app://state-changed";
pub const SELECT_JOB_EVENT: &str = "app://select-job";
pub const UPDATE_INSTALL_PROGRESS_EVENT: &str = "app://update-install-progress";

pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellError {
    pub operation: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddJobRequest {
    pub url: String,
    pub directory_override: Option<String>,
    pub filename_hint: Option<String>,
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddJobsRequest {
    pub urls: Vec<String>,
    pub bulk_archive_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddJobStatus {
    Queued,
    DuplicateExistingJob,
}

impl AddJobStatus {
    pub fn as_protocol_value(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::DuplicateExistingJob => "duplicate_existing_job",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddJobResult {
    pub job_id: String,
    pub filename: String,
    pub status: AddJobStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddJobsResult {
    pub results: Vec<AddJobResult>,
    pub queued_count: usize,
    pub duplicate_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmPromptRequest {
    pub id: String,
    pub directory_override: Option<String>,
    pub duplicate_action: PromptDuplicateAction,
    pub renamed_filename: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PromptDuplicateAction {
    #[default]
    ReturnExisting,
    DownloadAnyway,
    Overwrite,
    Rename,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalUseResult {
    pub paused_torrent: bool,
    pub auto_reseed_retry_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressBatchContext {
    pub batch_id: Option<String>,
    pub job_ids: Vec<String>,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentSessionCacheClearResult {
    pub cleared: bool,
    pub pending_restart: bool,
    pub session_path: String,
}

#[derive(Debug, Clone)]
pub enum DesktopEvent {
    StateChanged(Box<DesktopSnapshot>),
    DownloadPromptChanged(Option<Box<DownloadPrompt>>),
    SelectJobRequested(String),
    UpdateInstallProgress(UpdateInstallProgressEvent),
    ShellError(ShellError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateInstallProgressEvent {
    Started { content_length: Option<u64> },
    Progress { chunk_length: usize },
    Finished,
}

pub trait DesktopBackend: Send + Sync {
    fn get_app_snapshot(&self) -> BackendFuture<'_, DesktopSnapshot>;
    fn get_diagnostics(&self) -> BackendFuture<'_, DiagnosticsSnapshot>;
    fn export_diagnostics_report(&self) -> BackendFuture<'_, Option<String>>;
    fn add_job(&self, request: AddJobRequest) -> BackendFuture<'_, AddJobResult>;
    fn add_jobs(&self, request: AddJobsRequest) -> BackendFuture<'_, AddJobsResult>;
    fn pause_job(&self, id: String) -> BackendFuture<'_, ()>;
    fn resume_job(&self, id: String) -> BackendFuture<'_, ()>;
    fn pause_all_jobs(&self) -> BackendFuture<'_, ()>;
    fn resume_all_jobs(&self) -> BackendFuture<'_, ()>;
    fn cancel_job(&self, id: String) -> BackendFuture<'_, ()>;
    fn retry_job(&self, id: String) -> BackendFuture<'_, ()>;
    fn restart_job(&self, id: String) -> BackendFuture<'_, ()>;
    fn retry_failed_jobs(&self) -> BackendFuture<'_, ()>;
    fn swap_failed_download_to_browser(&self, id: String) -> BackendFuture<'_, ()>;
    fn remove_job(&self, id: String) -> BackendFuture<'_, ()>;
    fn delete_job(&self, id: String, delete_from_disk: bool) -> BackendFuture<'_, ()>;
    fn rename_job(&self, id: String, filename: String) -> BackendFuture<'_, ()>;
    fn clear_completed_jobs(&self) -> BackendFuture<'_, ()>;
    fn save_settings(&self, settings: Settings) -> BackendFuture<'_, Settings>;
    fn browse_directory(&self) -> BackendFuture<'_, Option<String>>;
    fn clear_torrent_session_cache(&self) -> BackendFuture<'_, TorrentSessionCacheClearResult>;
    fn browse_torrent_file(&self) -> BackendFuture<'_, Option<String>>;
    fn get_current_download_prompt(&self) -> BackendFuture<'_, Option<DownloadPrompt>>;
    fn confirm_download_prompt(&self, request: ConfirmPromptRequest) -> BackendFuture<'_, ()>;
    fn show_existing_download_prompt(&self, id: String) -> BackendFuture<'_, ()>;
    fn swap_download_prompt(&self, id: String) -> BackendFuture<'_, ()>;
    fn cancel_download_prompt(&self, id: String) -> BackendFuture<'_, ()>;
    fn take_pending_selected_job_request(&self) -> BackendFuture<'_, Option<String>>;
    fn open_progress_window(&self, id: String) -> BackendFuture<'_, ()>;
    fn open_batch_progress_window(
        &self,
        context: ProgressBatchContext,
    ) -> BackendFuture<'_, String>;
    fn get_progress_batch_context(
        &self,
        batch_id: String,
    ) -> BackendFuture<'_, Option<ProgressBatchContext>>;
    fn open_job_file(&self, id: String) -> BackendFuture<'_, ExternalUseResult>;
    fn reveal_job_in_folder(&self, id: String) -> BackendFuture<'_, ExternalUseResult>;
    fn open_install_docs(&self) -> BackendFuture<'_, ()>;
    fn run_host_registration_fix(&self) -> BackendFuture<'_, ()>;
    fn test_extension_handoff(&self) -> BackendFuture<'_, ()>;
    fn check_for_update(&self) -> BackendFuture<'_, Option<AppUpdateMetadata>>;
    fn install_update(&self) -> BackendFuture<'_, ()>;
}

pub trait ShellServices: Send + Sync {
    fn emit_event(&self, event: DesktopEvent) -> BackendFuture<'_, ()>;
    fn notify(&self, title: String, body: String) -> BackendFuture<'_, ()>;
    fn open_path(&self, path: String) -> BackendFuture<'_, ()>;
    fn reveal_path(&self, path: String) -> BackendFuture<'_, ()>;
    fn close_to_tray(&self) -> BackendFuture<'_, ()>;
    fn request_exit(&self) -> BackendFuture<'_, ()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUpdateMetadata {
    pub version: String,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
}
