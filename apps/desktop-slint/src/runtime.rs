use crate::controller::{
    active_download_urls, add_download_form_model, add_download_outcome_for_result, build_filename,
    delete_prompt_from_jobs, ensure_trailing_editable_line, normalize_archive_name,
    queue_view_model_from_snapshot, slint_job_row_from_row, slint_nav_item_from_item,
    split_filename, status_text_from_snapshot, validate_optional_sha256, view_for_job,
    AddDownloadFormState, AddDownloadProgressIntent, AddDownloadResult, DeletePromptDetails,
    DownloadMode, QueueUiState, SortColumn, ViewFilter,
};
use crate::shell::{self, lifecycle, main_window};
use crate::update::{self, AppUpdateState, PendingUpdateState, UpdateCheckMode, UpdateStateStore};
use crate::MainWindow;
use simple_download_manager_desktop_core::backend::{
    prompt_enqueue_details, CoreDesktopBackend, ProgressBatchRegistry,
};
use simple_download_manager_desktop_core::contracts::{
    AddJobRequest, AddJobResult, AddJobsRequest, AddJobsResult, AppUpdateMetadata, BackendFuture,
    DesktopBackend, DesktopEvent, ProgressBatchContext, ShellServices,
};
use simple_download_manager_desktop_core::prompts::{PromptDecision, PromptRegistry};
use simple_download_manager_desktop_core::state::{EnqueueOptions, EnqueueStatus, SharedState};
use simple_download_manager_desktop_core::storage::{
    DesktopSnapshot, DownloadPrompt, DownloadSource, HostRegistrationDiagnostics, TorrentInfo,
    TorrentSettings, TransferKind,
};
use simple_download_manager_desktop_core::transfer::{self, TransferShell};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::runtime::{Builder, Runtime};

pub type SlintBackend<D> = CoreDesktopBackend<SlintShellServices<D>>;

#[derive(Clone, Debug)]
pub enum UiAction {
    ApplySnapshot(Box<DesktopSnapshot>),
    FocusMainWindow,
    FocusJobInMainWindow {
        id: String,
    },
    DownloadPromptChanged(Option<Box<DownloadPrompt>>),
    ShowDownloadPromptWindow,
    CloseDownloadPromptWindow {
        remember_position: bool,
    },
    ShowProgressWindow {
        id: String,
        transfer_kind: TransferKind,
    },
    ShowBatchProgressWindow {
        batch_id: String,
        context: Option<ProgressBatchContext>,
    },
    HideMainWindow,
    RequestExit,
    ApplyUpdateState(Box<AppUpdateState>),
}

pub trait UiDispatcher: Clone + Send + Sync + 'static {
    fn dispatch(&self, action: UiAction) -> Result<(), String>;
}

#[derive(Clone, Default)]
pub struct QueueViewRuntimeState {
    state: Arc<Mutex<QueueUiState>>,
    snapshot: Arc<Mutex<Option<DesktopSnapshot>>>,
    delete_prompt: Arc<Mutex<Option<DeletePromptDetails>>>,
    rename_prompt: Arc<Mutex<Option<RenamePromptState>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenamePromptState {
    id: String,
    original_filename: String,
    base_name: String,
    extension: String,
}

#[derive(Clone, Default)]
pub struct AddDownloadRuntimeState {
    state: Arc<Mutex<AddDownloadFormState>>,
    visible: Arc<Mutex<bool>>,
    submitting: Arc<Mutex<bool>>,
    importing: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AddDownloadSubmission {
    mode: DownloadMode,
    urls: Vec<String>,
    expected_sha256: Option<String>,
    bulk_archive_name: Option<String>,
}

impl AddDownloadRuntimeState {
    pub fn open(&self, ui: &MainWindow) {
        *self.state.lock().expect("add download state lock") = AddDownloadFormState::default();
        *self.visible.lock().expect("add download visible lock") = true;
        *self
            .submitting
            .lock()
            .expect("add download submitting lock") = false;
        *self.importing.lock().expect("add download importing lock") = false;
        self.render(ui);
    }

    pub fn close(&self, ui: &MainWindow) {
        *self.state.lock().expect("add download state lock") = AddDownloadFormState::default();
        *self.visible.lock().expect("add download visible lock") = false;
        *self
            .submitting
            .lock()
            .expect("add download submitting lock") = false;
        *self.importing.lock().expect("add download importing lock") = false;
        self.render(ui);
    }

    pub fn set_mode(&self, ui: &MainWindow, mode_id: &str) {
        if let Some(mode) = DownloadMode::from_id(mode_id) {
            self.update_form(ui, |state| {
                state.mode = mode;
                state.error_text.clear();
            });
        }
    }

    pub fn set_single_url(&self, ui: &MainWindow, value: String) {
        self.update_form(ui, |state| {
            state.single_url = value;
            state.error_text.clear();
        });
    }

    pub fn set_torrent_url(&self, ui: &MainWindow, value: String) {
        self.update_form(ui, |state| {
            state.torrent_url = value;
            state.error_text.clear();
        });
    }

    pub fn set_single_sha256(&self, ui: &MainWindow, value: String) {
        self.update_form(ui, |state| {
            state.single_sha256 = value;
            state.error_text.clear();
        });
    }

    pub fn set_multi_urls(&self, ui: &MainWindow, value: String) {
        self.update_form(ui, |state| {
            state.multi_urls = ensure_trailing_editable_line(&value);
            state.error_text.clear();
        });
    }

    pub fn set_bulk_urls(&self, ui: &MainWindow, value: String) {
        self.update_form(ui, |state| {
            state.bulk_urls = ensure_trailing_editable_line(&value);
            state.error_text.clear();
        });
    }

    pub fn set_archive_name(&self, ui: &MainWindow, value: String) {
        self.update_form(ui, |state| {
            state.archive_name = normalize_archive_name(&value);
            state.error_text.clear();
        });
    }

    pub fn set_combine_bulk(&self, ui: &MainWindow, value: bool) {
        self.update_form(ui, |state| {
            state.combine_bulk = value;
            state.error_text.clear();
        });
    }

    pub fn apply_imported_torrent(&self, ui: &MainWindow, path: Option<String>) {
        if let Some(path) = path {
            self.update_form(ui, |state| {
                state.mode = DownloadMode::Torrent;
                state.torrent_url = path;
                state.error_text.clear();
            });
        } else {
            self.render(ui);
        }
    }

    fn prepare_submission(&self) -> Result<AddDownloadSubmission, String> {
        let state = self.state.lock().expect("add download state lock").clone();
        let urls = active_download_urls(&state);
        if urls.is_empty() {
            return Err("Add at least one download URL.".into());
        }
        let expected_sha256 = if state.mode == DownloadMode::Single {
            validate_optional_sha256(&state.single_sha256)?
        } else {
            None
        };
        let bulk_archive_name = if state.mode == DownloadMode::Bulk && state.combine_bulk {
            let archive_name = normalize_archive_name(&state.archive_name);
            if archive_name.is_empty() {
                None
            } else {
                Some(archive_name)
            }
        } else {
            None
        };
        Ok(AddDownloadSubmission {
            mode: state.mode,
            urls,
            expected_sha256,
            bulk_archive_name,
        })
    }

    fn set_error(&self, ui: &MainWindow, message: String) {
        self.update_form(ui, |state| {
            state.error_text = message;
        });
    }

    fn set_submitting(&self, ui: &MainWindow, submitting: bool) {
        *self
            .submitting
            .lock()
            .expect("add download submitting lock") = submitting;
        self.render(ui);
    }

    fn set_importing(&self, ui: &MainWindow, importing: bool) {
        *self.importing.lock().expect("add download importing lock") = importing;
        self.render(ui);
    }

    fn update_form(&self, ui: &MainWindow, update: impl FnOnce(&mut AddDownloadFormState)) {
        {
            let mut state = self.state.lock().expect("add download state lock");
            update(&mut state);
        }
        self.render(ui);
    }

    fn render(&self, ui: &MainWindow) {
        let state = self.state.lock().expect("add download state lock").clone();
        let model = add_download_form_model(&state);
        ui.set_add_download_visible(*self.visible.lock().expect("add download visible lock"));
        ui.set_add_download_mode(model.mode_id.into());
        ui.set_add_download_single_url(state.single_url.into());
        ui.set_add_download_torrent_url(state.torrent_url.into());
        ui.set_add_download_single_sha256(state.single_sha256.into());
        ui.set_add_download_multi_urls(state.multi_urls.into());
        ui.set_add_download_bulk_urls(state.bulk_urls.into());
        ui.set_add_download_archive_name(state.archive_name.into());
        ui.set_add_download_combine_bulk(state.combine_bulk);
        ui.set_add_download_submit_label(model.submit_label.into());
        ui.set_add_download_ready_label(model.ready_label.into());
        ui.set_add_download_ready_detail(model.ready_detail.into());
        ui.set_add_download_error_text(state.error_text.into());
        ui.set_add_download_can_submit(model.can_submit);
        ui.set_add_download_submitting(
            *self
                .submitting
                .lock()
                .expect("add download submitting lock"),
        );
        ui.set_add_download_importing(*self.importing.lock().expect("add download importing lock"));
    }
}

impl QueueViewRuntimeState {
    pub fn apply_snapshot_to_main_window(&self, ui: &MainWindow, snapshot: &DesktopSnapshot) {
        *self.snapshot.lock().expect("queue snapshot lock") = Some(snapshot.clone());
        self.render(ui);
    }

