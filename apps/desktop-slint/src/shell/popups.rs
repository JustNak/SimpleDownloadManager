use crate::controller::{
    download_progress_metrics, empty_batch_details, empty_progress_details, prompt_confirm_request,
    record_progress_sample, reset_prompt_interaction_state,
    slint_batch_details_from_context_with_state, slint_progress_details_from_job_with_state,
    slint_prompt_details_from_prompt_with_state, waiting_prompt_details,
    ProgressPopupInteractionState, ProgressSample, PromptConfirmAction,
    PromptWindowInteractionState,
};
use crate::{BatchProgressWindow, DownloadPromptWindow, HttpProgressWindow, TorrentProgressWindow};
use simple_download_manager_desktop_core::contracts::{ConfirmPromptRequest, ProgressBatchContext};
use simple_download_manager_desktop_core::storage::{
    DesktopSnapshot, DownloadJob, DownloadPrompt, JobState, TransferKind,
};
use slint::{
    CloseRequestResponse, ComponentHandle, PhysicalPosition, PhysicalSize, WindowPosition,
    WindowSize as SlintWindowSize,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{WindowRole, WindowSize};

const PROGRESS_WINDOW_PREFIX: &str = "download-progress-";
const TORRENT_PROGRESS_WINDOW_PREFIX: &str = "torrent-progress-";
const BATCH_PROGRESS_WINDOW_PREFIX: &str = "batch-progress-";
const PROGRESS_WINDOW_STACK_OFFSET: i32 = 28;

pub type PromptWindowActionDispatcher = Arc<dyn Fn(PromptWindowAction) + Send + Sync>;
pub type ProgressPopupActionDispatcher = Arc<dyn Fn(ProgressPopupAction) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptWindowAction {
    BrowseDirectory,
    Confirm(ConfirmPromptRequest),
    Cancel(String),
    Swap(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressPopupAction {
    Pause(String),
    Resume(String),
    Retry(String),
    Cancel(String),
    OpenFile(String),
    RevealInFolder(String),
    SwapFailedToBrowser(String),
    BatchPause(Vec<String>),
    BatchResume(Vec<String>),
    BatchCancel(Vec<String>),
    BatchRevealCompleted(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupWindowConfig {
    pub title: &'static str,
    pub size: WindowSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupWindowPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PopupLifecycleState {
    last_prompt_position: Option<PopupWindowPosition>,
}

impl PopupLifecycleState {
    pub fn close_prompt(&mut self, remember_position: bool, position: Option<PopupWindowPosition>) {
        if remember_position {
            self.last_prompt_position = position;
        }
    }

    pub fn last_prompt_position(&self) -> Option<PopupWindowPosition> {
        self.last_prompt_position
    }
}

#[derive(Clone, Default)]
pub struct PendingSelectedJob {
    value: Arc<Mutex<Option<String>>>,
}

impl PendingSelectedJob {
    pub fn queue(&self, id: String) {
        if let Ok(mut value) = self.value.lock() {
            *value = Some(id);
        }
    }

    pub fn take(&self) -> Option<String> {
        self.value.lock().ok().and_then(|mut value| value.take())
    }
}

#[derive(Default)]
pub struct PopupRegistry {
    prompt: Option<DownloadPromptWindow>,
    current_prompt: Option<DownloadPrompt>,
    prompt_interaction: PromptWindowInteractionState,
    http_progress: HashMap<String, HttpProgressWindow>,
    torrent_progress: HashMap<String, TorrentProgressWindow>,
    batch_progress: HashMap<String, BatchProgressWindow>,
    progress_interactions: HashMap<String, ProgressPopupInteractionState>,
    batch_interactions: HashMap<String, ProgressPopupInteractionState>,
    progress_samples: Vec<ProgressSample>,
    batch_contexts: HashMap<String, ProgressBatchContext>,
    snapshot: Option<DesktopSnapshot>,
    lifecycle: PopupLifecycleState,
}

thread_local! {
    static POPUP_REGISTRY: RefCell<PopupRegistry> = RefCell::new(PopupRegistry::default());
    static PROMPT_ACTION_DISPATCHER: RefCell<Option<PromptWindowActionDispatcher>> = RefCell::new(None);
    static PROGRESS_POPUP_ACTION_DISPATCHER: RefCell<Option<ProgressPopupActionDispatcher>> = RefCell::new(None);
}

pub fn with_popup_registry<R>(operation: impl FnOnce(&mut PopupRegistry) -> R) -> R {
    POPUP_REGISTRY.with(|registry| operation(&mut registry.borrow_mut()))
}

pub fn install_prompt_action_dispatcher(dispatcher: PromptWindowActionDispatcher) {
    PROMPT_ACTION_DISPATCHER.with(|current| {
        *current.borrow_mut() = Some(dispatcher);
    });
}

pub fn clear_prompt_action_dispatcher_for_tests() {
    PROMPT_ACTION_DISPATCHER.with(|current| {
        *current.borrow_mut() = None;
    });
}

pub fn install_progress_popup_action_dispatcher(dispatcher: ProgressPopupActionDispatcher) {
    PROGRESS_POPUP_ACTION_DISPATCHER.with(|current| {
        *current.borrow_mut() = Some(dispatcher);
    });
}

pub fn clear_progress_popup_action_dispatcher_for_tests() {
    PROGRESS_POPUP_ACTION_DISPATCHER.with(|current| {
        *current.borrow_mut() = None;
    });
}

pub fn dispatch_prompt_window_action(action: PromptWindowAction) -> Result<(), String> {
    let dispatcher = PROMPT_ACTION_DISPATCHER.with(|current| current.borrow().clone());
    let Some(dispatcher) = dispatcher else {
        return Err("Prompt window action dispatcher is not installed.".into());
    };
    dispatcher(action);
    Ok(())
}

pub fn dispatch_progress_popup_action(action: ProgressPopupAction) -> Result<(), String> {
    let dispatcher = PROGRESS_POPUP_ACTION_DISPATCHER.with(|current| current.borrow().clone());
    let Some(dispatcher) = dispatcher else {
        return Err("Progress popup action dispatcher is not installed.".into());
    };
    dispatcher(action);
    Ok(())
}

pub fn popup_window_config(role: WindowRole) -> PopupWindowConfig {
    let title = match role {
        WindowRole::Main => "Simple Download Manager",
        WindowRole::DownloadPrompt => "New download detected",
        WindowRole::HttpProgress => "Download progress",
        WindowRole::TorrentProgress => "Torrent session",
        WindowRole::BatchProgress => "Batch progress",
    };

    PopupWindowConfig {
        title,
        size: role.default_size(),
    }
}

pub fn progress_window_label(job_id: &str) -> String {
    format!(
        "{PROGRESS_WINDOW_PREFIX}{}",
        sanitize_progress_identifier(job_id)
    )
}

pub fn torrent_progress_window_label(job_id: &str) -> String {
    format!(
        "{TORRENT_PROGRESS_WINDOW_PREFIX}{}",
        sanitize_progress_identifier(job_id)
    )
}

pub fn batch_progress_window_label(batch_id: &str) -> String {
    format!(
        "{BATCH_PROGRESS_WINDOW_PREFIX}{}",
        sanitize_batch_identifier(batch_id)
    )
}

pub fn progress_window_position(
    prompt_position: Option<PopupWindowPosition>,
    open_progress_windows: usize,
) -> Option<PopupWindowPosition> {
    let prompt_position = prompt_position?;
    let offset = open_progress_windows.min(8) as i32 * PROGRESS_WINDOW_STACK_OFFSET;

    Some(PopupWindowPosition {
        x: prompt_position.x + offset,
        y: prompt_position.y + offset,
    })
}

impl PopupRegistry {
    pub fn apply_snapshot(&mut self, snapshot: &DesktopSnapshot) {
        let timestamp = unix_timestamp_millis();
        let mut samples = std::mem::take(&mut self.progress_samples);
        for job in &snapshot.jobs {
            samples = record_progress_sample(samples, job, timestamp);
        }
        self.progress_samples = samples;
        self.snapshot = Some(snapshot.clone());
        self.refresh_open_progress_windows();
    }

    pub fn set_download_prompt(&mut self, prompt: Option<DownloadPrompt>) -> Result<(), String> {
        let previous_id = self
            .current_prompt
            .as_ref()
            .map(|prompt| prompt.id.as_str());
        let next_id = prompt.as_ref().map(|prompt| prompt.id.as_str());
        if previous_id != next_id {
            reset_prompt_interaction_state(&mut self.prompt_interaction, prompt.as_ref());
        }
        self.current_prompt = prompt;
        if self.current_prompt.is_some() {
            self.show_download_prompt_window()
        } else {
            self.close_download_prompt_window(false)
        }
    }

    pub fn show_download_prompt_window(&mut self) -> Result<(), String> {
        let details = self
            .current_prompt
            .as_ref()
            .map(|prompt| {
                slint_prompt_details_from_prompt_with_state(prompt, &self.prompt_interaction)
            })
            .unwrap_or_else(waiting_prompt_details);
        self.ensure_download_prompt_window()?;
        let window = self
            .prompt
            .as_ref()
            .expect("prompt window should exist after ensure");
        window.set_prompt(details);
        show_component(window)
    }

    pub fn close_download_prompt_window(&mut self, remember_position: bool) -> Result<(), String> {
        let position = self
            .prompt
            .as_ref()
            .map(|window| popup_position(window.window()));
        self.lifecycle.close_prompt(remember_position, position);
        if let Some(window) = &self.prompt {
            window.hide().map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn apply_prompt_directory_override(&mut self, directory: Option<String>) {
        self.prompt_interaction.directory_override = directory;
        self.prompt_interaction.busy = false;
        self.prompt_interaction.error_text.clear();
        self.refresh_download_prompt_window();
    }

    pub fn set_prompt_busy(&mut self, busy: bool) {
        self.prompt_interaction.busy = busy;
        self.refresh_download_prompt_window();
    }

    pub fn set_prompt_error(&mut self, error: String) {
        self.prompt_interaction.busy = false;
        self.prompt_interaction.error_text = error;
        self.refresh_download_prompt_window();
    }

    pub fn set_progress_action_busy(&mut self, id: &str, busy: bool) {
        self.progress_interactions
            .entry(id.into())
            .or_default()
            .busy = busy;
        self.refresh_open_progress_windows();
    }

    pub fn set_progress_action_error(&mut self, id: &str, error: String) {
        let state = self.progress_interactions.entry(id.into()).or_default();
        state.busy = false;
        state.error_text = error;
        self.refresh_open_progress_windows();
    }

    pub fn set_batch_action_busy(&mut self, batch_id: &str, busy: bool) {
        self.batch_interactions
            .entry(batch_id.into())
            .or_default()
            .busy = busy;
        self.refresh_open_progress_windows();
    }

    pub fn set_batch_action_error(&mut self, batch_id: &str, error: String) {
        let state = self.batch_interactions.entry(batch_id.into()).or_default();
        state.busy = false;
        state.error_text = error;
        self.refresh_open_progress_windows();
    }

    pub fn prompt_change_directory_requested(&mut self) {
        if self.current_prompt.is_none() {
            return;
        }
        self.prompt_interaction.busy = true;
        self.prompt_interaction.error_text.clear();
        self.refresh_download_prompt_window();
        if let Err(error) = dispatch_prompt_window_action(PromptWindowAction::BrowseDirectory) {
            self.set_prompt_error(error);
        }
    }

    pub fn prompt_cancel_requested(&mut self) {
        let Some(prompt) = self.current_prompt.as_ref() else {
            return;
        };
        self.prompt_interaction.busy = true;
        self.prompt_interaction.error_text.clear();
        let id = prompt.id.clone();
        self.refresh_download_prompt_window();
        if let Err(error) = dispatch_prompt_window_action(PromptWindowAction::Cancel(id)) {
            self.set_prompt_error(error);
        }
    }

    pub fn prompt_download_requested(&mut self) {
        self.dispatch_prompt_confirm(PromptConfirmAction::DefaultDownload);
    }

    pub fn prompt_swap_requested(&mut self) {
        let Some(prompt) = self.current_prompt.as_ref() else {
            return;
        };
        self.prompt_interaction.busy = true;
        self.prompt_interaction.error_text.clear();
        let id = prompt.id.clone();
        self.refresh_download_prompt_window();
        if let Err(error) = dispatch_prompt_window_action(PromptWindowAction::Swap(id)) {
            self.set_prompt_error(error);
        }
    }

    pub fn prompt_duplicate_menu_toggled(&mut self) {
        self.prompt_interaction.duplicate_menu_open = !self.prompt_interaction.duplicate_menu_open;
        self.prompt_interaction.error_text.clear();
        self.refresh_download_prompt_window();
    }

    pub fn prompt_duplicate_action_requested(&mut self, action_id: &str) {
        match action_id {
            "overwrite" => self.dispatch_prompt_confirm(PromptConfirmAction::Overwrite),
            "download_anyway" => self.dispatch_prompt_confirm(PromptConfirmAction::DownloadAnyway),
            _ => self.set_prompt_error("Unknown duplicate prompt action.".into()),
        }
    }

    pub fn prompt_duplicate_rename_started(&mut self) {
        if let Some(prompt) = self.current_prompt.as_ref() {
            if self.prompt_interaction.renamed_filename.trim().is_empty() {
                self.prompt_interaction.renamed_filename = prompt.filename.clone();
            }
        }
        self.prompt_interaction.duplicate_menu_open = false;
        self.prompt_interaction.renaming_duplicate = true;
        self.prompt_interaction.error_text.clear();
        self.refresh_download_prompt_window();
    }

    pub fn prompt_renamed_filename_changed(&mut self, filename: String) {
        self.prompt_interaction.renamed_filename = filename;
        self.prompt_interaction.error_text.clear();
        self.refresh_download_prompt_window();
    }

    pub fn prompt_duplicate_rename_confirmed(&mut self) {
        self.dispatch_prompt_confirm(PromptConfirmAction::Rename);
    }

    pub fn prompt_duplicate_rename_cancelled(&mut self) {
        if let Some(prompt) = self.current_prompt.as_ref() {
            self.prompt_interaction.renamed_filename = prompt.filename.clone();
        }
        self.prompt_interaction.renaming_duplicate = false;
        self.prompt_interaction.duplicate_menu_open = false;
        self.prompt_interaction.error_text.clear();
        self.refresh_download_prompt_window();
    }

    pub fn progress_action_requested(&mut self, id: String, action: ProgressPopupAction) {
        if matches!(action, ProgressPopupAction::Cancel(_)) {
            let state = self.progress_interactions.entry(id.clone()).or_default();
            if !state.cancel_confirming {
                state.cancel_confirming = true;
                state.error_text.clear();
                self.refresh_open_progress_windows();
                return;
            }
        }

        let state = self.progress_interactions.entry(id.clone()).or_default();
        state.busy = true;
        state.cancel_confirming = false;
        state.error_text.clear();
        self.refresh_open_progress_windows();
        if let Err(error) = dispatch_progress_popup_action(action) {
            self.set_progress_action_error(&id, error);
        }
    }

    pub fn progress_close_requested(&mut self, id: String, transfer_kind: TransferKind) {
        let result = match transfer_kind {
            TransferKind::Http => self.http_progress.get(&id).map(|window| window.hide()),
            TransferKind::Torrent => self.torrent_progress.get(&id).map(|window| window.hide()),
        };
        if let Some(Err(error)) = result {
            self.set_progress_action_error(&id, error.to_string());
        }
    }

    pub fn batch_action_requested(&mut self, batch_id: String, action: ProgressPopupAction) {
        let state = self.batch_interactions.entry(batch_id.clone()).or_default();
        state.busy = true;
        state.cancel_confirming = false;
        state.error_text.clear();
        self.refresh_open_progress_windows();
        if let Err(error) = dispatch_progress_popup_action(action) {
            self.set_batch_action_error(&batch_id, error);
        }
    }

    pub fn batch_close_requested(&mut self, batch_id: String) {
        if let Some(window) = self.batch_progress.get(&batch_id) {
            if let Err(error) = window.hide() {
                self.set_batch_action_error(&batch_id, error.to_string());
            }
        }
    }

    pub fn show_progress_window(
        &mut self,
        id: String,
        transfer_kind: TransferKind,
    ) -> Result<(), String> {
        match transfer_kind {
            TransferKind::Http => self.show_http_progress_window(id),
            TransferKind::Torrent => self.show_torrent_progress_window(id),
        }
    }

    pub fn show_batch_progress_window(
        &mut self,
        batch_id: String,
        context: Option<ProgressBatchContext>,
    ) -> Result<(), String> {
        if let Some(context) = context {
            self.batch_contexts.insert(batch_id.clone(), context);
        }
        let details = self.batch_details(&batch_id);
        if !self.batch_progress.contains_key(&batch_id) {
            let window = BatchProgressWindow::new().map_err(|error| error.to_string())?;
            configure_component_window(&window, WindowRole::BatchProgress);
            install_hide_on_close(&window);
            install_batch_progress_callbacks(&window);
            if let Some(position) = self.next_progress_position() {
                set_component_position(&window, position);
            }
            self.batch_progress.insert(batch_id.clone(), window);
        }
        let window = self
            .batch_progress
            .get(&batch_id)
            .expect("batch progress window should exist after insert");
        window.set_details(details);
        show_component(window)
    }

    fn show_http_progress_window(&mut self, id: String) -> Result<(), String> {
        let details =
            self.progress_details(&id, popup_window_config(WindowRole::HttpProgress).title);
        if !self.http_progress.contains_key(&id) {
            let window = HttpProgressWindow::new().map_err(|error| error.to_string())?;
            configure_component_window(&window, WindowRole::HttpProgress);
            install_hide_on_close(&window);
            install_http_progress_callbacks(&window);
            if let Some(position) = self.next_progress_position() {
                set_component_position(&window, position);
            }
            self.http_progress.insert(id.clone(), window);
        }
        let window = self
            .http_progress
            .get(&id)
            .expect("HTTP progress window should exist after insert");
        window.set_details(details);
        show_component(window)
    }

    fn show_torrent_progress_window(&mut self, id: String) -> Result<(), String> {
        let details =
            self.progress_details(&id, popup_window_config(WindowRole::TorrentProgress).title);
        if !self.torrent_progress.contains_key(&id) {
            let window = TorrentProgressWindow::new().map_err(|error| error.to_string())?;
            configure_component_window(&window, WindowRole::TorrentProgress);
            install_hide_on_close(&window);
            install_torrent_progress_callbacks(&window);
            if let Some(position) = self.next_progress_position() {
                set_component_position(&window, position);
            }
            self.torrent_progress.insert(id.clone(), window);
        }
        let window = self
            .torrent_progress
            .get(&id)
            .expect("torrent progress window should exist after insert");
        window.set_details(details);
        show_component(window)
    }

    fn ensure_download_prompt_window(&mut self) -> Result<(), String> {
        if self.prompt.is_some() {
            return Ok(());
        }

        let window = DownloadPromptWindow::new().map_err(|error| error.to_string())?;
        configure_component_window(&window, WindowRole::DownloadPrompt);
        install_hide_on_close(&window);
        install_download_prompt_callbacks(&window);
        self.prompt = Some(window);
        Ok(())
    }

    fn dispatch_prompt_confirm(&mut self, action: PromptConfirmAction) {
        let Some(prompt) = self.current_prompt.as_ref() else {
            return;
        };
        match prompt_confirm_request(prompt, &self.prompt_interaction, action) {
            Ok(request) => {
                self.prompt_interaction.busy = true;
                self.prompt_interaction.error_text.clear();
                self.refresh_download_prompt_window();
                if let Err(error) =
                    dispatch_prompt_window_action(PromptWindowAction::Confirm(request))
                {
                    self.set_prompt_error(error);
                }
            }
            Err(error) => self.set_prompt_error(error),
        }
    }

    fn refresh_download_prompt_window(&self) {
        let Some(window) = &self.prompt else {
            return;
        };
        let details = self
            .current_prompt
            .as_ref()
            .map(|prompt| {
                slint_prompt_details_from_prompt_with_state(prompt, &self.prompt_interaction)
            })
            .unwrap_or_else(waiting_prompt_details);
        window.set_prompt(details);
    }

    fn refresh_open_progress_windows(&self) {
        for (id, window) in &self.http_progress {
            window.set_details(
                self.progress_details(id, popup_window_config(WindowRole::HttpProgress).title),
            );
        }
        for (id, window) in &self.torrent_progress {
            window.set_details(
                self.progress_details(id, popup_window_config(WindowRole::TorrentProgress).title),
            );
        }
        for (batch_id, window) in &self.batch_progress {
            window.set_details(self.batch_details(batch_id));
        }
    }

    fn progress_details(&self, id: &str, title: &str) -> crate::ProgressDetails {
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.jobs.iter().find(|job| job.id == id))
            .map(|job| {
                let interaction = self
                    .progress_interactions
                    .get(id)
                    .cloned()
                    .unwrap_or_default();
                let metrics =
                    download_progress_metrics(job, &self.progress_samples, unix_timestamp_millis());
                slint_progress_details_from_job_with_state(job, title, &metrics, &interaction)
            })
            .unwrap_or_else(|| empty_progress_details(id, title))
    }

    fn batch_details(&self, batch_id: &str) -> crate::BatchDetails {
        self.batch_contexts
            .get(batch_id)
            .and_then(|context| {
                self.snapshot.as_ref().map(|snapshot| {
                    let interaction = self
                        .batch_interactions
                        .get(batch_id)
                        .cloned()
                        .unwrap_or_default();
                    slint_batch_details_from_context_with_state(context, snapshot, &interaction)
                })
            })
            .unwrap_or_else(|| empty_batch_details(batch_id))
    }

    fn batch_jobs_for_action(
        &self,
        batch_id: &str,
        predicate: impl Fn(&DownloadJob) -> bool,
    ) -> Vec<String> {
        let Some(context) = self.batch_contexts.get(batch_id) else {
            return Vec::new();
        };
        let Some(snapshot) = self.snapshot.as_ref() else {
            return Vec::new();
        };
        context
            .job_ids
            .iter()
            .filter_map(|id| snapshot.jobs.iter().find(|job| job.id == *id))
            .filter(|job| predicate(job))
            .map(|job| job.id.clone())
            .collect()
    }

    fn next_progress_position(&self) -> Option<PopupWindowPosition> {
        progress_window_position(
            self.lifecycle.last_prompt_position(),
            self.open_progress_popup_count(),
        )
    }

    fn open_progress_popup_count(&self) -> usize {
        self.http_progress.len() + self.torrent_progress.len() + self.batch_progress.len()
    }
}

fn configure_component_window(component: &impl ComponentHandle, role: WindowRole) {
    let size = popup_window_config(role).size;
    component
        .window()
        .set_size(SlintWindowSize::Physical(PhysicalSize::new(
            size.width,
            size.height,
        )));
}

fn install_hide_on_close(component: &impl ComponentHandle) {
    component
        .window()
        .on_close_requested(|| CloseRequestResponse::HideWindow);
}

fn install_download_prompt_callbacks(window: &DownloadPromptWindow) {
    window.on_change_directory_requested(|| {
        with_popup_registry(|registry| registry.prompt_change_directory_requested());
    });
    window.on_cancel_requested(|| {
        with_popup_registry(|registry| registry.prompt_cancel_requested());
    });
    window.on_download_requested(|| {
        with_popup_registry(|registry| registry.prompt_download_requested());
    });
    window.on_swap_requested(|| {
        with_popup_registry(|registry| registry.prompt_swap_requested());
    });
    window.on_duplicate_menu_toggled(|| {
        with_popup_registry(|registry| registry.prompt_duplicate_menu_toggled());
    });
    window.on_duplicate_action_requested(|action| {
        with_popup_registry(|registry| registry.prompt_duplicate_action_requested(action.as_str()));
    });
    window.on_duplicate_rename_started(|| {
        with_popup_registry(|registry| registry.prompt_duplicate_rename_started());
    });
    window.on_duplicate_renamed_filename_changed(|filename| {
        with_popup_registry(|registry| registry.prompt_renamed_filename_changed(filename.into()));
    });
    window.on_duplicate_rename_confirmed(|| {
        with_popup_registry(|registry| registry.prompt_duplicate_rename_confirmed());
    });
    window.on_duplicate_rename_cancelled(|| {
        with_popup_registry(|registry| registry.prompt_duplicate_rename_cancelled());
    });
}

fn install_http_progress_callbacks(window: &HttpProgressWindow) {
    window.on_progress_pause_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::Pause(id));
        });
    });
    window.on_progress_resume_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::Resume(id));
        });
    });
    window.on_progress_retry_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::Retry(id));
        });
    });
    window.on_progress_cancel_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::Cancel(id));
        });
    });
    window.on_progress_open_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::OpenFile(id));
        });
    });
    window.on_progress_reveal_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::RevealInFolder(id));
        });
    });
    window.on_progress_swap_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(
                id.clone(),
                ProgressPopupAction::SwapFailedToBrowser(id),
            );
        });
    });
    window.on_progress_close_requested(|id| {
        with_popup_registry(|registry| {
            registry.progress_close_requested(id.to_string(), TransferKind::Http);
        });
    });
}

