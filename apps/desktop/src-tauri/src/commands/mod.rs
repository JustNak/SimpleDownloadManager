use crate::download::schedule_downloads;
use crate::ipc::gather_host_registration_diagnostics;
use crate::prompts::{PromptDecision, PromptRegistry, PROMPT_CHANGED_EVENT};
use crate::state::{EnqueueResult, EnqueueStatus, SharedState};
use crate::storage::{DesktopSnapshot, DiagnosticsSnapshot, DownloadPrompt, DownloadSource, Settings};
use crate::windows::{
    close_download_prompt_window, focus_job_in_main_window, show_download_prompt_window,
    show_progress_window, DOWNLOAD_PROMPT_WINDOW,
};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{AppHandle, Emitter, State};

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(windows)]
use windows_sys::Win32::UI::Shell::ShellExecuteW;

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub const STATE_CHANGED_EVENT: &str = "app://state-changed";
const INSTALL_RESOURCE_DIR: &str = "resources\\install";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseMetadata {
    sidecar_binary_name: Option<String>,
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
    let next_prompt = prompts.resolve(id, decision).await?;
    if let Some(prompt) = next_prompt {
        show_download_prompt_window(app)?;
        app.emit_to(DOWNLOAD_PROMPT_WINDOW, PROMPT_CHANGED_EVENT, prompt)
            .map_err(|error| error.to_string())?;
    } else {
        close_download_prompt_window(app);
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
pub async fn export_diagnostics_report(state: State<'_, SharedState>) -> Result<Option<String>, String> {
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
pub async fn add_job(app: AppHandle, state: State<'_, SharedState>, url: String) -> Result<AddJobResult, String> {
    let result = state
        .enqueue_download(url, None)
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
    let snapshot = state.pause_all_jobs().await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn resume_all_jobs(app: AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let snapshot = state.resume_all_jobs().await.map_err(|error| error.message)?;
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
    let snapshot = state.restart_job(&id).await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn retry_failed_jobs(app: AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let snapshot = state.retry_failed_jobs().await.map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn remove_job(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
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
    let snapshot = state
        .delete_job(&id, delete_from_disk)
        .await
        .map_err(|error| error.message)?;
    emit_snapshot(&app, &snapshot);
    schedule_downloads(app, state.inner().clone());
    Ok(())
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
pub async fn clear_completed_jobs(app: AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
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
    settings: Settings,
) -> Result<Settings, String> {
    let snapshot = state.save_settings(settings).await?;
    let saved_settings = snapshot.settings.clone();
    emit_snapshot(&app, &snapshot);
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
    allow_duplicate: bool,
) -> Result<(), String> {
    complete_prompt_action(
        &app,
        prompts.inner().clone(),
        &id,
        PromptDecision::Download {
            directory_override,
            allow_duplicate,
        },
    )
    .await
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

    complete_prompt_action(&app, prompts.inner().clone(), &id, PromptDecision::ShowExisting)
        .await?;

    if let Some(job_id) = existing_job_id {
        focus_job_in_main_window(&app, &job_id);
    }

    Ok(())
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
pub async fn open_job_file(state: State<'_, SharedState>, id: String) -> Result<(), String> {
    let path = state.resolve_openable_path(&id).await.map_err(|error| error.message)?;

    tauri::async_runtime::spawn_blocking(move || open_path(&path))
        .await
        .map_err(|error| format!("Could not open file: {error}"))??;

    Ok(())
}

#[tauri::command]
pub async fn reveal_job_in_folder(state: State<'_, SharedState>, id: String) -> Result<(), String> {
    let path = state
        .resolve_revealable_path(&id)
        .await
        .map_err(|error| error.message)?;

    tauri::async_runtime::spawn_blocking(move || reveal_path(&path))
        .await
        .map_err(|error| format!("Could not reveal file: {error}"))??;

    Ok(())
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
    let script_path = resolve_install_resource_path("register-native-host.ps1")?;
    let host_binary_path = resolve_host_binary_path()?;

    tauri::async_runtime::spawn_blocking(move || run_registration_script(&script_path, &host_binary_path))
        .await
        .map_err(|error| format!("Could not start host registration: {error}"))??;

    Ok(())
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
        let decision = receiver.await.unwrap_or(PromptDecision::Cancel);
        match decision {
            PromptDecision::Cancel => {}
            PromptDecision::ShowExisting => {
                if let Some(job) = prompt.duplicate_job {
                    focus_job_in_main_window(&worker_app, &job.id);
                }
            }
            PromptDecision::Download {
                directory_override,
                allow_duplicate,
            } => {
                let result = worker_state
                    .enqueue_download_with_options(
                        prompt.url,
                        crate::state::EnqueueOptions {
                            source: prompt.source,
                            directory_override,
                            filename_hint: Some(prompt.filename),
                            duplicate_policy: if allow_duplicate {
                                crate::state::DuplicatePolicy::Allow
                            } else {
                                crate::state::DuplicatePolicy::ReturnExisting
                            },
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
fn shell_execute(operation: &OsStr, file: &OsStr, parameters: Option<&OsStr>) -> Result<(), String> {
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
    let current_exe = std::env::current_exe().map_err(|error| format!("Could not locate app binary: {error}"))?;
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
        for relative_root in ["src-tauri\\resources\\install", "apps\\desktop\\src-tauri\\resources\\install"] {
            let candidate = ancestor.join(relative_root).join(file_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Err(format!("Could not find bundled install resource: {file_name}."))
}

fn resolve_host_binary_path() -> Result<PathBuf, String> {
    let install_root = current_install_root()?;
    let mut candidate_names = Vec::new();

    if let Ok(release_path) = resolve_install_resource_path("release.json") {
        if let Ok(content) = std::fs::read_to_string(release_path) {
            if let Ok(metadata) = serde_json::from_str::<ReleaseMetadata>(&content) {
                if let Some(sidecar_binary_name) = metadata.sidecar_binary_name {
                    candidate_names.push(sidecar_binary_name);
                }
            }
        }
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
fn run_registration_script(script_path: &Path, host_binary_path: &Path) -> Result<(), String> {
    let status = Command::new("pwsh")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(script_path)
        .arg("-HostBinaryPath")
        .arg(host_binary_path)
        .status()
        .map_err(|error| format!("Could not start registration script: {error}"))?;

    if !status.success() {
        return Err(format!("Registration script exited with status {status}."));
    }

    Ok(())
}

#[cfg(not(windows))]
fn run_registration_script(_script_path: &Path, _host_binary_path: &Path) -> Result<(), String> {
    Err("Native host registration is only supported on Windows in this build.".into())
}