    pub fn change_view(&self, ui: &MainWindow, view_id: &str) {
        if let Some(view) = ViewFilter::from_id(view_id) {
            self.update_state(ui, |state| {
                state.view = view;
                state.selection.clear();
            });
        }
    }

    pub fn change_search_query(&self, ui: &MainWindow, query: String) {
        self.update_state(ui, |state| {
            state.search_query = query;
        });
    }

    pub fn toggle_sort_column(&self, ui: &MainWindow, column_id: &str) {
        if let Some(column) = SortColumn::from_id(column_id) {
            self.update_state(ui, |state| {
                state.sort_mode = state.sort_mode.next_for_column(column);
            });
        }
    }

    pub fn select_job(&self, ui: &MainWindow, id: &str) {
        self.update_state(ui, |state| {
            state.selection.select_single(id);
        });
    }

    pub fn toggle_job_selection(&self, ui: &MainWindow, id: &str, selected: bool) {
        self.update_state(ui, |state| {
            state.selection.toggle(id, selected);
        });
    }

    pub fn select_all_visible(&self, ui: &MainWindow, selected: bool) {
        if selected {
            let visible_ids = self.visible_ids();
            self.update_state(ui, |state| {
                state.selection.select_all(&visible_ids);
            });
        } else {
            self.update_state(ui, |state| state.selection.clear());
        }
    }

    pub fn clear_selection(&self, ui: &MainWindow) {
        self.update_state(ui, |state| state.selection.clear());
    }

    pub fn selected_ids(&self) -> Vec<String> {
        self.state
            .lock()
            .expect("queue state lock")
            .selection
            .selected_ids()
    }

    pub fn request_delete_job(&self, ui: &MainWindow, id: &str) {
        let snapshot = self.snapshot.lock().expect("queue snapshot lock").clone();
        let prompt = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.jobs.iter().find(|job| job.id == id).cloned())
            .and_then(|job| delete_prompt_from_jobs(&[job]));
        *self.delete_prompt.lock().expect("delete prompt lock") = prompt;
        self.render_delete_prompt(ui);
    }

    pub fn request_delete_selected(&self, ui: &MainWindow) {
        let selected: BTreeSet<String> = self.selected_ids().into_iter().collect();
        let snapshot = self.snapshot.lock().expect("queue snapshot lock").clone();
        let prompt = snapshot.as_ref().and_then(|snapshot| {
            let jobs = snapshot
                .jobs
                .iter()
                .filter(|job| selected.contains(&job.id))
                .cloned()
                .collect::<Vec<_>>();
            delete_prompt_from_jobs(&jobs)
        });
        *self.delete_prompt.lock().expect("delete prompt lock") = prompt;
        self.render_delete_prompt(ui);
    }

    pub fn set_delete_from_disk(&self, ui: &MainWindow, delete_from_disk: bool) {
        if let Some(prompt) = self
            .delete_prompt
            .lock()
            .expect("delete prompt lock")
            .as_mut()
        {
            prompt.delete_from_disk = delete_from_disk;
        }
        self.render_delete_prompt(ui);
    }

    pub fn cancel_delete_prompt(&self, ui: &MainWindow) {
        *self.delete_prompt.lock().expect("delete prompt lock") = None;
        self.render_delete_prompt(ui);
    }

    pub fn confirm_delete_prompt(&self, ui: &MainWindow) -> Option<(Vec<String>, bool)> {
        let prompt = self
            .delete_prompt
            .lock()
            .expect("delete prompt lock")
            .take();
        let Some(prompt) = prompt else {
            self.render_delete_prompt(ui);
            return None;
        };
        let ids = prompt
            .jobs
            .iter()
            .map(|job| job.id.clone())
            .collect::<Vec<_>>();
        self.clear_selection(ui);
        self.render_delete_prompt(ui);
        Some((ids, prompt.delete_from_disk))
    }

    pub fn request_rename_job(&self, ui: &MainWindow, id: &str) {
        let snapshot = self.snapshot.lock().expect("queue snapshot lock").clone();
        let prompt = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.jobs.iter().find(|job| job.id == id))
            .map(|job| {
                let filename = split_filename(&job.filename);
                RenamePromptState {
                    id: job.id.clone(),
                    original_filename: job.filename.clone(),
                    base_name: filename.base_name,
                    extension: filename.extension,
                }
            });
        *self.rename_prompt.lock().expect("rename prompt lock") = prompt;
        self.render_rename_prompt(ui);
    }

    pub fn set_rename_base_name(&self, ui: &MainWindow, base_name: String) {
        if let Some(prompt) = self
            .rename_prompt
            .lock()
            .expect("rename prompt lock")
            .as_mut()
        {
            prompt.base_name = base_name;
        }
        self.render_rename_prompt(ui);
    }

    pub fn set_rename_extension(&self, ui: &MainWindow, extension: String) {
        if let Some(prompt) = self
            .rename_prompt
            .lock()
            .expect("rename prompt lock")
            .as_mut()
        {
            prompt.extension = extension;
        }
        self.render_rename_prompt(ui);
    }

    pub fn cancel_rename_prompt(&self, ui: &MainWindow) {
        *self.rename_prompt.lock().expect("rename prompt lock") = None;
        self.render_rename_prompt(ui);
    }

    pub fn confirm_rename_prompt(&self, ui: &MainWindow) -> Option<(String, String)> {
        let prompt = self
            .rename_prompt
            .lock()
            .expect("rename prompt lock")
            .clone();
        let Some(prompt) = prompt else {
            self.render_rename_prompt(ui);
            return None;
        };
        let Some(filename) = build_filename(&prompt.base_name, &prompt.extension) else {
            self.render_rename_prompt(ui);
            return None;
        };
        *self.rename_prompt.lock().expect("rename prompt lock") = None;
        self.render_rename_prompt(ui);
        Some((prompt.id, filename))
    }

    pub fn select_job_in_main_window(&self, ui: &MainWindow, id: &str) {
        let snapshot = self.snapshot.lock().expect("queue snapshot lock").clone();
        self.update_state(ui, |state| {
            if let Some(snapshot) = &snapshot {
                if let Some(job) = snapshot.jobs.iter().find(|job| job.id == id) {
                    let visible_now = queue_view_model_from_snapshot(snapshot, state)
                        .rows
                        .iter()
                        .any(|row| row.id == id);
                    if !visible_now {
                        state.view = view_for_job(job);
                        state.search_query.clear();
                    }
                }
            }
            state.selection.select_single(id);
        });
    }

    fn update_state(&self, ui: &MainWindow, update: impl FnOnce(&mut QueueUiState)) {
        {
            let mut state = self.state.lock().expect("queue state lock");
            update(&mut state);
        }
        self.render(ui);
    }

    fn render(&self, ui: &MainWindow) {
        let Some(snapshot) = self.snapshot.lock().expect("queue snapshot lock").clone() else {
            return;
        };
        let mut state = self.state.lock().expect("queue state lock");
        let model = queue_view_model_from_snapshot(&snapshot, &state);
        let visible_ids: Vec<String> = model.rows.iter().map(|row| row.id.clone()).collect();
        state.selection.prune_to_visible(&visible_ids);
        let model = queue_view_model_from_snapshot(&snapshot, &state);
        apply_queue_view_model_to_main_window(ui, model);
        ui.set_status_text(status_text_from_snapshot(&snapshot).into());
        self.render_delete_prompt(ui);
        self.render_rename_prompt(ui);
    }

    fn visible_ids(&self) -> Vec<String> {
        let Some(snapshot) = self.snapshot.lock().expect("queue snapshot lock").clone() else {
            return Vec::new();
        };
        let state = self.state.lock().expect("queue state lock");
        queue_view_model_from_snapshot(&snapshot, &state)
            .rows
            .iter()
            .map(|row| row.id.clone())
            .collect()
    }