fn install_torrent_progress_callbacks(window: &TorrentProgressWindow) {
    window.on_progress_pause_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::Pause(id));
        });
    });
    window.on_progress_resume_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::Resume(id));
        });
    });
    window.on_progress_retry_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::Retry(id));
        });
    });
    window.on_progress_cancel_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::Cancel(id));
        });
    });
    window.on_progress_open_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::OpenFile(id));
        });
    });
    window.on_progress_reveal_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(id.clone(), ProgressPopupAction::RevealInFolder(id));
        });
    });
    window.on_progress_swap_requested(|id| {
        let id = id.to_string();
        with_popup_registry(|registry| {
            registry.progress_action_requested(
                id.clone(),
                ProgressPopupAction::SwapFailedToBrowser(id),
            );
        });
    });
    window.on_progress_close_requested(|id| {
        with_popup_registry(|registry| {
            registry.progress_close_requested(id.to_string(), TransferKind::Torrent);
        });
    });
}

fn install_batch_progress_callbacks(window: &BatchProgressWindow) {
    window.on_batch_pause_requested(|batch_id| {
        let batch_id = batch_id.to_string();
        with_popup_registry(|registry| {
            let ids = registry.batch_jobs_for_action(&batch_id, batch_can_pause);
            registry.batch_action_requested(batch_id, ProgressPopupAction::BatchPause(ids));
        });
    });
    window.on_batch_resume_requested(|batch_id| {
        let batch_id = batch_id.to_string();
        with_popup_registry(|registry| {
            let ids = registry.batch_jobs_for_action(&batch_id, batch_can_resume);
            registry.batch_action_requested(batch_id, ProgressPopupAction::BatchResume(ids));
        });
    });
    window.on_batch_cancel_requested(|batch_id| {
        let batch_id = batch_id.to_string();
        with_popup_registry(|registry| {
            let ids = registry.batch_jobs_for_action(&batch_id, batch_can_cancel);
            registry.batch_action_requested(batch_id, ProgressPopupAction::BatchCancel(ids));
        });
    });
    window.on_batch_reveal_completed_requested(|batch_id| {
        let batch_id = batch_id.to_string();
        with_popup_registry(|registry| {
            let ids = registry.batch_jobs_for_action(&batch_id, |job| {
                job.state == JobState::Completed && !job.target_path.trim().is_empty()
            });
            if let Some(id) = ids.into_iter().next() {
                registry.batch_action_requested(
                    batch_id,
                    ProgressPopupAction::BatchRevealCompleted(id),
                );
            }
        });
    });
    window.on_batch_close_requested(|batch_id| {
        with_popup_registry(|registry| {
            registry.batch_close_requested(batch_id.to_string());
        });
    });
}

