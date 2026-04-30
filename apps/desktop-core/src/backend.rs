use crate::contracts::{
    AddJobRequest, AddJobResult, AddJobStatus, AddJobsRequest, AddJobsResult, AppUpdateMetadata,
    BackendFuture, ConfirmPromptRequest, DesktopBackend, DesktopEvent, ExternalUseResult,
    ProgressBatchContext, ShellServices,
};
use crate::host_protocol::{HostRequest, HostResponse};
use crate::prompts::{PromptDecision, PromptRegistry};
use crate::state::{
    clear_torrent_session_cache_directory, validate_settings, BackendError, EnqueueOptions,
    EnqueueResult, EnqueueStatus, SharedState, TorrentSessionCacheClearResult,
};
use crate::storage::{
    DesktopSnapshot, DiagnosticLevel, DiagnosticsSnapshot, DownloadJob, DownloadPrompt, JobState,
    Settings, TransferKind,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub const EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS: u64 = 60;

#[derive(Clone)]
pub struct CoreDesktopBackend<S> {
    state: SharedState,
    prompts: PromptRegistry,
    progress_batches: ProgressBatchRegistry,
    shell: Arc<S>,
}

impl<S> CoreDesktopBackend<S>
where
    S: ShellServices + 'static,
{
    pub fn new(
        state: SharedState,
        prompts: PromptRegistry,
        progress_batches: ProgressBatchRegistry,
        shell: S,
    ) -> Self {
        Self {
            state,
            prompts,
            progress_batches,
            shell: Arc::new(shell),
        }
    }

    pub fn state(&self) -> &SharedState {
        &self.state
    }

    pub fn prompts(&self) -> &PromptRegistry {
        &self.prompts
    }

    pub async fn handle_host_request(&self, request: HostRequest) -> HostResponse {
        crate::host_protocol::handle_host_request(
            self.state.clone(),
            self.prompts.clone(),
            self.shell.as_ref(),
            request,
        )
        .await
    }

    pub async fn refresh_host_connection_diagnostics(&self) -> Result<(), String> {
        crate::host_protocol::refresh_host_connection_diagnostics(&self.state, self.shell.as_ref())
            .await
    }
}

#[derive(Clone, Default)]
pub struct ProgressBatchRegistry {
    contexts: Arc<RwLock<HashMap<String, ProgressBatchContext>>>,
}

impl ProgressBatchRegistry {
    pub fn store(&self, context: ProgressBatchContext) {
        if let Ok(mut contexts) = self.contexts.write() {
            contexts.insert(context.batch_id.clone(), context);
        }
    }

    pub fn get(&self, batch_id: &str) -> Option<ProgressBatchContext> {
        self.contexts
            .read()
            .ok()
            .and_then(|contexts| contexts.get(batch_id).cloned())
    }
}

impl From<EnqueueStatus> for AddJobStatus {
    fn from(value: EnqueueStatus) -> Self {
        match value {
            EnqueueStatus::Queued => Self::Queued,
            EnqueueStatus::DuplicateExistingJob => Self::DuplicateExistingJob,
        }
    }
}

impl From<EnqueueResult> for AddJobResult {
    fn from(result: EnqueueResult) -> Self {
        Self {
            job_id: result.job_id,
            filename: result.filename,
            status: result.status.into(),
        }
    }
}

impl<S> DesktopBackend for CoreDesktopBackend<S>
where
    S: ShellServices + 'static,
{
    fn get_app_snapshot(&self) -> BackendFuture<'_, DesktopSnapshot> {
        Box::pin(async move { Ok(self.state.snapshot().await) })
    }

    fn get_diagnostics(&self) -> BackendFuture<'_, DiagnosticsSnapshot> {
        Box::pin(async move {
            let host_registration = self.shell.gather_host_registration_diagnostics().await?;
            Ok(self.state.diagnostics_snapshot(host_registration).await)
        })
    }

    fn export_diagnostics_report(&self) -> BackendFuture<'_, Option<String>> {
        Box::pin(async move {
            let diagnostics = self.get_diagnostics().await?;
            let report = serde_json::to_string_pretty(&diagnostics)
                .map_err(|error| format!("Could not serialize diagnostics report: {error}"))?;
            self.shell.save_diagnostics_report(report).await
        })
    }

    fn add_job(&self, request: AddJobRequest) -> BackendFuture<'_, AddJobResult> {
        Box::pin(async move {
            let result = self
                .state
                .enqueue_download_with_options(
                    request.url,
                    EnqueueOptions {
                        directory_override: request.directory_override,
                        filename_hint: request.filename_hint,
                        expected_sha256: request.expected_sha256,
                        transfer_kind: request.transfer_kind,
                        ..Default::default()
                    },
                )
                .await
                .map_err(backend_error_message)?;
            self.emit_snapshot(&result.snapshot).await?;
            if result.status == EnqueueStatus::Queued {
                self.shell.schedule_downloads(self.state.clone()).await?;
            }
            Ok(result.into())
        })
    }

    fn add_jobs(&self, request: AddJobsRequest) -> BackendFuture<'_, AddJobsResult> {
        Box::pin(async move {
            let results = self
                .state
                .enqueue_downloads(request.urls, None, request.bulk_archive_name)
                .await
                .map_err(backend_error_message)?;

            if let Some(result) = results.last() {
                self.emit_snapshot(&result.snapshot).await?;
            }

            if results
                .iter()
                .any(|result| result.status == EnqueueStatus::Queued)
            {
                self.shell.schedule_downloads(self.state.clone()).await?;
            }

            let queued_count = results
                .iter()
                .filter(|result| result.status == EnqueueStatus::Queued)
                .count();
            let duplicate_count = results.len().saturating_sub(queued_count);

            Ok(AddJobsResult {
                results: results.into_iter().map(Into::into).collect(),
                queued_count,
                duplicate_count,
            })
        })
    }

    fn pause_job(&self, id: String) -> BackendFuture<'_, ()> {
        self.mutate_job_and_reschedule(move |state| async move { state.pause_job(&id).await })
    }

    fn resume_job(&self, id: String) -> BackendFuture<'_, ()> {
        self.mutate_job_and_reschedule(move |state| async move { state.resume_job(&id).await })
    }

    fn pause_all_jobs(&self) -> BackendFuture<'_, ()> {
        self.mutate_job_and_reschedule(|state| async move { state.pause_all_jobs().await })
    }

    fn resume_all_jobs(&self) -> BackendFuture<'_, ()> {
        self.mutate_job_and_reschedule(|state| async move { state.resume_all_jobs().await })
    }

    fn cancel_job(&self, id: String) -> BackendFuture<'_, ()> {
        self.mutate_job_and_reschedule(move |state| async move { state.cancel_job(&id).await })
    }

    fn retry_job(&self, id: String) -> BackendFuture<'_, ()> {
        self.mutate_job_and_reschedule(move |state| async move { state.retry_job(&id).await })
    }

    fn restart_job(&self, id: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            if let Some(torrent) = self
                .state
                .torrent_restart_cleanup_info(&id)
                .await
                .map_err(backend_error_message)?
            {
                self.shell
                    .forget_torrent_session_for_restart(torrent)
                    .await?;
            }

            let snapshot = self
                .state
                .restart_job(&id)
                .await
                .map_err(backend_error_message)?;
            self.emit_snapshot(&snapshot).await?;
            self.shell.schedule_downloads(self.state.clone()).await
        })
    }

    fn retry_failed_jobs(&self) -> BackendFuture<'_, ()> {
        self.mutate_job_and_reschedule(|state| async move { state.retry_failed_jobs().await })
    }

    fn swap_failed_download_to_browser(&self, id: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let snapshot = self.state.snapshot().await;
            let job = snapshot
                .jobs
                .iter()
                .find(|job| job.id == id)
                .ok_or_else(|| "Download was not found.".to_string())?;
            let url = failed_browser_download_url(job)?;
            self.shell.open_url(url.to_string()).await
        })
    }

    fn remove_job(&self, id: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.prepare_torrent_removal(&id).await?;
            let snapshot = self
                .state
                .remove_job(&id)
                .await
                .map_err(backend_error_message)?;
            self.emit_snapshot(&snapshot).await?;
            self.shell.schedule_downloads(self.state.clone()).await
        })
    }

    fn delete_job(&self, id: String, delete_from_disk: bool) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.prepare_torrent_removal(&id).await?;
            let snapshot = self
                .state
                .delete_job(&id, delete_from_disk)
                .await
                .map_err(backend_error_message)?;
            self.emit_snapshot(&snapshot).await?;
            self.shell.schedule_downloads(self.state.clone()).await
        })
    }

    fn rename_job(&self, id: String, filename: String) -> BackendFuture<'_, ()> {
        self.mutate_job_and_reschedule(move |state| async move {
            state.rename_job(&id, &filename).await
        })
    }

    fn clear_completed_jobs(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let snapshot = self
                .state
                .clear_completed_jobs()
                .await
                .map_err(backend_error_message)?;
            self.emit_snapshot(&snapshot).await
        })
    }

    fn save_settings(&self, mut settings: Settings) -> BackendFuture<'_, Settings> {
        Box::pin(async move {
            validate_settings(&mut settings)?;
            self.shell
                .sync_autostart_setting(settings.start_on_startup)
                .await?;
            let snapshot = self.state.save_settings(settings).await?;
            let saved_settings = snapshot.settings.clone();
            self.emit_snapshot(&snapshot).await?;
            self.shell
                .apply_torrent_runtime_settings(saved_settings.torrent.clone())
                .await?;
            self.shell.schedule_downloads(self.state.clone()).await?;
            Ok(saved_settings)
        })
    }

    fn browse_directory(&self) -> BackendFuture<'_, Option<String>> {
        Box::pin(async move { self.shell.browse_directory().await })
    }

    fn clear_torrent_session_cache(&self) -> BackendFuture<'_, TorrentSessionCacheClearResult> {
        Box::pin(async move {
            let prepared = self
                .state
                .prepare_torrent_session_cache_clear()
                .await
                .map_err(backend_error_message)?;

            if let Err(error) = self
                .shell
                .forget_known_torrent_sessions(prepared.torrents)
                .await
            {
                let _ = self
                    .state
                    .record_diagnostic_event(
                        DiagnosticLevel::Warning,
                        "torrent",
                        format!(
                            "Could not forget in-memory torrent sessions before cache cleanup: {error}"
                        ),
                        None,
                    )
                    .await;
            }

            let result = clear_torrent_session_cache_directory(&self.state.app_data_dir())?;
            self.emit_snapshot(&prepared.snapshot).await?;
            Ok(result)
        })
    }

    fn browse_torrent_file(&self) -> BackendFuture<'_, Option<String>> {
        Box::pin(async move { self.shell.browse_torrent_file().await })
    }

    fn get_current_download_prompt(&self) -> BackendFuture<'_, Option<DownloadPrompt>> {
        Box::pin(async move { Ok(self.prompts.active_prompt().await) })
    }

    fn confirm_download_prompt(&self, request: ConfirmPromptRequest) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.complete_prompt_action(
                &request.id,
                PromptDecision::Download {
                    directory_override: request.directory_override,
                    duplicate_action: request.duplicate_action,
                    renamed_filename: request.renamed_filename,
                },
            )
            .await
        })
    }

    fn show_existing_download_prompt(&self, id: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let active_prompt = self.prompts.active_prompt().await;
            let existing_job_id = active_prompt
                .as_ref()
                .and_then(|prompt| prompt.duplicate_job.as_ref())
                .map(|job| job.id.clone());

            self.complete_prompt_action(&id, PromptDecision::ShowExisting)
                .await?;

            if let Some(job_id) = existing_job_id {
                self.shell.focus_job_in_main_window(job_id).await?;
            }

            Ok(())
        })
    }

    fn swap_download_prompt(&self, id: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.complete_prompt_action(&id, PromptDecision::SwapToBrowser)
                .await
        })
    }

    fn cancel_download_prompt(&self, id: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.complete_prompt_action(&id, PromptDecision::Cancel)
                .await
        })
    }

    fn take_pending_selected_job_request(&self) -> BackendFuture<'_, Option<String>> {
        Box::pin(async move { self.shell.take_pending_selected_job_request().await })
    }

    fn open_progress_window(&self, id: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            let snapshot = self.state.snapshot().await;
            let transfer_kind = snapshot
                .jobs
                .iter()
                .find(|job| job.id == id)
                .map(|job| job.transfer_kind)
                .unwrap_or_default();
            self.shell.show_progress_window(id, transfer_kind).await
        })
    }

    fn open_batch_progress_window(
        &self,
        context: ProgressBatchContext,
    ) -> BackendFuture<'_, String> {
        Box::pin(async move {
            let batch_id = context.batch_id.clone();
            self.progress_batches.store(context);
            self.shell
                .show_batch_progress_window(batch_id.clone())
                .await?;
            Ok(batch_id)
        })
    }

    fn get_progress_batch_context(
        &self,
        batch_id: String,
    ) -> BackendFuture<'_, Option<ProgressBatchContext>> {
        Box::pin(async move { Ok(self.progress_batches.get(&batch_id)) })
    }

    fn open_job_file(&self, id: String) -> BackendFuture<'_, ExternalUseResult> {
        Box::pin(async move {
            let preparation = self
                .state
                .prepare_job_for_external_use(&id)
                .await
                .map_err(backend_error_message)?;
            if let Some(snapshot) = &preparation.snapshot {
                self.emit_snapshot(snapshot).await?;
            }

            let path = self
                .state
                .resolve_openable_path(&id)
                .await
                .map_err(backend_error_message)?;

            let auto_reseed_retry_seconds = if preparation.paused_torrent {
                self.shell
                    .schedule_external_reseed(self.state.clone(), id)
                    .await?;
                Some(EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS)
            } else {
                None
            };

            self.shell.open_path(path.display().to_string()).await?;

            Ok(ExternalUseResult {
                paused_torrent: preparation.paused_torrent,
                auto_reseed_retry_seconds,
            })
        })
    }

    fn reveal_job_in_folder(&self, id: String) -> BackendFuture<'_, ExternalUseResult> {
        Box::pin(async move {
            let preparation = self
                .state
                .prepare_job_for_external_use(&id)
                .await
                .map_err(backend_error_message)?;
            if let Some(snapshot) = &preparation.snapshot {
                self.emit_snapshot(snapshot).await?;
            }

            let path = self
                .state
                .resolve_revealable_path(&id)
                .await
                .map_err(backend_error_message)?;

            let auto_reseed_retry_seconds = if preparation.paused_torrent {
                self.shell
                    .schedule_external_reseed(self.state.clone(), id)
                    .await?;
                Some(EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS)
            } else {
                None
            };

            self.shell.reveal_path(path.display().to_string()).await?;

            Ok(ExternalUseResult {
                paused_torrent: preparation.paused_torrent,
                auto_reseed_retry_seconds,
            })
        })
    }

    fn open_install_docs(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move { self.shell.open_install_docs().await })
    }

    fn run_host_registration_fix(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move { self.shell.run_host_registration_fix().await })
    }

    fn test_extension_handoff(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.shell
                .test_extension_handoff(self.state.clone(), self.prompts.clone())
                .await
        })
    }

    fn check_for_update(&self) -> BackendFuture<'_, Option<AppUpdateMetadata>> {
        Box::pin(async move { self.shell.check_for_update().await })
    }

    fn install_update(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move { self.shell.install_update().await })
    }
}