    fn render_delete_prompt(&self, ui: &MainWindow) {
        let prompt = self
            .delete_prompt
            .lock()
            .expect("delete prompt lock")
            .clone();
        if let Some(prompt) = prompt {
            ui.set_delete_prompt_visible(true);
            ui.set_delete_prompt_title(prompt.content.title.into());
            ui.set_delete_prompt_description(prompt.content.description.into());
            ui.set_delete_prompt_checkbox_label(prompt.content.checkbox_label.into());
            ui.set_delete_prompt_confirm_label(prompt.content.confirm_label.into());
            ui.set_delete_prompt_selected_summary(prompt.content.selected_summary.into());
            ui.set_delete_prompt_missing_path_label(prompt.content.missing_path_label.into());
            ui.set_delete_prompt_delete_from_disk(prompt.delete_from_disk);
            ui.set_delete_prompt_jobs(ModelRc::from(Rc::new(VecModel::from(
                prompt
                    .jobs
                    .into_iter()
                    .map(|job| crate::DeletePromptJob {
                        id: job.id.into(),
                        filename: job.filename.into(),
                        target_path: job.target_path.into(),
                    })
                    .collect::<Vec<_>>(),
            ))));
        } else {
            ui.set_delete_prompt_visible(false);
            ui.set_delete_prompt_delete_from_disk(false);
            ui.set_delete_prompt_jobs(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
        }
    }

    fn render_rename_prompt(&self, ui: &MainWindow) {
        let prompt = self
            .rename_prompt
            .lock()
            .expect("rename prompt lock")
            .clone();
        if let Some(prompt) = prompt {
            let preview = build_filename(&prompt.base_name, &prompt.extension).unwrap_or_default();
            ui.set_rename_prompt_visible(true);
            ui.set_rename_job_id(prompt.id.into());
            ui.set_rename_original_filename(prompt.original_filename.into());
            ui.set_rename_base_name(prompt.base_name.into());
            ui.set_rename_extension(prompt.extension.into());
            ui.set_rename_can_confirm(!preview.is_empty());
            ui.set_rename_preview_filename(preview.into());
        } else {
            ui.set_rename_prompt_visible(false);
            ui.set_rename_job_id(String::new().into());
            ui.set_rename_original_filename(String::new().into());
            ui.set_rename_base_name(String::new().into());
            ui.set_rename_extension(String::new().into());
            ui.set_rename_preview_filename(String::new().into());
            ui.set_rename_can_confirm(false);
        }
    }
}

#[derive(Clone)]
pub struct MainWindowDispatcher {
    window: slint::Weak<MainWindow>,
    state: SharedState,
    queue_view: QueueViewRuntimeState,
}

impl MainWindowDispatcher {
    pub fn new(window: &MainWindow, state: SharedState, queue_view: QueueViewRuntimeState) -> Self {
        Self {
            window: window.as_weak(),
            state,
            queue_view,
        }
    }
}

impl UiDispatcher for MainWindowDispatcher {
    fn dispatch(&self, action: UiAction) -> Result<(), String> {
        let window = self.window.clone();
        let state = self.state.clone();
        let queue_view = self.queue_view.clone();
        slint::invoke_from_event_loop(move || {
            let Some(ui) = window.upgrade() else {
                return;
            };

            match action {
                UiAction::ApplySnapshot(snapshot) => {
                    queue_view.apply_snapshot_to_main_window(&ui, &snapshot);
                    shell::popups::with_popup_registry(|registry| {
                        registry.apply_snapshot(&snapshot);
                    });
                }
                UiAction::FocusMainWindow => {
                    if let Err(error) = main_window::show_main_window(&ui) {
                        eprintln!("failed to focus main window: {error}");
                    }
                }
                UiAction::FocusJobInMainWindow { id } => {
                    queue_view.select_job_in_main_window(&ui, &id);
                    if let Err(error) = main_window::show_main_window(&ui) {
                        eprintln!("failed to focus main window for job {id}: {error}");
                    }
                }
                UiAction::DownloadPromptChanged(prompt) => {
                    if let Err(error) = shell::popups::with_popup_registry(|registry| {
                        registry.set_download_prompt(prompt.map(|prompt| *prompt))
                    }) {
                        eprintln!("failed to update download prompt window: {error}");
                    }
                }
                UiAction::ShowDownloadPromptWindow => {
                    if let Err(error) = shell::popups::with_popup_registry(|registry| {
                        registry.show_download_prompt_window()
                    }) {
                        eprintln!("failed to show download prompt window: {error}");
                    }
                }
                UiAction::CloseDownloadPromptWindow { remember_position } => {
                    if let Err(error) = shell::popups::with_popup_registry(|registry| {
                        registry.close_download_prompt_window(remember_position)
                    }) {
                        eprintln!("failed to close download prompt window: {error}");
                    }
                }
                UiAction::ShowProgressWindow { id, transfer_kind } => {
                    if let Err(error) = shell::popups::with_popup_registry(|registry| {
                        registry.show_progress_window(id, transfer_kind)
                    }) {
                        eprintln!("failed to show progress window: {error}");
                    }
                }
                UiAction::ShowBatchProgressWindow { batch_id, context } => {
                    if let Err(error) = shell::popups::with_popup_registry(|registry| {
                        registry.show_batch_progress_window(batch_id, context)
                    }) {
                        eprintln!("failed to show batch progress window: {error}");
                    }
                }
                UiAction::HideMainWindow => {
                    if let Err(error) = main_window::hide_main_window(&ui) {
                        eprintln!("failed to hide main window: {error}");
                    }
                }
                UiAction::RequestExit => {
                    if let Err(error) = main_window::request_exit(&ui, &state) {
                        eprintln!("failed to exit application: {error}");
                    }
                }
                UiAction::ApplyUpdateState(update_state) => {
                    apply_update_state_to_main_window(&ui, &update_state);
                }
            }
        })
        .map_err(|error| error.to_string())
    }
}

pub fn ui_action_for_tray_action(action: shell::tray::TrayAction) -> UiAction {
    match action {
        shell::tray::TrayAction::OpenMainWindow => UiAction::FocusMainWindow,
        shell::tray::TrayAction::ExitApplication => UiAction::RequestExit,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainWindowLifecycleCommand {
    Minimize,
    ToggleMaximize,
    CloseToTray,
    StartDrag,
}

pub trait MainWindowLifecycleSink: 'static {
    fn run_main_window_lifecycle_command(&self, command: MainWindowLifecycleCommand);
}

#[derive(Clone)]
pub struct MainWindowLifecycleController {
    window: slint::Weak<MainWindow>,
    state: SharedState,
}

impl MainWindowLifecycleController {
    pub fn new(window: &MainWindow, state: SharedState) -> Self {
        Self {
            window: window.as_weak(),
            state,
        }
    }
}

impl MainWindowLifecycleSink for MainWindowLifecycleController {
    fn run_main_window_lifecycle_command(&self, command: MainWindowLifecycleCommand) {
        let Some(ui) = self.window.upgrade() else {
            return;
        };

        match command {
            MainWindowLifecycleCommand::Minimize => {
                main_window::minimize_main_window(&ui);
            }
            MainWindowLifecycleCommand::ToggleMaximize => {
                main_window::toggle_main_window_maximized(&ui);
            }
            MainWindowLifecycleCommand::CloseToTray => {
                if let Err(error) = main_window::close_main_window_to_tray(&ui, &self.state) {
                    eprintln!("failed to close main window to tray: {error}");
                }
            }
            MainWindowLifecycleCommand::StartDrag => {
                if let Err(error) = main_window::start_main_window_drag(&ui) {
                    eprintln!("failed to start main window drag: {error}");
                }
            }
        }
    }
}

pub fn wire_main_window_lifecycle_callbacks<C>(ui: &MainWindow, sink: Arc<C>)
where
    C: MainWindowLifecycleSink,
{
    let minimize_sink = sink.clone();
    ui.on_minimize_main_window_requested(move || {
        minimize_sink.run_main_window_lifecycle_command(MainWindowLifecycleCommand::Minimize);
    });

    let maximize_sink = sink.clone();
    ui.on_toggle_main_window_maximize_requested(move || {
        maximize_sink.run_main_window_lifecycle_command(MainWindowLifecycleCommand::ToggleMaximize);
    });

    let close_sink = sink.clone();
    ui.on_close_main_window_requested(move || {
        close_sink.run_main_window_lifecycle_command(MainWindowLifecycleCommand::CloseToTray);
    });

    let drag_sink = sink.clone();
    ui.on_start_main_window_drag_requested(move || {
        drag_sink.run_main_window_lifecycle_command(MainWindowLifecycleCommand::StartDrag);
    });

    ui.on_titlebar_double_clicked(move || {
        sink.run_main_window_lifecycle_command(MainWindowLifecycleCommand::ToggleMaximize);
    });
}

#[derive(Clone)]
pub struct SlintShellServices<D>
where
    D: UiDispatcher,
{
    dispatcher: D,
    progress_batches: ProgressBatchRegistry,
    pending_selected_job: shell::popups::PendingSelectedJob,
    update_state: UpdateStateStore,
    pending_update: Arc<PendingUpdateState>,
}

impl<D> SlintShellServices<D>
where
    D: UiDispatcher,
{
    pub fn new(dispatcher: D) -> Self {
        Self::with_progress_batches(dispatcher, ProgressBatchRegistry::default())
    }

    pub fn with_progress_batches(dispatcher: D, progress_batches: ProgressBatchRegistry) -> Self {
        Self::with_update_state(
            dispatcher,
            progress_batches,
            UpdateStateStore::default(),
            Arc::new(PendingUpdateState::default()),
        )
    }

    pub fn with_update_state(
        dispatcher: D,
        progress_batches: ProgressBatchRegistry,
        update_state: UpdateStateStore,
        pending_update: Arc<PendingUpdateState>,
    ) -> Self {
        Self {
            dispatcher,
            progress_batches,
            pending_selected_job: shell::popups::PendingSelectedJob::default(),
            update_state,
            pending_update,
        }
    }
}

impl<D> ShellServices for SlintShellServices<D>
where
    D: UiDispatcher,
{
    fn emit_event(&self, event: DesktopEvent) -> BackendFuture<'_, ()> {
        let dispatcher = self.dispatcher.clone();
        Box::pin(async move {
            match event {
                DesktopEvent::StateChanged(snapshot) => {
                    dispatcher.dispatch(UiAction::ApplySnapshot(snapshot))?;
                }
                DesktopEvent::DownloadPromptChanged(prompt) => {
                    dispatcher.dispatch(UiAction::DownloadPromptChanged(prompt))?;
                }
                DesktopEvent::SelectJobRequested(id) => {
                    dispatcher.dispatch(UiAction::FocusJobInMainWindow { id })?;
                }
                DesktopEvent::UpdateInstallProgress(event) => {
                    let next = self
                        .update_state
                        .update(|state| update::apply_install_progress_event(state, event));
                    dispatcher.dispatch(UiAction::ApplyUpdateState(Box::new(next)))?;
                }
                DesktopEvent::ShellError(error) => {
                    eprintln!("shell error during {}: {}", error.operation, error.message);
                }
            }
            Ok(())
        })
    }

    fn notify(&self, title: String, body: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                shell::notifications::show_notification(&title, &body)
            })
            .await
            .map_err(|error| format!("Could not start notification task: {error}"))?
        })
    }

    fn focus_main_window(&self) -> BackendFuture<'_, ()> {
        let dispatcher = self.dispatcher.clone();
        Box::pin(async move {
            dispatcher.dispatch(UiAction::FocusMainWindow)?;
            Ok(())
        })
    }

    fn show_download_prompt_window(&self) -> BackendFuture<'_, ()> {
        let dispatcher = self.dispatcher.clone();
        Box::pin(async move {
            dispatcher.dispatch(UiAction::ShowDownloadPromptWindow)?;
            Ok(())
        })
    }

    fn close_download_prompt_window(&self, remember_position: bool) -> BackendFuture<'_, ()> {
        let dispatcher = self.dispatcher.clone();
        Box::pin(async move {
            dispatcher.dispatch(UiAction::CloseDownloadPromptWindow { remember_position })?;
            Ok(())
        })
    }

    fn focus_job_in_main_window(&self, id: String) -> BackendFuture<'_, ()> {
        let dispatcher = self.dispatcher.clone();
        let pending_selected_job = self.pending_selected_job.clone();
        Box::pin(async move {
            pending_selected_job.queue(id.clone());
            dispatcher.dispatch(UiAction::FocusJobInMainWindow { id })?;
            Ok(())
        })
    }

    fn take_pending_selected_job_request(&self) -> BackendFuture<'_, Option<String>> {
        let pending_selected_job = self.pending_selected_job.clone();
        Box::pin(async move { Ok(pending_selected_job.take()) })
    }

    fn show_progress_window(
        &self,
        id: String,
        transfer_kind: TransferKind,
    ) -> BackendFuture<'_, ()> {
        let dispatcher = self.dispatcher.clone();
        Box::pin(async move {
            dispatcher.dispatch(UiAction::ShowProgressWindow { id, transfer_kind })?;
            Ok(())
        })
    }

    fn show_batch_progress_window(&self, batch_id: String) -> BackendFuture<'_, ()> {
        let dispatcher = self.dispatcher.clone();
        let context = self.progress_batches.get(&batch_id);
        Box::pin(async move {
            dispatcher.dispatch(UiAction::ShowBatchProgressWindow { batch_id, context })?;
            Ok(())
        })
    }

    fn browse_directory(&self) -> BackendFuture<'_, Option<String>> {
        Box::pin(async {
            tokio::task::spawn_blocking(shell::windows::browse_directory)
                .await
                .map_err(|error| format!("Could not open folder picker: {error}"))?
        })
    }

    fn browse_torrent_file(&self) -> BackendFuture<'_, Option<String>> {
        Box::pin(async {
            tokio::task::spawn_blocking(shell::windows::browse_torrent_file)
                .await
                .map_err(|error| format!("Could not open torrent picker: {error}"))?
        })
    }

    fn save_diagnostics_report(&self, report: String) -> BackendFuture<'_, Option<String>> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || shell::windows::save_diagnostics_report(report))
                .await
                .map_err(|error| format!("Could not open save dialog: {error}"))?
        })
    }

    fn gather_host_registration_diagnostics(
        &self,
    ) -> BackendFuture<'_, HostRegistrationDiagnostics> {
        Box::pin(async {
            tokio::task::spawn_blocking(shell::native_host::gather_host_registration_diagnostics)
                .await
                .map_err(|error| {
                    format!("Could not gather native host registration diagnostics: {error}")
                })?
        })
    }

    fn sync_autostart_setting(&self, enabled: bool) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || shell::windows::sync_autostart_setting(enabled))
                .await
                .map_err(|error| format!("Could not sync startup registration: {error}"))?
        })
    }

    fn schedule_downloads(&self, state: SharedState) -> BackendFuture<'_, ()> {
        let shell = TransferShell::new(self.clone());
        Box::pin(async move { transfer::schedule_downloads(shell, state).await })
    }

    fn forget_torrent_session_for_restart(&self, torrent: TorrentInfo) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            transfer::forget_known_torrent_sessions(&[torrent]).await?;
            Ok(())
        })
    }

    fn forget_known_torrent_sessions(&self, torrents: Vec<TorrentInfo>) -> BackendFuture<'_, ()> {
        Box::pin(async move { transfer::forget_known_torrent_sessions(&torrents).await })
    }

    fn apply_torrent_runtime_settings(&self, settings: TorrentSettings) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            transfer::apply_torrent_runtime_settings(&settings);
            Ok(())
        })
    }

    fn schedule_external_reseed(&self, state: SharedState, id: String) -> BackendFuture<'_, ()> {
        let shell = TransferShell::new(self.clone());
        Box::pin(async move {
            transfer::schedule_external_reseed(shell, state, id).await;
            Ok(())
        })
    }

    fn open_url(&self, url: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || shell::windows::open_url(&url))
                .await
                .map_err(|error| format!("Could not open URL: {error}"))?
        })
    }

    fn open_path(&self, path: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                shell::windows::open_path(std::path::Path::new(&path))
            })
            .await
            .map_err(|error| format!("Could not open file: {error}"))?
        })
    }

    fn reveal_path(&self, path: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                shell::windows::reveal_path(std::path::Path::new(&path))
            })
            .await
            .map_err(|error| format!("Could not reveal file: {error}"))?
        })
    }

    fn open_install_docs(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            tokio::task::spawn_blocking(shell::windows::open_install_docs)
                .await
                .map_err(|error| format!("Could not open install docs: {error}"))?
        })
    }

    fn run_host_registration_fix(&self) -> BackendFuture<'_, ()> {
        Box::pin(async {
            tokio::task::spawn_blocking(shell::native_host::register_native_host)
                .await
                .map_err(|error| format!("Could not start host registration: {error}"))?
        })
    }

    fn test_extension_handoff(
        &self,
        state: SharedState,
        prompts: PromptRegistry,
    ) -> BackendFuture<'_, ()> {
        Box::pin(async move { run_test_extension_handoff(self.clone(), state, prompts).await })
    }

    fn check_for_update(&self) -> BackendFuture<'_, Option<AppUpdateMetadata>> {
        let pending_update = self.pending_update.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || update::check_for_update(&pending_update))
                .await
                .map_err(|error| format!("Could not start update check: {error}"))?
                .map_err(|error| error.message)
        })
    }

    fn install_update(&self) -> BackendFuture<'_, ()> {
        let pending_update = self.pending_update.clone();
        let update_state = self.update_state.clone();
        let dispatcher = self.dispatcher.clone();
        Box::pin(async move {
            let callback_state = update_state.clone();
            let callback_dispatcher = dispatcher.clone();
            let result = tokio::task::spawn_blocking(move || {
                update::install_update_with_progress(&pending_update, move |event| {
                    let next = callback_state
                        .update(|state| update::apply_install_progress_event(state, event));
                    let _ =
                        callback_dispatcher.dispatch(UiAction::ApplyUpdateState(Box::new(next)));
                })
            })
            .await
            .map_err(|error| format!("Could not start update install: {error}"))?;

            if let Err(error) = result {
                let next = update_state
                    .update(|state| update::fail_update_install(state, error.message.clone()));
                dispatcher.dispatch(UiAction::ApplyUpdateState(Box::new(next)))?;
                return Err(error.message);
            }

            Ok(())
        })
    }

    fn close_to_tray(&self) -> BackendFuture<'_, ()> {
        let dispatcher = self.dispatcher.clone();
        Box::pin(async move {
            dispatcher.dispatch(UiAction::HideMainWindow)?;
            Ok(())
        })
    }

    fn request_exit(&self) -> BackendFuture<'_, ()> {
        let dispatcher = self.dispatcher.clone();
        Box::pin(async move {
            dispatcher.dispatch(UiAction::RequestExit)?;
            Ok(())
        })
    }
}

