use crate::download::{
    apply_torrent_runtime_settings, forget_torrent_session_for_restart, schedule_downloads,
    schedule_external_reseed, EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS,
};
use crate::ipc::gather_host_registration_diagnostics;
use crate::lifecycle::sync_autostart_setting;
use crate::prompts::{PromptDecision, PromptDuplicateAction, PromptRegistry, PROMPT_CHANGED_EVENT};
use crate::state::{validate_settings, DuplicatePolicy, EnqueueResult, EnqueueStatus, SharedState};
use crate::storage::{
    DesktopSnapshot, DiagnosticsSnapshot, DownloadJob, DownloadPrompt, DownloadSource,
    HostRegistrationStatus, JobState, Settings, TransferKind,
};
use crate::windows::{
    close_download_prompt_window, focus_job_in_main_window, show_batch_progress_window,
    show_download_prompt_window, show_progress_window, DOWNLOAD_PROMPT_WINDOW,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tauri::{AppHandle, Emitter, State};

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(windows)]
use serde_json::json;

#[cfg(windows)]
use windows_sys::Win32::UI::Shell::ShellExecuteW;

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[cfg(windows)]
use winreg::enums::HKEY_CURRENT_USER;

#[cfg(windows)]
use winreg::RegKey;

pub const STATE_CHANGED_EVENT: &str = "app://state-changed";
const INSTALL_RESOURCE_DIR: &str = "resources\\install";
const NATIVE_HOST_NAME: &str = "com.myapp.download_manager";
const DEFAULT_CHROMIUM_EXTENSION_ID: &str = "pkaojpfpjieklhinoibjibmjldohlmbb";
const DEFAULT_FIREFOX_EXTENSION_ID: &str = "simple-download-manager@example.com";

#[cfg(windows)]
const CHROME_REGISTRY_PATH: &str =
    r"Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager";
#[cfg(windows)]
const EDGE_REGISTRY_PATH: &str =
    r"Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager";
#[cfg(windows)]
const FIREFOX_REGISTRY_PATH: &str =
    r"Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseMetadata {
    sidecar_binary_name: Option<String>,
    chromium_extension_id: Option<String>,
    edge_extension_id: Option<String>,
    firefox_extension_id: Option<String>,
}

