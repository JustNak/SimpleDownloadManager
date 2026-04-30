pub use crate::prompts::PromptDuplicateAction;
use crate::prompts::PromptRegistry;
use crate::state::{SharedState, TorrentSessionCacheClearResult};
use crate::storage::{
    DesktopSnapshot, DiagnosticsSnapshot, DownloadPrompt, DownloadSource, HandoffAuth,
    HostRegistrationDiagnostics, Settings, TorrentInfo, TorrentSettings, TransferKind,
};
use serde::{Deserialize, Serialize};
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
    pub transfer_kind: Option<TransferKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddJobsRequest {
    pub urls: Vec<String>,
    pub bulk_archive_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddJobResult {
    pub job_id: String,
    pub filename: String,
    pub status: AddJobStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalUseResult {
    pub paused_torrent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_reseed_retry_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressBatchKind {
    Multi,
    Bulk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressBatchContext {
    pub batch_id: String,
    pub kind: ProgressBatchKind,
    pub job_ids: Vec<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_name: Option<String>,
}

#[derive(Debug, Clone)]
pub enum DesktopEvent {
    StateChanged(Box<DesktopSnapshot>),
    DownloadPromptChanged(Option<Box<DownloadPrompt>>),
    SelectJobRequested(String),
    UpdateInstallProgress(UpdateInstallProgressEvent),
    ShellError(ShellError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
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
    fn emit_event(&self, _event: DesktopEvent) -> BackendFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn notify(&self, _title: String, _body: String) -> BackendFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn show_download_prompt_window(&self) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("show download prompt window")
    }

    fn close_download_prompt_window(&self, _remember_position: bool) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("close download prompt window")
    }

    fn focus_job_in_main_window(&self, _id: String) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("focus job in main window")
    }

    fn take_pending_selected_job_request(&self) -> BackendFuture<'_, Option<String>> {
        Box::pin(async { Ok(None) })
    }

    fn show_progress_window(
        &self,
        _id: String,
        _transfer_kind: TransferKind,
    ) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("show progress window")
    }

    fn show_batch_progress_window(&self, _batch_id: String) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("show batch progress window")
    }

    fn browse_directory(&self) -> BackendFuture<'_, Option<String>> {
        unsupported_shell_operation("browse directory")
    }

    fn browse_torrent_file(&self) -> BackendFuture<'_, Option<String>> {
        unsupported_shell_operation("browse torrent file")
    }

    fn save_diagnostics_report(&self, _report: String) -> BackendFuture<'_, Option<String>> {
        unsupported_shell_operation("save diagnostics report")
    }

    fn gather_host_registration_diagnostics(
        &self,
    ) -> BackendFuture<'_, HostRegistrationDiagnostics> {
        unsupported_shell_operation("gather host registration diagnostics")
    }

    fn sync_autostart_setting(&self, _enabled: bool) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("sync autostart setting")
    }

    fn schedule_downloads(&self, _state: SharedState) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("schedule downloads")
    }

    fn forget_torrent_session_for_restart(&self, _torrent: TorrentInfo) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("forget torrent session for restart")
    }

    fn forget_known_torrent_sessions(&self, _torrents: Vec<TorrentInfo>) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("forget known torrent sessions")
    }

    fn apply_torrent_runtime_settings(&self, _settings: TorrentSettings) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("apply torrent runtime settings")
    }

    fn probe_browser_download_access(
        &self,
        _state: SharedState,
        _source: DownloadSource,
        _url: String,
        _handoff_auth: Option<HandoffAuth>,
    ) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("probe browser download access")
    }

    fn schedule_external_reseed(&self, _state: SharedState, _id: String) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("schedule external reseed")
    }

    fn open_url(&self, _url: String) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("open URL")
    }

    fn open_path(&self, _path: String) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("open path")
    }

    fn reveal_path(&self, _path: String) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("reveal path")
    }

    fn open_install_docs(&self) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("open install docs")
    }

    fn run_host_registration_fix(&self) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("run host registration fix")
    }

    fn test_extension_handoff(
        &self,
        _state: SharedState,
        _prompts: PromptRegistry,
    ) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("test extension handoff")
    }

    fn check_for_update(&self) -> BackendFuture<'_, Option<AppUpdateMetadata>> {
        unsupported_shell_operation("check for update")
    }

    fn install_update(&self) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("install update")
    }

    fn close_to_tray(&self) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("close to tray")
    }

    fn request_exit(&self) -> BackendFuture<'_, ()> {
        unsupported_shell_operation("request exit")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateMetadata {
    pub version: String,
    pub current_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

fn unsupported_shell_operation<T>(operation: &'static str) -> BackendFuture<'static, T> {
    Box::pin(async move { Err(format!("Shell service does not support {operation}.")) })
}