async fn run_test_extension_handoff<D>(
    shell: SlintShellServices<D>,
    state: SharedState,
    prompts: PromptRegistry,
) -> Result<(), String>
where
    D: UiDispatcher,
{
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
    shell.show_download_prompt_window().await?;
    if let Some(active_prompt) = prompts.active_prompt().await {
        shell
            .emit_event(DesktopEvent::DownloadPromptChanged(Some(Box::new(
                active_prompt,
            ))))
            .await?;
    }

    let worker_shell = shell.clone();
    let worker_state = state.clone();
    tokio::spawn(async move {
        let decision = receiver.await.unwrap_or(PromptDecision::SwapToBrowser);
        match decision {
            PromptDecision::Cancel | PromptDecision::SwapToBrowser => {}
            PromptDecision::ShowExisting => {
                if let Some(job) = prompt.duplicate_job {
                    let _ = worker_shell.focus_job_in_main_window(job.id).await;
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
                    Err(error) => {
                        eprintln!("test extension handoff failed: {}", error.message);
                        return;
                    }
                };
                let result = worker_state
                    .enqueue_download_with_options(
                        prompt.url,
                        EnqueueOptions {
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
                        if let Err(error) = worker_shell
                            .emit_event(DesktopEvent::StateChanged(Box::new(
                                result.snapshot.clone(),
                            )))
                            .await
                        {
                            eprintln!("test extension handoff snapshot failed: {error}");
                        }
                        if result.status == EnqueueStatus::Queued {
                            if show_progress {
                                let transfer_kind = result
                                    .snapshot
                                    .jobs
                                    .iter()
                                    .find(|job| job.id == result.job_id)
                                    .map(|job| job.transfer_kind)
                                    .unwrap_or_default();
                                let _ = worker_shell
                                    .show_progress_window(result.job_id.clone(), transfer_kind)
                                    .await;
                            }
                            let _ = worker_shell.schedule_downloads(worker_state).await;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueCommand {
    Pause(String),
    Resume(String),
    Cancel(String),
    Retry(String),
    Restart(String),
    Remove(String),
    DeleteMany {
        ids: Vec<String>,
        delete_from_disk: bool,
    },
    Rename {
        id: String,
        filename: String,
    },
    OpenFile(String),
    RevealInFolder(String),
    SwapFailedToBrowser(String),
    OpenProgress(String),
    PauseAll,
    ResumeAll,
    RetryFailed,
    ClearCompleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCommand {
    Check(UpdateCheckMode),
    Install,
}

pub trait QueueCommandSink: Send + Sync + 'static {
    fn run_queue_command(&self, command: QueueCommand) -> BackendFuture<'_, ()>;
}

pub trait AddDownloadCommandSink: Send + Sync + 'static {
    fn add_download_job(&self, request: AddJobRequest) -> BackendFuture<'_, AddJobResult>;
    fn add_download_jobs(&self, request: AddJobsRequest) -> BackendFuture<'_, AddJobsResult>;
    fn browse_torrent_file_for_add_download(&self) -> BackendFuture<'_, Option<String>>;
    fn open_add_download_progress_window(&self, id: String) -> BackendFuture<'_, ()>;
    fn open_add_download_batch_progress_window(
        &self,
        context: ProgressBatchContext,
    ) -> BackendFuture<'_, String>;
}

pub trait UpdateCommandSink: Send + Sync + 'static {
    fn run_update_command(
        &self,
        command: UpdateCommand,
    ) -> BackendFuture<'_, Option<AppUpdateMetadata>>;
}

type SlintStringCallback = Box<dyn FnMut(slint::SharedString)>;
type StringCommandRegistrar = fn(&MainWindow, SlintStringCallback);

impl<S> QueueCommandSink for CoreDesktopBackend<S>
where
    S: ShellServices + 'static,
{
    fn run_queue_command(&self, command: QueueCommand) -> BackendFuture<'_, ()> {
        match command {
            QueueCommand::Pause(id) => self.pause_job(id),
            QueueCommand::Resume(id) => self.resume_job(id),
            QueueCommand::Cancel(id) => self.cancel_job(id),
            QueueCommand::Retry(id) => self.retry_job(id),
            QueueCommand::Restart(id) => self.restart_job(id),
            QueueCommand::Remove(id) => self.remove_job(id),
            QueueCommand::DeleteMany {
                ids,
                delete_from_disk,
            } => Box::pin(async move {
                for id in ids {
                    self.delete_job(id, delete_from_disk).await?;
                }
                Ok(())
            }),
            QueueCommand::Rename { id, filename } => self.rename_job(id, filename),
            QueueCommand::OpenFile(id) => Box::pin(async move {
                self.open_job_file(id).await?;
                Ok(())
            }),
            QueueCommand::RevealInFolder(id) => Box::pin(async move {
                self.reveal_job_in_folder(id).await?;
                Ok(())
            }),
            QueueCommand::SwapFailedToBrowser(id) => self.swap_failed_download_to_browser(id),
            QueueCommand::OpenProgress(id) => self.open_progress_window(id),
            QueueCommand::PauseAll => self.pause_all_jobs(),
            QueueCommand::ResumeAll => self.resume_all_jobs(),
            QueueCommand::RetryFailed => self.retry_failed_jobs(),
            QueueCommand::ClearCompleted => self.clear_completed_jobs(),
        }
    }
}

impl<S> AddDownloadCommandSink for CoreDesktopBackend<S>
where
    S: ShellServices + 'static,
{
    fn add_download_job(&self, request: AddJobRequest) -> BackendFuture<'_, AddJobResult> {
        self.add_job(request)
    }

    fn add_download_jobs(&self, request: AddJobsRequest) -> BackendFuture<'_, AddJobsResult> {
        self.add_jobs(request)
    }

    fn browse_torrent_file_for_add_download(&self) -> BackendFuture<'_, Option<String>> {
        self.browse_torrent_file()
    }

    fn open_add_download_progress_window(&self, id: String) -> BackendFuture<'_, ()> {
        self.open_progress_window(id)
    }

    fn open_add_download_batch_progress_window(
        &self,
        context: ProgressBatchContext,
    ) -> BackendFuture<'_, String> {
        self.open_batch_progress_window(context)
    }
}