impl Default for ReleaseMetadata {
    fn default() -> Self {
        Self {
            sidecar_binary_name: None,
            chromium_extension_id: Some(DEFAULT_CHROMIUM_EXTENSION_ID.into()),
            edge_extension_id: Some(DEFAULT_CHROMIUM_EXTENSION_ID.into()),
            firefox_extension_id: Some(DEFAULT_FIREFOX_EXTENSION_ID.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddJobResult {
    pub job_id: String,
    pub filename: String,
    pub status: String,
}

impl From<EnqueueResult> for AddJobResult {
    fn from(result: EnqueueResult) -> Self {
        Self {
            job_id: result.job_id,
            filename: result.filename,
            status: result.status.as_protocol_value().into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddJobsResult {
    pub results: Vec<AddJobResult>,
    pub queued_count: usize,
    pub duplicate_count: usize,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Default)]
pub struct ProgressBatchRegistry {
    contexts: RwLock<HashMap<String, ProgressBatchContext>>,
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

pub fn emit_snapshot(app: &AppHandle, snapshot: &DesktopSnapshot) {
    if let Err(error) = app.emit(STATE_CHANGED_EVENT, snapshot.clone()) {
        eprintln!("failed to emit state snapshot: {error}");
    }
}

async fn complete_prompt_action(
    app: &AppHandle,
    prompts: PromptRegistry,
    id: &str,
    decision: PromptDecision,
) -> Result<(), String> {
    let remember_prompt_position = matches!(&decision, PromptDecision::Download { .. });
    let next_prompt = prompts.resolve(id, decision).await?;
    if let Some(prompt) = next_prompt {
        show_download_prompt_window(app)?;
        app.emit_to(DOWNLOAD_PROMPT_WINDOW, PROMPT_CHANGED_EVENT, prompt)
            .map_err(|error| error.to_string())?;
    } else {
        close_download_prompt_window(app, remember_prompt_position);
    }
    Ok(())
}

#[tauri::command]
pub async fn get_app_snapshot(state: State<'_, SharedState>) -> Result<DesktopSnapshot, String> {
    Ok(state.snapshot().await)
}

#[tauri::command]
pub async fn get_diagnostics(state: State<'_, SharedState>) -> Result<DiagnosticsSnapshot, String> {
    let host_registration = gather_host_registration_diagnostics()?;
    Ok(state.diagnostics_snapshot(host_registration).await)
}

#[tauri::command]
pub async fn export_diagnostics_report(
    state: State<'_, SharedState>,
) -> Result<Option<String>, String> {
    let host_registration = gather_host_registration_diagnostics()?;
    let diagnostics = state.diagnostics_snapshot(host_registration).await;
    let report = serde_json::to_string_pretty(&diagnostics)
        .map_err(|error| format!("Could not serialize diagnostics report: {error}"))?;

    let path = tauri::async_runtime::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_file_name("simple-download-manager-diagnostics.json")
            .save_file()
    })
    .await
    .map_err(|error| format!("Could not open save dialog: {error}"))?;

    let Some(path) = path else {
        return Ok(None);
    };

    std::fs::write(&path, report)
        .map_err(|error| format!("Could not write diagnostics report: {error}"))?;

    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub async fn add_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    url: String,
    expected_sha256: Option<String>,
    transfer_kind: Option<TransferKind>,
) -> Result<AddJobResult, String> {
    let result = state
        .enqueue_download_with_options(
            url,
            crate::state::EnqueueOptions {
                expected_sha256,
                transfer_kind,
                ..Default::default()
            },
        )
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &result.snapshot);
    if result.status == EnqueueStatus::Queued {
        schedule_downloads(app, state.inner().clone());
    }

    Ok(result.into())
}

#[tauri::command]
pub async fn add_jobs(
    app: AppHandle,
    state: State<'_, SharedState>,
    urls: Vec<String>,
    bulk_archive_name: Option<String>,
) -> Result<AddJobsResult, String> {
    let results = state
        .enqueue_downloads(urls, None, bulk_archive_name)
        .await
        .map_err(|error| error.message)?;

    if let Some(result) = results.last() {
        emit_snapshot(&app, &result.snapshot);
    }

    if results
        .iter()
        .any(|result| result.status == EnqueueStatus::Queued)
    {
        schedule_downloads(app, state.inner().clone());
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
}

#[tauri::command]
pub async fn pause_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    let snapshot = state.pause_job(&id).await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn resume_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    let snapshot = state.resume_job(&id).await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn pause_all_jobs(app: AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let snapshot = state
        .pause_all_jobs()
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn resume_all_jobs(app: AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let snapshot = state
        .resume_all_jobs()
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn cancel_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    let snapshot = state.cancel_job(&id).await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn retry_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    let snapshot = state.retry_job(&id).await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn restart_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    if let Some(torrent) = state
        .torrent_restart_cleanup_info(&id)
        .await
        .map_err(|error| error.message)?
    {
        forget_torrent_session_for_restart(state.inner(), &torrent).await?;
    }

    let snapshot = state
        .restart_job(&id)
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn retry_failed_jobs(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let snapshot = state
        .retry_failed_jobs()
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn swap_failed_download_to_browser(
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    let snapshot = state.snapshot().await;
    let job = snapshot
        .jobs
        .iter()
        .find(|job| job.id == id)
        .ok_or_else(|| "Download was not found.".to_string())?;
    let url = failed_browser_download_url(job)?;
    open_url(url)
}

#[tauri::command]
pub async fn remove_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    prepare_torrent_removal(&state, &id).await?;
    let snapshot = state.remove_job(&id).await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn delete_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
    delete_from_disk: bool,
) -> Result<(), String> {
    prepare_torrent_removal(&state, &id).await?;
    let snapshot = state
        .delete_job(&id, delete_from_disk)
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

async fn prepare_torrent_removal(
    state: &State<'_, SharedState>,
    id: &str,
) -> Result<(), String> {
    let Some(cleanup) = state
        .torrent_removal_cleanup_info(id)
        .await
        .map_err(|error| error.message)?
    else {
        return Ok(());
    };

    if cleanup.wait_for_worker_release {
        state
            .wait_for_torrent_removal_release(id)
            .await
            .map_err(|error| error.message)?;
    }

    forget_torrent_session_for_restart(state.inner(), &cleanup.torrent).await
}

#[tauri::command]
pub async fn rename_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
    filename: String,
) -> Result<(), String> {
    let snapshot = state
        .rename_job(&id, &filename)
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn clear_completed_jobs(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let snapshot = state
        .clear_completed_jobs()
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    Ok(())
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, SharedState>,
    mut settings: Settings,
) -> Result<Settings, String> {
    validate_settings(&mut settings)?;
    sync_autostart_setting(settings.start_on_startup)?;
    let snapshot = state.save_settings(settings).await?;
    let saved_settings = snapshot.settings.clone();
    emit_snapshot(&app, &snapshot);
    apply_torrent_runtime_settings(&saved_settings.torrent);
    schedule_downloads(app, state.inner().clone());
    Ok(saved_settings)
}

#[tauri::command]
pub async fn browse_directory() -> Result<Option<String>, String> {
    let selected = tauri::async_runtime::spawn_blocking(|| rfd::FileDialog::new().pick_folder())
        .await
        .map_err(|error| format!("Could not open folder picker: {error}"))?;

    Ok(selected.map(|path| path.display().to_string()))
}

#[tauri::command]
pub async fn browse_torrent_file() -> Result<Option<String>, String> {
    let selected = tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .add_filter("Torrent or magnet", &["torrent", "magnet", "txt"])
            .pick_file()
    })
    .await
    .map_err(|error| format!("Could not open torrent picker: {error}"))?;

    selected
        .as_deref()
        .map(torrent_import_value_from_path)
        .transpose()
}

#[tauri::command]
pub async fn get_current_download_prompt(
    prompts: State<'_, PromptRegistry>,
) -> Result<Option<DownloadPrompt>, String> {
    Ok(prompts.active_prompt().await)
}

#[tauri::command]
pub async fn confirm_download_prompt(
    app: AppHandle,
    prompts: State<'_, PromptRegistry>,
    id: String,
    directory_override: Option<String>,
    allow_duplicate: Option<bool>,
    duplicate_action: Option<PromptDuplicateAction>,
    renamed_filename: Option<String>,
) -> Result<(), String> {
    let duplicate_action = duplicate_action.unwrap_or_else(|| {
        if allow_duplicate.unwrap_or(false) {
            PromptDuplicateAction::DownloadAnyway
        } else {
            PromptDuplicateAction::ReturnExisting
        }
    });
    complete_prompt_action(
        &app,
        prompts.inner().clone(),
        &id,
        PromptDecision::Download {
            directory_override,
            duplicate_action,
            renamed_filename,
        },
    )
    .await
}

fn prompt_enqueue_details(
    default_filename: String,
    duplicate_action: PromptDuplicateAction,
    renamed_filename: Option<String>,
) -> Result<(String, DuplicatePolicy), String> {
    match duplicate_action {
        PromptDuplicateAction::ReturnExisting => {
            Ok((default_filename, DuplicatePolicy::ReturnExisting))
        }
        PromptDuplicateAction::DownloadAnyway => Ok((default_filename, DuplicatePolicy::Allow)),
        PromptDuplicateAction::Overwrite => {
            Ok((default_filename, DuplicatePolicy::ReplaceExisting))
        }
        PromptDuplicateAction::Rename => {
            let filename = renamed_filename.unwrap_or_default();
            if filename.trim().is_empty() {
                return Err("Filename cannot be empty.".into());
            }
            Ok((filename, DuplicatePolicy::Allow))
        }
    }
}

#[tauri::command]
pub async fn show_existing_download_prompt(
    app: AppHandle,
    prompts: State<'_, PromptRegistry>,
    id: String,
) -> Result<(), String> {
    let active_prompt = prompts.active_prompt().await;
    let existing_job_id = active_prompt
        .as_ref()
        .and_then(|prompt| prompt.duplicate_job.as_ref())
        .map(|job| job.id.clone());

    complete_prompt_action(
        &app,
        prompts.inner().clone(),
        &id,
        PromptDecision::ShowExisting,
    )
    .await?;

    if let Some(job_id) = existing_job_id {
        focus_job_in_main_window(&app, &job_id);
    }

    Ok(())
}

#[tauri::command]
pub async fn swap_download_prompt(
    app: AppHandle,
    prompts: State<'_, PromptRegistry>,
    id: String,
) -> Result<(), String> {
    complete_prompt_action(
        &app,
        prompts.inner().clone(),
        &id,
        PromptDecision::SwapToBrowser,
    )
    .await
}

#[tauri::command]
pub async fn cancel_download_prompt(
    app: AppHandle,
    prompts: State<'_, PromptRegistry>,
    id: String,
) -> Result<(), String> {
    complete_prompt_action(&app, prompts.inner().clone(), &id, PromptDecision::Cancel).await
}

#[tauri::command]
pub async fn open_progress_window(app: AppHandle, id: String) -> Result<(), String> {
    show_progress_window(&app, &id)
}

#[tauri::command]
pub async fn open_batch_progress_window(
    app: AppHandle,
    registry: State<'_, ProgressBatchRegistry>,
    context: ProgressBatchContext,
) -> Result<(), String> {
    let batch_id = context.batch_id.clone();
    registry.store(context);
    show_batch_progress_window(&app, &batch_id)
}

#[tauri::command]
pub async fn get_progress_batch_context(
    registry: State<'_, ProgressBatchRegistry>,
    batch_id: String,
) -> Result<Option<ProgressBatchContext>, String> {
    Ok(registry.get(&batch_id))
}

#[tauri::command]
pub async fn open_job_file(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<ExternalUseResult, String> {
    let preparation = state
        .prepare_job_for_external_use(&id)
        .await
        .map_err(|error| error.message)?;
    if let Some(snapshot) = &preparation.snapshot {
        emit_snapshot(&app, snapshot);
    }

    let path = state
        .resolve_openable_path(&id)
        .await
        .map_err(|error| error.message)?;

    let auto_reseed_retry_seconds = if preparation.paused_torrent {
        schedule_external_reseed(app.clone(), state.inner().clone(), id.clone()).await;
        Some(EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS)
    } else {
        None
    };

    tauri::async_runtime::spawn_blocking(move || open_path(&path))
        .await
        .map_err(|error| format!("Could not open file: {error}"))??;

    Ok(ExternalUseResult {
        paused_torrent: preparation.paused_torrent,
        auto_reseed_retry_seconds,
    })
}

#[tauri::command]
pub async fn reveal_job_in_folder(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<ExternalUseResult, String> {
    let preparation = state
        .prepare_job_for_external_use(&id)
        .await
        .map_err(|error| error.message)?;
    if let Some(snapshot) = &preparation.snapshot {
        emit_snapshot(&app, snapshot);
    }

    let path = state
        .resolve_revealable_path(&id)
        .await
        .map_err(|error| error.message)?;

    let auto_reseed_retry_seconds = if preparation.paused_torrent {
        schedule_external_reseed(app.clone(), state.inner().clone(), id.clone()).await;
        Some(EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS)
    } else {
        None
    };

    tauri::async_runtime::spawn_blocking(move || reveal_path(&path))
        .await
        .map_err(|error| format!("Could not reveal file: {error}"))??;

    Ok(ExternalUseResult {
        paused_torrent: preparation.paused_torrent,
        auto_reseed_retry_seconds,
    })
}

#[tauri::command]
pub async fn open_install_docs() -> Result<(), String> {
    let docs_path = resolve_install_resource_path("install.md")?;

    tauri::async_runtime::spawn_blocking(move || open_path(&docs_path))
        .await
        .map_err(|error| format!("Could not open install docs: {error}"))??;

    Ok(())
}

#[tauri::command]
pub async fn run_host_registration_fix() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(register_native_host)
        .await
        .map_err(|error| format!("Could not start host registration: {error}"))??;

    Ok(())
}

pub fn initialize_native_host_registration() {
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn(async move {
            let result = tauri::async_runtime::spawn_blocking(ensure_native_host_registration)
                .await
                .map_err(|error| format!("Could not start native host registration: {error}"))
                .and_then(|result| result);

            if let Err(error) = result {
                eprintln!("native host auto-registration failed: {error}");
            }
        });
    }
}

#[tauri::command]
pub async fn test_extension_handoff(
    app: AppHandle,
    state: State<'_, SharedState>,
    prompts: State<'_, PromptRegistry>,
) -> Result<(), String> {
    let request_id = format!("test_handoff_{}", unix_timestamp_millis());
    let prompt = state
        .prepare_download_prompt(
            request_id,
            "https://example.com/simple-download-manager-test.bin",
            Some(DownloadSource {
                entry_point: "browser_download".into(),
                browser: "chrome".into(),
                extension_version: "settings-test".into(),
                page_url: Some("https://example.com/downloads".into()),
                page_title: Some("Simple Download Manager handoff test".into()),
                referrer: Some("https://example.com/downloads".into()),
                incognito: Some(false),
            }),
            Some("simple-download-manager-test.bin".into()),
            Some(1_048_576),
        )
        .await
        .map_err(|error| error.message)?;

    let receiver = prompts.enqueue(prompt.clone()).await;
    show_download_prompt_window(&app)?;
    if let Some(active_prompt) = prompts.active_prompt().await {
        app.emit_to(DOWNLOAD_PROMPT_WINDOW, PROMPT_CHANGED_EVENT, active_prompt)
            .map_err(|error| error.to_string())?;
    }

    let worker_app = app.clone();
    let worker_state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let decision = receiver.await.unwrap_or(PromptDecision::SwapToBrowser);
        match decision {
            PromptDecision::Cancel => {}
            PromptDecision::SwapToBrowser => {}
            PromptDecision::ShowExisting => {
                if let Some(job) = prompt.duplicate_job {
                    focus_job_in_main_window(&worker_app, &job.id);
                }
            }
            PromptDecision::Download {
                directory_override,
                duplicate_action,
                renamed_filename,
            } => {
                let (filename_hint, duplicate_policy) = match prompt_enqueue_details(
                    prompt.filename.clone(),
                    duplicate_action,
                    renamed_filename,
                ) {
                    Ok(details) => details,
                    Err(message) => {
                        eprintln!("test extension handoff failed: {message}");
                        return;
                    }
                };
                let result = worker_state
                    .enqueue_download_with_options(
                        prompt.url,
                        crate::state::EnqueueOptions {
                            source: prompt.source,
                            directory_override,
                            filename_hint: Some(filename_hint),
                            duplicate_policy,
                            ..Default::default()
                        },
                    )
                    .await;

                match result {
                    Ok(result) => {
                        let show_progress = worker_state.show_progress_after_handoff().await;
                        emit_snapshot(&worker_app, &result.snapshot);
                        if result.status == EnqueueStatus::Queued {
                            if show_progress {
                                let _ = show_progress_window(&worker_app, &result.job_id);
                            }
                            schedule_downloads(worker_app, worker_state);
                        }
                    }
                    Err(error) => {
                        eprintln!("test extension handoff failed: {}", error.message);
                    }
                }
            }
        }
    });

    Ok(())
}

fn unix_timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
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

#[cfg(windows)]
fn open_url(url: &str) -> Result<(), String> {
    shell_execute(OsStr::new("open"), OsStr::new(url), None)
}

#[cfg(not(windows))]
fn open_url(_url: &str) -> Result<(), String> {
    Err("Opening downloads in the browser is only supported on Windows in this build.".into())
}

#[cfg(windows)]
fn open_path(path: &Path) -> Result<(), String> {
    shell_execute(OsStr::new("open"), path.as_os_str(), None)
}

#[cfg(not(windows))]
fn open_path(_path: &Path) -> Result<(), String> {
    Err("Opening files is only supported on Windows in this build.".into())
}

#[cfg(windows)]
fn reveal_path(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        return open_path(path);
    }

    let arguments = format!("/select,\"{}\"", path.display());
    shell_execute(
        OsStr::new("open"),
        OsStr::new("explorer.exe"),
        Some(OsStr::new(&arguments)),
    )
}

#[cfg(not(windows))]
fn reveal_path(_path: &Path) -> Result<(), String> {
    Err("Revealing files is only supported on Windows in this build.".into())
}

#[cfg(windows)]
fn shell_execute(
    operation: &OsStr,
    file: &OsStr,
    parameters: Option<&OsStr>,
) -> Result<(), String> {
    let operation = wide_null(operation);
    let file = wide_null(file);
    let parameters = parameters.map(wide_null);
    let parameters_ptr = parameters
        .as_ref()
        .map(|value| value.as_ptr())
        .unwrap_or(std::ptr::null());

    // ShellExecuteW opens files and folders without showing a console window.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            parameters_ptr,
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;

    if result <= 32 {
        return Err(format!("ShellExecuteW failed with code {result}."));
    }

    Ok(())
}

#[cfg(windows)]
fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn current_install_root() -> Result<PathBuf, String> {
    let current_exe =
        std::env::current_exe().map_err(|error| format!("Could not locate app binary: {error}"))?;
    current_exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Could not resolve app install directory.".to_string())
}

fn resolve_install_resource_path(file_name: &str) -> Result<PathBuf, String> {
    let install_root = current_install_root()?;
    let bundled_candidate = install_root.join(INSTALL_RESOURCE_DIR).join(file_name);
    if bundled_candidate.exists() {
        return Ok(bundled_candidate);
    }

    for ancestor in install_root.ancestors() {
        for relative_root in [
            "src-tauri\\resources\\install",
            "apps\\desktop\\src-tauri\\resources\\install",
        ] {
            let candidate = ancestor.join(relative_root).join(file_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Err(format!(
        "Could not find bundled install resource: {file_name}."
    ))
}

fn resolve_host_binary_path() -> Result<PathBuf, String> {
    let install_root = current_install_root()?;
    let mut candidate_names = Vec::new();

    let metadata = resolve_release_metadata();
    if let Some(sidecar_binary_name) = metadata.sidecar_binary_name {
        candidate_names.push(sidecar_binary_name);
    }

    candidate_names.push("simple-download-manager-native-host.exe".into());
    candidate_names.push("simple-download-manager-native-host-x86_64-pc-windows-msvc.exe".into());

    for candidate_name in candidate_names {
        let candidate_path = install_root.join(&candidate_name);
        if candidate_path.exists() {
            return Ok(candidate_path);
        }
    }

    Err("The bundled native host executable was not found beside the desktop app.".into())
}

#[cfg(windows)]
fn ensure_native_host_registration() -> Result<(), String> {
    let diagnostics = gather_host_registration_diagnostics()?;
    if should_register_native_host(diagnostics.status) {
        register_native_host()?;
    }

    Ok(())
}

#[cfg(windows)]
fn register_native_host() -> Result<(), String> {
    let install_root = current_install_root()?;
    let host_binary_path = resolve_host_binary_path()?;
    let manifest_root = install_root.join("native-messaging");
    let metadata = resolve_release_metadata();
    let chromium_extension_id = metadata
        .chromium_extension_id
        .as_deref()
        .unwrap_or(DEFAULT_CHROMIUM_EXTENSION_ID);
    let edge_extension_id = metadata
        .edge_extension_id
        .as_deref()
        .unwrap_or(chromium_extension_id);
    let firefox_extension_id = metadata
        .firefox_extension_id
        .as_deref()
        .unwrap_or(DEFAULT_FIREFOX_EXTENSION_ID);

    std::fs::create_dir_all(&manifest_root).map_err(|error| {
        format!("Could not create native messaging manifest directory: {error}")
    })?;

    let chrome_manifest_path = manifest_root.join(format!("{NATIVE_HOST_NAME}.chrome.json"));
    let edge_manifest_path = manifest_root.join(format!("{NATIVE_HOST_NAME}.edge.json"));
    let firefox_manifest_path = manifest_root.join(format!("{NATIVE_HOST_NAME}.firefox.json"));

    write_native_host_manifest(
        &chrome_manifest_path,
        native_host_manifest_json(
            &host_binary_path,
            "allowed_origins",
            json!([format!("chrome-extension://{chromium_extension_id}/")]),
        ),
    )?;
    write_native_host_manifest(
        &edge_manifest_path,
        native_host_manifest_json(
            &host_binary_path,
            "allowed_origins",
            json!([format!("chrome-extension://{edge_extension_id}/")]),
        ),
    )?;
    write_native_host_manifest(
        &firefox_manifest_path,
        native_host_manifest_json(
            &host_binary_path,
            "allowed_extensions",
            json!([firefox_extension_id]),
        ),
    )?;

    set_registry_default_value(CHROME_REGISTRY_PATH, &chrome_manifest_path)?;
    set_registry_default_value(EDGE_REGISTRY_PATH, &edge_manifest_path)?;
    set_registry_default_value(FIREFOX_REGISTRY_PATH, &firefox_manifest_path)?;

    Ok(())
}

#[cfg(not(windows))]
fn register_native_host() -> Result<(), String> {
    Err("Native host registration is only supported on Windows in this build.".into())
}

fn resolve_release_metadata() -> ReleaseMetadata {
    resolve_install_resource_path("release.json")
        .ok()
        .and_then(|release_path| std::fs::read_to_string(release_path).ok())
        .and_then(|content| serde_json::from_str::<ReleaseMetadata>(&content).ok())
        .unwrap_or_default()
}

#[cfg(windows)]
fn native_host_manifest_json(
    host_binary_path: &Path,
    browser_key: &str,
    browser_value: serde_json::Value,
) -> serde_json::Value {
    let mut manifest = json!({
        "name": NATIVE_HOST_NAME,
        "description": "Simple Download Manager native messaging host",
        "path": host_binary_path.display().to_string(),
        "type": "stdio",
    });

    if let Some(object) = manifest.as_object_mut() {
        object.insert(browser_key.into(), browser_value);
    }

    manifest
}

#[cfg(windows)]
fn write_native_host_manifest(path: &Path, manifest: serde_json::Value) -> Result<(), String> {
    let content = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("Could not serialize native host manifest: {error}"))?;

    std::fs::write(path, content)
        .map_err(|error| format!("Could not write native host manifest: {error}"))
}

#[cfg(windows)]
fn set_registry_default_value(registry_path: &str, manifest_path: &Path) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(registry_path)
        .map_err(|error| format!("Could not create HKCU\\{registry_path}: {error}"))?;

    key.set_value("", &manifest_path.display().to_string())
        .map_err(|error| format!("Could not write HKCU\\{registry_path}: {error}"))
}

fn should_register_native_host(status: HostRegistrationStatus) -> bool {
    !matches!(status, HostRegistrationStatus::Configured)
}

fn torrent_import_value_from_path(path: &Path) -> Result<String, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "torrent" => Ok(path.display().to_string()),
        "magnet" | "txt" => {
            let content = std::fs::read_to_string(path)
                .map_err(|error| format!("Could not read torrent import file: {error}"))?;
            torrent_import_value_from_text(&content)
        }
        _ => Err("Choose a .torrent file or a text file containing a magnet link.".into()),
    }
}

fn torrent_import_value_from_text(content: &str) -> Result<String, String> {
    let value = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| "The selected import file is empty.".to_string())?;

    if value.starts_with("magnet:?")
        || (value.starts_with("https://") || value.starts_with("http://"))
            && value.to_ascii_lowercase().contains(".torrent")
    {
        Ok(value.to_string())
    } else {
        Err("The selected import file must contain a magnet link or HTTP(S) .torrent URL.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::should_register_native_host;
    use crate::storage::{
        DownloadJob, DownloadSource, HostRegistrationStatus, JobState, ResumeSupport, TransferKind,
    };
    #[cfg(windows)]
    use std::path::Path;

    #[test]
    fn native_host_registration_runs_for_missing_or_broken_entries() {
        assert!(!should_register_native_host(
            HostRegistrationStatus::Configured
        ));
        assert!(should_register_native_host(HostRegistrationStatus::Missing));
        assert!(should_register_native_host(HostRegistrationStatus::Broken));
    }

    #[test]
    fn progress_batch_registry_stores_and_retrieves_context_by_batch_id() {
        let registry = super::ProgressBatchRegistry::default();
        let context = super::ProgressBatchContext {
            batch_id: "batch_123".into(),
            kind: super::ProgressBatchKind::Multi,
            job_ids: vec!["job_1".into(), "job_2".into()],
            title: "Multi-download progress".into(),
            archive_name: None,
        };

        registry.store(context.clone());

        assert_eq!(registry.get("batch_123"), Some(context));
        assert_eq!(registry.get("missing"), None);
    }

    #[test]
    fn external_use_commands_schedule_auto_reseed() {
        let source = include_str!("mod.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("commands source should contain production code");

        assert!(production_source.contains("pub auto_reseed_retry_seconds: Option<u64>"));
        assert_eq!(
            production_source
                .matches("schedule_external_reseed(app.clone(), state.inner().clone(), id.clone()).await")
                .count(),
            2,
            "open file and open folder should both schedule auto-reseed after pausing a torrent"
        );
        assert!(production_source.contains("Some(EXTERNAL_USE_AUTO_RESEED_RETRY_SECONDS)"));
    }

    #[test]
    fn save_settings_applies_torrent_runtime_settings_before_rescheduling() {
        let source = include_str!("mod.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("commands source should contain production code");
        let save_settings = production_source
            .find("pub async fn save_settings(")
            .expect("save_settings command should exist");
        let runtime_apply = production_source[save_settings..]
            .find("apply_torrent_runtime_settings(&saved_settings.torrent);")
            .expect("save_settings should apply torrent runtime settings")
            + save_settings;
        let schedule = production_source[save_settings..]
            .find("schedule_downloads(app, state.inner().clone());")
            .expect("save_settings should reschedule downloads")
            + save_settings;

        assert!(
            runtime_apply < schedule,
            "torrent upload limits should be applied to the live session before downloads are rescheduled"
        );
    }

    #[test]
    fn torrent_remove_and_delete_prepare_engine_cleanup_before_state_mutation() {
        let source = include_str!("mod.rs");
        let production_source = source
            .split("#[cfg(test)]")
            .next()
            .expect("commands source should contain production code");

        for function_name in ["remove_job", "delete_job"] {
            let function_start = production_source
                .find(&format!("pub async fn {function_name}("))
                .unwrap_or_else(|| panic!("{function_name} command should exist"));
            let function_body = &production_source[function_start..];
            let cleanup = function_body
                .find("prepare_torrent_removal(&state, &id).await?;")
                .unwrap_or_else(|| panic!("{function_name} should prepare torrent engine cleanup"));
            let state_mutation = function_body
                .find(&format!(".{function_name}(&id"))
                .unwrap_or_else(|| panic!("{function_name} should call state.{function_name}"));

            assert!(
                cleanup < state_mutation,
                "{function_name} should forget rqbit session metadata before removing persisted job state"
            );
        }
    }

    #[test]
    fn failed_browser_download_url_accepts_only_failed_browser_http_jobs() {
        let mut job = failed_browser_download_job();

        assert_eq!(
            super::failed_browser_download_url(&job).unwrap(),
            "https://example.com/file.zip"
        );

        job.state = JobState::Downloading;
        assert!(super::failed_browser_download_url(&job).is_err());

        job = failed_browser_download_job();
        job.source = None;
        assert!(super::failed_browser_download_url(&job).is_err());

        job = failed_browser_download_job();
        job.url = "magnet:?xt=urn:btih:example".into();
        job.transfer_kind = TransferKind::Torrent;
        assert!(super::failed_browser_download_url(&job).is_err());
    }

    #[test]
    fn torrent_import_value_accepts_torrent_files() {
        let dir = test_runtime_dir("torrent-import");
        let path = dir.join("sample.torrent");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"d4:infode").unwrap();

        assert_eq!(
            super::torrent_import_value_from_path(&path).unwrap(),
            path.display().to_string()
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn torrent_import_value_reads_magnet_files() {
        let dir = test_runtime_dir("magnet-import");
        let path = dir.join("sample.magnet");
        let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Sample";
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, format!("  {magnet}\n")).unwrap();

        assert_eq!(
            super::torrent_import_value_from_path(&path).unwrap(),
            magnet
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    fn failed_browser_download_job() -> DownloadJob {
        DownloadJob {
            id: "job_1".into(),
            url: "https://example.com/file.zip".into(),
            filename: "file.zip".into(),
            source: Some(DownloadSource {
                entry_point: "browser_download".into(),
                browser: "chrome".into(),
                extension_version: "0.3.51".into(),
                page_url: None,
                page_title: None,
                referrer: None,
                incognito: Some(false),
            }),
            transfer_kind: TransferKind::Http,
            integrity_check: None,
            torrent: None,
            state: JobState::Failed,
            created_at: 0,
            progress: 25.0,
            total_bytes: 100,
            downloaded_bytes: 25,
            speed: 0,
            eta: 0,
            error: Some("network error".into()),
            failure_category: None,
            resume_support: ResumeSupport::Supported,
            retry_attempts: 1,
            target_path: "C:/Downloads/file.zip".into(),
            temp_path: "C:/Downloads/file.zip.part".into(),
            artifact_exists: None,
            bulk_archive: None,
        }
    }

    fn test_runtime_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::current_dir()
            .unwrap()
            .join("test-runtime")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(windows)]
    #[test]
    fn native_host_manifest_uses_native_path_and_browser_allowlist() {
        let manifest = super::native_host_manifest_json(
            Path::new(
                r"C:\Program Files\Simple Download Manager\simple-download-manager-native-host.exe",
            ),
            "allowed_origins",
            serde_json::json!(["chrome-extension://extension-id/"]),
        );

        assert_eq!(
            manifest.get("path").and_then(|value| value.as_str()),
            Some(
                r"C:\Program Files\Simple Download Manager\simple-download-manager-native-host.exe"
            )
        );
        assert_eq!(
            manifest
                .get("allowed_origins")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("chrome-extension://extension-id/")
        );
    }
}