impl<S> CoreDesktopBackend<S>
where
    S: ShellServices + 'static,
{
    fn mutate_job_and_reschedule<F, Fut>(&self, mutation: F) -> BackendFuture<'_, ()>
    where
        F: FnOnce(SharedState) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<DesktopSnapshot, BackendError>> + Send + 'static,
    {
        let state = self.state.clone();
        let shell = self.shell.clone();
        Box::pin(async move {
            let snapshot = mutation(state.clone())
                .await
                .map_err(backend_error_message)?;
            shell
                .emit_event(DesktopEvent::StateChanged(Box::new(snapshot)))
                .await?;
            shell.schedule_downloads(state).await
        })
    }

    async fn emit_snapshot(&self, snapshot: &DesktopSnapshot) -> Result<(), String> {
        self.shell
            .emit_event(DesktopEvent::StateChanged(Box::new(snapshot.clone())))
            .await
    }

    async fn complete_prompt_action(
        &self,
        id: &str,
        decision: PromptDecision,
    ) -> Result<(), String> {
        let remember_prompt_position = matches!(&decision, PromptDecision::Download { .. });
        let next_prompt = self.prompts.resolve(id, decision).await?;
        if let Some(prompt) = next_prompt {
            self.shell.show_download_prompt_window().await?;
            self.shell
                .emit_event(DesktopEvent::DownloadPromptChanged(Some(Box::new(prompt))))
                .await
        } else {
            self.shell
                .close_download_prompt_window(remember_prompt_position)
                .await?;
            self.shell
                .emit_event(DesktopEvent::DownloadPromptChanged(None))
                .await
        }
    }

    async fn prepare_torrent_removal(&self, id: &str) -> Result<(), String> {
        let Some(cleanup) = self
            .state
            .torrent_removal_cleanup_info(id)
            .await
            .map_err(backend_error_message)?
        else {
            return Ok(());
        };

        if cleanup.wait_for_worker_release {
            self.state
                .wait_for_torrent_removal_release(id)
                .await
                .map_err(backend_error_message)?;
        }

        self.shell
            .forget_torrent_session_for_restart(cleanup.torrent)
            .await
    }
}

fn backend_error_message(error: BackendError) -> String {
    error.message
}

fn failed_browser_download_url(job: &DownloadJob) -> Result<&str, String> {
    if job.state != JobState::Failed
        || job.transfer_kind != TransferKind::Http
        || job
            .source
            .as_ref()
            .map(|source| source.entry_point.as_str())
            != Some("browser_download")
    {
        return Err("Only failed browser downloads can be swapped back to the browser.".into());
    }

    let parsed = url::Url::parse(&job.url)
        .map_err(|_| "The download URL is not valid for browser swap.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only http and https downloads can be swapped back to the browser.".into());
    }

    Ok(job.url.as_str())
}

pub use crate::host_protocol::prompt_enqueue_details;