impl<S> UpdateCommandSink for CoreDesktopBackend<S>
where
    S: ShellServices + 'static,
{
    fn run_update_command(
        &self,
        command: UpdateCommand,
    ) -> BackendFuture<'_, Option<AppUpdateMetadata>> {
        match command {
            UpdateCommand::Check(_) => self.check_for_update(),
            UpdateCommand::Install => Box::pin(async move {
                self.install_update().await?;
                Ok(None)
            }),
        }
    }
}

pub struct SlintRuntime<D>
where
    D: UiDispatcher,
{
    runtime: Arc<Runtime>,
    backend: Arc<SlintBackend<D>>,
    dispatcher: D,
}

impl<D> SlintRuntime<D>
where
    D: UiDispatcher,
{
    pub fn backend(&self) -> Arc<SlintBackend<D>> {
        self.backend.clone()
    }

    pub fn runtime(&self) -> Arc<Runtime> {
        self.runtime.clone()
    }

    pub fn dispatcher(&self) -> D {
        self.dispatcher.clone()
    }
}

pub fn run_app() -> Result<(), String> {
    let state = SharedState::new()?;
    let Some(_single_instance_guard) = lifecycle::acquire_single_instance_or_notify()? else {
        return Ok(());
    };
    let startup_window_action =
        main_window::current_startup_window_action(state.settings_sync().startup_launch_mode);

    let ui = MainWindow::new().map_err(|error| error.to_string())?;
    let runtime = bootstrap_main_window_with_state(&ui, state)?;
    let dispatcher = runtime.dispatcher();
    let _tray = shell::tray::create_system_tray(move |action| {
        if let Err(error) = dispatcher.dispatch(ui_action_for_tray_action(action)) {
            eprintln!("failed to dispatch tray action: {error}");
        }
    })?;
    crate::ipc::start_named_pipe_listener(runtime.runtime(), runtime.backend());
    #[cfg(windows)]
    {
        let backend_runtime = runtime.runtime();
        backend_runtime.spawn(async {
            let result =
                tokio::task::spawn_blocking(shell::native_host::ensure_native_host_registration)
                    .await
                    .map_err(|error| format!("Could not start native host registration: {error}"))
                    .and_then(|result| result);

            if let Err(error) = result {
                eprintln!("native host auto-registration failed: {error}");
            }
        });
    }
    main_window::apply_startup_window_action(&ui, startup_window_action)?;
    slint::run_event_loop_until_quit().map_err(|error| error.to_string())
}