fn batch_can_pause(job: &DownloadJob) -> bool {
    matches!(
        job.state,
        JobState::Queued | JobState::Starting | JobState::Downloading | JobState::Seeding
    )
}

fn batch_can_resume(job: &DownloadJob) -> bool {
    matches!(
        job.state,
        JobState::Paused | JobState::Failed | JobState::Canceled
    )
}

fn batch_can_cancel(job: &DownloadJob) -> bool {
    matches!(
        job.state,
        JobState::Queued
            | JobState::Starting
            | JobState::Downloading
            | JobState::Seeding
            | JobState::Paused
    )
}

fn show_component(component: &impl ComponentHandle) -> Result<(), String> {
    component.window().set_minimized(false);
    component.show().map_err(|error| error.to_string())
}

fn set_component_position(component: &impl ComponentHandle, position: PopupWindowPosition) {
    component
        .window()
        .set_position(WindowPosition::Physical(PhysicalPosition::new(
            position.x, position.y,
        )));
}

fn popup_position(window: &slint::Window) -> PopupWindowPosition {
    let position = window.position();
    PopupWindowPosition {
        x: position.x,
        y: position.y,
    }
}

fn unix_timestamp_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn sanitize_progress_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | ':' | '/')
        })
        .collect()
}

fn sanitize_batch_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .collect()
}