pub fn bootstrap_main_window(
    ui: &MainWindow,
) -> Result<SlintRuntime<MainWindowDispatcher>, String> {
    let state = SharedState::new()?;
    bootstrap_main_window_with_state(ui, state)
}

pub fn bootstrap_main_window_with_state(
    ui: &MainWindow,
    state: SharedState,
) -> Result<SlintRuntime<MainWindowDispatcher>, String> {
    main_window::initialize_main_window(ui, &state);

    let runtime = Arc::new(
        Builder::new_multi_thread()
            .enable_all()
            .thread_name("sdm-slint-backend")
            .build()
            .map_err(|error| format!("Could not initialize Slint backend runtime: {error}"))?,
    );
    let queue_view = QueueViewRuntimeState::default();
    let dispatcher = MainWindowDispatcher::new(ui, state.clone(), queue_view.clone());
    let progress_batches = ProgressBatchRegistry::default();
    let update_state = UpdateStateStore::default();
    let pending_update = Arc::new(PendingUpdateState::default());
    let shell = SlintShellServices::with_update_state(
        dispatcher.clone(),
        progress_batches.clone(),
        update_state.clone(),
        pending_update,
    );
    let backend = Arc::new(CoreDesktopBackend::new(
        state.clone(),
        PromptRegistry::default(),
        progress_batches,
        shell.clone(),
    ));

    let snapshot = runtime.block_on(backend.get_app_snapshot())?;
    queue_view.apply_snapshot_to_main_window(ui, &snapshot);
    let add_download_state = AddDownloadRuntimeState::default();
    wire_main_window_lifecycle_callbacks(
        ui,
        Arc::new(MainWindowLifecycleController::new(ui, state.clone())),
    );
    wire_queue_command_callbacks(ui, runtime.clone(), backend.clone(), queue_view.clone());
    wire_add_download_callbacks(
        ui,
        runtime.clone(),
        backend.clone(),
        queue_view,
        add_download_state,
    );
    wire_update_callbacks(ui, runtime.clone(), backend.clone(), update_state.clone());
    start_startup_update_check(runtime.clone(), backend.clone(), update_state, ui.as_weak());

    let startup_shell = shell.clone();
    runtime.spawn(async move {
        if let Err(error) = startup_shell.schedule_downloads(state).await {
            eprintln!("failed to schedule persisted downloads: {error}");
        }
    });

    Ok(SlintRuntime {
        runtime,
        backend,
        dispatcher,
    })
}

pub fn apply_snapshot_to_main_window(ui: &MainWindow, snapshot: &DesktopSnapshot) {
    QueueViewRuntimeState::default().apply_snapshot_to_main_window(ui, snapshot);
}

pub fn apply_queue_view_model_to_main_window(
    ui: &MainWindow,
    model: crate::controller::QueueViewModel,
) {
    let rows = model
        .rows
        .into_iter()
        .map(slint_job_row_from_row)
        .collect::<Vec<_>>();
    let nav_items = model
        .nav_items
        .into_iter()
        .map(slint_nav_item_from_item)
        .collect::<Vec<_>>();

    ui.set_jobs(ModelRc::from(Rc::new(VecModel::from(rows))));
    ui.set_nav_items(ModelRc::from(Rc::new(VecModel::from(nav_items))));
    ui.set_queue_view_id(model.view_id.into());
    ui.set_queue_title(model.title.into());
    ui.set_queue_subtitle(model.subtitle.into());
    ui.set_queue_footer_text(model.footer_text.into());
    ui.set_queue_empty_text(model.empty_text.into());
    ui.set_queue_search_query(model.search_query.into());
    ui.set_queue_selected_count(model.selected_count as i32);
    ui.set_queue_all_visible_selected(model.all_visible_selected);
    ui.set_queue_has_visible_selection(model.has_visible_selection);
    ui.set_queue_sort_column(model.sort_mode.column.id().into());
    ui.set_queue_sort_direction(model.sort_mode.direction.id().into());
}

pub fn apply_update_state_to_main_window(ui: &MainWindow, state: &AppUpdateState) {
    let current_version = state
        .available_update
        .as_ref()
        .map(|update| update.current_version.as_str())
        .unwrap_or(env!("CARGO_PKG_VERSION"));
    let latest_version = state
        .available_update
        .as_ref()
        .map(|update| update.version.as_str())
        .unwrap_or_else(|| match state.status.as_str() {
            "checking" => "Checking...",
            "not_available" => current_version,
            _ => "Unknown",
        });
    let status_text = update_status_text(state);
    let progress_text = if state.status == "downloading" {
        update::format_update_progress(state.downloaded_bytes, state.total_bytes)
    } else {
        String::new()
    };

    ui.set_update_status_text(status_text.into());
    ui.set_update_current_version(current_version.into());
    ui.set_update_new_version(latest_version.into());
    ui.set_update_body(
        state
            .available_update
            .as_ref()
            .and_then(|update| update.body.as_deref())
            .unwrap_or_default()
            .into(),
    );
    ui.set_update_error_text(state.error_message.clone().unwrap_or_default().into());
    ui.set_update_progress_text(progress_text.into());
    ui.set_update_progress(
        update::progress_percent(state.downloaded_bytes, state.total_bytes) as f32,
    );
    ui.set_update_can_check(!matches!(
        state.status.as_str(),
        "checking" | "downloading" | "installing"
    ));
    ui.set_update_can_install(state.status == "available" && state.available_update.is_some());
}

fn update_status_text(state: &AppUpdateState) -> String {
    match state.status.as_str() {
        "checking" => "Checking for updates...".into(),
        "available" => state
            .available_update
            .as_ref()
            .map(|update| format!("Update {} is ready.", update.version))
            .unwrap_or_else(|| "Update is ready.".into()),
        "not_available" => "You are running the latest version.".into(),
        "downloading" => "Downloading update...".into(),
        "installing" => "Installing update...".into(),
        "error" => "Update failed.".into(),
        _ => "Updates idle".into(),
    }
}

pub fn wire_queue_command_callbacks<C>(
    ui: &MainWindow,
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    queue_view: QueueViewRuntimeState,
) where
    C: QueueCommandSink,
{
    let view_state = queue_view.clone();
    let view_window = ui.as_weak();
    ui.on_view_change_requested(move |view_id| {
        if let Some(ui) = view_window.upgrade() {
            view_state.change_view(&ui, view_id.as_str());
        }
    });

    let search_state = queue_view.clone();
    let search_window = ui.as_weak();
    ui.on_search_query_changed(move |query| {
        if let Some(ui) = search_window.upgrade() {
            search_state.change_search_query(&ui, query.to_string());
        }
    });

    let sort_state = queue_view.clone();
    let sort_window = ui.as_weak();
    ui.on_sort_column_requested(move |column| {
        if let Some(ui) = sort_window.upgrade() {
            sort_state.toggle_sort_column(&ui, column.as_str());
        }
    });

    let select_state = queue_view.clone();
    let select_window = ui.as_weak();
    ui.on_job_selection_requested(move |id| {
        if let Some(ui) = select_window.upgrade() {
            select_state.select_job(&ui, id.as_str());
        }
    });

    let toggle_state = queue_view.clone();
    let toggle_window = ui.as_weak();
    ui.on_job_selection_toggled(move |id, selected| {
        if let Some(ui) = toggle_window.upgrade() {
            toggle_state.toggle_job_selection(&ui, id.as_str(), selected);
        }
    });

    let select_all_state = queue_view.clone();
    let select_all_window = ui.as_weak();
    ui.on_select_all_visible_requested(move |selected| {
        if let Some(ui) = select_all_window.upgrade() {
            select_all_state.select_all_visible(&ui, selected);
        }
    });

    let clear_state = queue_view.clone();
    let clear_window = ui.as_weak();
    ui.on_clear_selection_requested(move || {
        if let Some(ui) = clear_window.upgrade() {
            clear_state.clear_selection(&ui);
        }
    });

    let delete_job_state = queue_view.clone();
    let delete_job_window = ui.as_weak();
    ui.on_request_delete_job(move |id| {
        if let Some(ui) = delete_job_window.upgrade() {
            delete_job_state.request_delete_job(&ui, id.as_str());
        }
    });

    let delete_selected_state = queue_view.clone();
    let delete_selected_window = ui.as_weak();
    ui.on_request_delete_selected(move || {
        if let Some(ui) = delete_selected_window.upgrade() {
            delete_selected_state.request_delete_selected(&ui);
        }
    });

    let delete_from_disk_state = queue_view.clone();
    let delete_from_disk_window = ui.as_weak();
    ui.on_delete_from_disk_changed(move |delete_from_disk| {
        if let Some(ui) = delete_from_disk_window.upgrade() {
            delete_from_disk_state.set_delete_from_disk(&ui, delete_from_disk);
        }
    });

    let delete_cancel_state = queue_view.clone();
    let delete_cancel_window = ui.as_weak();
    ui.on_delete_cancelled(move || {
        if let Some(ui) = delete_cancel_window.upgrade() {
            delete_cancel_state.cancel_delete_prompt(&ui);
        }
    });

    let delete_confirm_state = queue_view.clone();
    let delete_confirm_window = ui.as_weak();
    let delete_confirm_runtime = runtime.clone();
    let delete_confirm_sink = command_sink.clone();
    ui.on_delete_confirmed(move || {
        let Some(ui) = delete_confirm_window.upgrade() else {
            return;
        };
        let Some((ids, delete_from_disk)) = delete_confirm_state.confirm_delete_prompt(&ui) else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        let command_sink = delete_confirm_sink.clone();
        delete_confirm_runtime.spawn(async move {
            if let Err(error) = command_sink
                .run_queue_command(QueueCommand::DeleteMany {
                    ids,
                    delete_from_disk,
                })
                .await
            {
                eprintln!("queue delete command failed: {error}");
            }
        });
    });

    let rename_request_state = queue_view.clone();
    let rename_request_window = ui.as_weak();
    ui.on_request_rename_job(move |id| {
        if let Some(ui) = rename_request_window.upgrade() {
            rename_request_state.request_rename_job(&ui, id.as_str());
        }
    });

    let rename_base_state = queue_view.clone();
    let rename_base_window = ui.as_weak();
    ui.on_rename_base_name_changed(move |base_name| {
        if let Some(ui) = rename_base_window.upgrade() {
            rename_base_state.set_rename_base_name(&ui, base_name.to_string());
        }
    });

    let rename_extension_state = queue_view.clone();
    let rename_extension_window = ui.as_weak();
    ui.on_rename_extension_changed(move |extension| {
        if let Some(ui) = rename_extension_window.upgrade() {
            rename_extension_state.set_rename_extension(&ui, extension.to_string());
        }
    });

    let rename_cancel_state = queue_view.clone();
    let rename_cancel_window = ui.as_weak();
    ui.on_rename_cancelled(move || {
        if let Some(ui) = rename_cancel_window.upgrade() {
            rename_cancel_state.cancel_rename_prompt(&ui);
        }
    });

    let rename_confirm_state = queue_view.clone();
    let rename_confirm_window = ui.as_weak();
    let rename_confirm_runtime = runtime.clone();
    let rename_confirm_sink = command_sink.clone();
    ui.on_rename_confirmed(move || {
        let Some(ui) = rename_confirm_window.upgrade() else {
            return;
        };
        let Some((id, filename)) = rename_confirm_state.confirm_rename_prompt(&ui) else {
            return;
        };
        let command_sink = rename_confirm_sink.clone();
        rename_confirm_runtime.spawn(async move {
            if let Err(error) = command_sink
                .run_queue_command(QueueCommand::Rename { id, filename })
                .await
            {
                eprintln!("queue rename command failed: {error}");
            }
        });
    });

    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_pause_job_requested,
        QueueCommand::Pause,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_resume_job_requested,
        QueueCommand::Resume,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_cancel_job_requested,
        QueueCommand::Cancel,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_retry_job_requested,
        QueueCommand::Retry,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_restart_job_requested,
        QueueCommand::Restart,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_open_progress_requested,
        QueueCommand::OpenProgress,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_open_job_file_requested,
        QueueCommand::OpenFile,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_reveal_job_requested,
        QueueCommand::RevealInFolder,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_swap_failed_to_browser_requested,
        QueueCommand::SwapFailedToBrowser,
    );
    wire_void_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_pause_all_requested,
        QueueCommand::PauseAll,
    );
    wire_void_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_resume_all_requested,
        QueueCommand::ResumeAll,
    );
    wire_void_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_retry_failed_requested,
        QueueCommand::RetryFailed,
    );
    wire_void_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        MainWindow::on_clear_completed_requested,
        QueueCommand::ClearCompleted,
    );
}

pub fn wire_add_download_callbacks<C>(
    ui: &MainWindow,
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    queue_view: QueueViewRuntimeState,
    add_state: AddDownloadRuntimeState,
) where
    C: AddDownloadCommandSink,
{
    add_state.render(ui);

    let open_state = add_state.clone();
    let open_window = ui.as_weak();
    ui.on_add_download_requested(move || {
        if let Some(ui) = open_window.upgrade() {
            open_state.open(&ui);
        }
    });

    let cancel_state = add_state.clone();
    let cancel_window = ui.as_weak();
    ui.on_add_download_cancelled(move || {
        if let Some(ui) = cancel_window.upgrade() {
            cancel_state.close(&ui);
        }
    });

    let mode_state = add_state.clone();
    let mode_window = ui.as_weak();
    ui.on_add_download_mode_changed(move |mode| {
        if let Some(ui) = mode_window.upgrade() {
            mode_state.set_mode(&ui, mode.as_str());
        }
    });

    let single_url_state = add_state.clone();
    let single_url_window = ui.as_weak();
    ui.on_add_download_single_url_changed(move |value| {
        if let Some(ui) = single_url_window.upgrade() {
            single_url_state.set_single_url(&ui, value.to_string());
        }
    });

    let torrent_url_state = add_state.clone();
    let torrent_url_window = ui.as_weak();
    ui.on_add_download_torrent_url_changed(move |value| {
        if let Some(ui) = torrent_url_window.upgrade() {
            torrent_url_state.set_torrent_url(&ui, value.to_string());
        }
    });

    let sha_state = add_state.clone();
    let sha_window = ui.as_weak();
    ui.on_add_download_single_sha256_changed(move |value| {
        if let Some(ui) = sha_window.upgrade() {
            sha_state.set_single_sha256(&ui, value.to_string());
        }
    });

    let multi_state = add_state.clone();
    let multi_window = ui.as_weak();
    ui.on_add_download_multi_urls_changed(move |value| {
        if let Some(ui) = multi_window.upgrade() {
            multi_state.set_multi_urls(&ui, value.to_string());
        }
    });

    let bulk_state = add_state.clone();
    let bulk_window = ui.as_weak();
    ui.on_add_download_bulk_urls_changed(move |value| {
        if let Some(ui) = bulk_window.upgrade() {
            bulk_state.set_bulk_urls(&ui, value.to_string());
        }
    });

    let archive_state = add_state.clone();
    let archive_window = ui.as_weak();
    ui.on_add_download_archive_name_changed(move |value| {
        if let Some(ui) = archive_window.upgrade() {
            archive_state.set_archive_name(&ui, value.to_string());
        }
    });

    let combine_state = add_state.clone();
    let combine_window = ui.as_weak();
    ui.on_add_download_combine_bulk_changed(move |value| {
        if let Some(ui) = combine_window.upgrade() {
            combine_state.set_combine_bulk(&ui, value);
        }
    });

    let import_state = add_state.clone();
    let import_window = ui.as_weak();
    let import_runtime = runtime.clone();
    let import_sink = command_sink.clone();
    ui.on_add_download_import_torrent_requested(move || {
        let Some(ui) = import_window.upgrade() else {
            return;
        };
        import_state.set_importing(&ui, true);
        let window = import_window.clone();
        let add_state = import_state.clone();
        let command_sink = import_sink.clone();
        import_runtime.spawn(async move {
            match command_sink.browse_torrent_file_for_add_download().await {
                Ok(path) => dispatch_add_download_import_to_weak(window, add_state, path),
                Err(error) => {
                    dispatch_add_download_error_to_weak(window, add_state, error, false, true)
                }
            }
        });
    });

    let submit_state = add_state.clone();
    let submit_window = ui.as_weak();
    let submit_runtime = runtime;
    ui.on_add_download_submit_requested(move || {
        let Some(ui) = submit_window.upgrade() else {
            return;
        };
        let submission = match submit_state.prepare_submission() {
            Ok(submission) => submission,
            Err(error) => {
                submit_state.set_error(&ui, error);
                return;
            }
        };
        submit_state.set_submitting(&ui, true);
        let command_sink = command_sink.clone();
        let window = submit_window.clone();
        let add_state = submit_state.clone();
        let queue_view = queue_view.clone();
        submit_runtime.spawn(async move {
            match run_add_download_submission(command_sink, submission).await {
                Ok(outcome) => {
                    dispatch_add_download_success_to_weak(window, add_state, queue_view, outcome)
                }
                Err(error) => {
                    dispatch_add_download_error_to_weak(window, add_state, error, true, false)
                }
            }
        });
    });
}

pub fn wire_update_callbacks<C>(
    ui: &MainWindow,
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    update_state: UpdateStateStore,
) where
    C: UpdateCommandSink,
{
    apply_update_state_to_main_window(ui, &update_state.snapshot());

    let check_window = ui.as_weak();
    let check_state = update_state.clone();
    let check_runtime = runtime.clone();
    let check_sink = command_sink.clone();
    ui.on_check_update_requested(move || {
        let next =
            check_state.update(|state| update::start_update_check(state, UpdateCheckMode::Manual));
        apply_update_state_to_weak(&check_window, &next);

        let window = check_window.clone();
        let update_state = check_state.clone();
        let command_sink = check_sink.clone();
        check_runtime.spawn(async move {
            match command_sink
                .run_update_command(UpdateCommand::Check(UpdateCheckMode::Manual))
                .await
            {
                Ok(update) => {
                    let next =
                        update_state.update(|state| update::finish_update_check(state, update));
                    dispatch_update_state_to_weak(window, next);
                }
                Err(error) => {
                    let next = update_state.update(|state| update::fail_update_check(state, error));
                    dispatch_update_state_to_weak(window, next);
                }
            }
        });
    });

    let install_window = ui.as_weak();
    let install_state = update_state;
    let install_runtime = runtime;
    ui.on_install_update_requested(move || {
        let next = install_state.update(update::begin_update_install);
        apply_update_state_to_weak(&install_window, &next);

        let window = install_window.clone();
        let update_state = install_state.clone();
        let command_sink = command_sink.clone();
        install_runtime.spawn(async move {
            if let Err(error) = command_sink
                .run_update_command(UpdateCommand::Install)
                .await
            {
                let next = update_state.update(|state| update::fail_update_install(state, error));
                dispatch_update_state_to_weak(window, next);
            }
        });
    });
}

async fn run_add_download_submission<C>(
    command_sink: Arc<C>,
    submission: AddDownloadSubmission,
) -> Result<crate::controller::AddDownloadOutcome, String>
where
    C: AddDownloadCommandSink,
{
    match submission.mode {
        DownloadMode::Single | DownloadMode::Torrent => {
            let url = submission
                .urls
                .first()
                .cloned()
                .ok_or_else(|| "Add at least one download URL.".to_string())?;
            let result = command_sink
                .add_download_job(AddJobRequest {
                    url,
                    directory_override: None,
                    filename_hint: None,
                    expected_sha256: submission.expected_sha256,
                    transfer_kind: Some(if submission.mode == DownloadMode::Torrent {
                        TransferKind::Torrent
                    } else {
                        TransferKind::Http
                    }),
                })
                .await?;
            let outcome = add_download_outcome_for_result(
                submission.mode,
                AddDownloadResult::Single(result),
                None,
            );
            if let Some(AddDownloadProgressIntent::Single { job_id }) = &outcome.progress_intent {
                command_sink
                    .open_add_download_progress_window(job_id.clone())
                    .await?;
            }
            Ok(outcome)
        }
        DownloadMode::Multi | DownloadMode::Bulk => {
            let archive_name = submission.bulk_archive_name.clone();
            let result = command_sink
                .add_download_jobs(AddJobsRequest {
                    urls: submission.urls,
                    bulk_archive_name: archive_name.clone(),
                })
                .await?;
            let outcome = add_download_outcome_for_result(
                submission.mode,
                AddDownloadResult::Batch(result),
                archive_name.as_deref(),
            );
            if let Some(AddDownloadProgressIntent::Batch { context }) = &outcome.progress_intent {
                command_sink
                    .open_add_download_batch_progress_window(context.clone())
                    .await?;
            }
            Ok(outcome)
        }
    }
}

fn dispatch_add_download_import_to_weak(
    window: slint::Weak<MainWindow>,
    add_state: AddDownloadRuntimeState,
    path: Option<String>,
) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            add_state.set_importing(&ui, false);
            add_state.apply_imported_torrent(&ui, path);
        }
    }) {
        eprintln!("failed to update Slint add-download import state: {error}");
    }
}

fn dispatch_add_download_success_to_weak(
    window: slint::Weak<MainWindow>,
    add_state: AddDownloadRuntimeState,
    queue_view: QueueViewRuntimeState,
    outcome: crate::controller::AddDownloadOutcome,
) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            queue_view.change_view(&ui, outcome.view_id);
            if let Some(id) = outcome.primary_job_id {
                queue_view.select_job_in_main_window(&ui, &id);
            }
            add_state.close(&ui);
        }
    }) {
        eprintln!("failed to update Slint add-download success state: {error}");
    }
}

fn dispatch_add_download_error_to_weak(
    window: slint::Weak<MainWindow>,
    add_state: AddDownloadRuntimeState,
    error: String,
    submitting: bool,
    importing: bool,
) {
    if let Err(dispatch_error) = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            if submitting {
                add_state.set_submitting(&ui, false);
            }
            if importing {
                add_state.set_importing(&ui, false);
            }
            add_state.set_error(&ui, error);
        }
    }) {
        eprintln!("failed to update Slint add-download error state: {dispatch_error}");
    }
}

fn start_startup_update_check<C>(
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    update_state: UpdateStateStore,
    window: slint::Weak<MainWindow>,
) where
    C: UpdateCommandSink,
{
    let next =
        update_state.update(|state| update::start_update_check(state, UpdateCheckMode::Startup));
    apply_update_state_to_weak(&window, &next);

    runtime.spawn(async move {
        match command_sink
            .run_update_command(UpdateCommand::Check(UpdateCheckMode::Startup))
            .await
        {
            Ok(update) => {
                let next = update_state.update(|state| update::finish_update_check(state, update));
                dispatch_update_state_to_weak(window, next);
            }
            Err(_) => {
                let next = update_state.update(update::finish_silent_startup_update_failure);
                dispatch_update_state_to_weak(window, next);
            }
        }
    });
}

fn apply_update_state_to_weak(window: &slint::Weak<MainWindow>, state: &AppUpdateState) {
    if let Some(ui) = window.upgrade() {
        apply_update_state_to_main_window(&ui, state);
    }
}

fn dispatch_update_state_to_weak(window: slint::Weak<MainWindow>, state: AppUpdateState) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        apply_update_state_to_weak(&window, &state);
    }) {
        eprintln!("failed to update Slint updater panel: {error}");
    }
}

fn wire_string_command<C>(
    ui: &MainWindow,
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    register: StringCommandRegistrar,
    command: fn(String) -> QueueCommand,
) where
    C: QueueCommandSink,
{
    register(
        ui,
        Box::new(move |id| {
            let command_sink = command_sink.clone();
            let command = command(id.to_string());
            runtime.spawn(async move {
                if let Err(error) = command_sink.run_queue_command(command).await {
                    eprintln!("queue command failed: {error}");
                }
            });
        }),
    );
}

fn wire_void_command<C>(
    ui: &MainWindow,
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    register: fn(&MainWindow, Box<dyn FnMut()>),
    command: QueueCommand,
) where
    C: QueueCommandSink,
{
    register(
        ui,
        Box::new(move || {
            let command_sink = command_sink.clone();
            let command = command.clone();
            runtime.spawn(async move {
                if let Err(error) = command_sink.run_queue_command(command).await {
                    eprintln!("queue command failed: {error}");
                }
            });
        }),
    );
}
