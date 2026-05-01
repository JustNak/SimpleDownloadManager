use crate::controller::{
    active_download_urls, add_download_form_model, add_download_outcome_for_result,
    add_excluded_hosts, build_filename, default_torrent_download_directory,
    delete_prompt_from_jobs, diagnostics_view_model_from_snapshot, ensure_trailing_editable_line,
    external_use_auto_reseed_message, format_diagnostics_report, handoff_mode_from_id,
    normalize_accent_color, normalize_archive_name, normalize_torrent_settings,
    parse_excluded_host_input, performance_mode_from_id, queue_row_size_from_id,
    queue_view_model_from_snapshot, remove_excluded_host, settings_view_model_from_state,
    slint_diagnostic_event_from_row, slint_host_registration_entry_from_row,
    slint_job_row_from_row, slint_nav_item_from_item, slint_settings_nav_item_from_item,
    slint_toast_from_message, slint_torrent_diagnostic_from_row, split_filename,
    startup_launch_mode_from_id, status_text_from_snapshot, theme_from_id, toast_for_shell_error,
    toast_message, torrent_peer_watchdog_mode_from_id, torrent_seed_mode_from_id,
    validate_optional_sha256, view_for_job, AddDownloadFormState, AddDownloadProgressIntent,
    AddDownloadResult, DeletePromptDetails, DownloadMode, QueueUiState, SettingsDraftState,
    SettingsSection, SortColumn, ToastMessage, ToastType, ViewFilter, TOAST_AUTO_CLOSE_MS,
};
use crate::shell::{self, lifecycle, main_window};
use crate::update::{self, AppUpdateState, PendingUpdateState, UpdateCheckMode, UpdateStateStore};
use crate::MainWindow;
use simple_download_manager_desktop_core::backend::{
    prompt_enqueue_details, CoreDesktopBackend, ProgressBatchRegistry,
};
use simple_download_manager_desktop_core::contracts::{
    AddJobRequest, AddJobResult, AddJobsRequest, AddJobsResult, AppUpdateMetadata, BackendFuture,
    ConfirmPromptRequest, DesktopBackend, DesktopEvent, ExternalUseResult, ProgressBatchContext,
    ShellServices,
};
use simple_download_manager_desktop_core::prompts::{PromptDecision, PromptRegistry};
use simple_download_manager_desktop_core::state::{
    EnqueueOptions, EnqueueStatus, SharedState, TorrentSessionCacheClearResult,
};
use simple_download_manager_desktop_core::storage::{
    DesktopSnapshot, DiagnosticsSnapshot, DownloadPrompt, DownloadSource,
    HostRegistrationDiagnostics, Settings, TorrentInfo, TorrentSettings, TransferKind,
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
    ShowToast(Box<ToastMessage>),
}

pub trait UiDispatcher: Clone + Send + Sync + 'static {
    fn dispatch(&self, action: UiAction) -> Result<(), String>;
}

#[derive(Clone, Default)]
pub struct ToastRuntimeState {
    state: Arc<Mutex<ToastRuntimeInner>>,
}

#[derive(Default)]
struct ToastRuntimeInner {
    next_id: u64,
    toasts: Vec<ToastMessage>,
}

impl ToastRuntimeState {
    pub fn add_toast(&self, ui: &MainWindow, mut toast: ToastMessage) -> String {
        let id = {
            let mut state = self.state.lock().expect("toast state lock");
            if toast.id.is_empty() {
                state.next_id += 1;
                toast.id = format!("toast_{}", state.next_id);
            }
            let id = toast.id.clone();
            state.toasts.push(toast.clone());
            id
        };
        self.render(ui);
        if toast.auto_close {
            self.schedule_auto_close(ui.as_weak(), id.clone());
        }
        id
    }

    pub fn dismiss_toast(&self, ui: &MainWindow, id: &str) {
        self.state
            .lock()
            .expect("toast state lock")
            .toasts
            .retain(|toast| toast.id != id);
        self.render(ui);
    }

    pub fn render(&self, ui: &MainWindow) {
        let toasts = self
            .state
            .lock()
            .expect("toast state lock")
            .toasts
            .clone()
            .into_iter()
            .map(slint_toast_from_message)
            .collect::<Vec<_>>();
        ui.set_toasts(ModelRc::from(Rc::new(VecModel::from(toasts))));
    }

    pub fn toasts(&self) -> Vec<ToastMessage> {
        self.state.lock().expect("toast state lock").toasts.clone()
    }

    fn schedule_auto_close(&self, window: slint::Weak<MainWindow>, id: String) {
        let state = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(TOAST_AUTO_CLOSE_MS));
            let dispatch_id = id.clone();
            if let Err(error) = slint::invoke_from_event_loop(move || {
                if let Some(ui) = window.upgrade() {
                    state.dismiss_toast(&ui, &dispatch_id);
                }
            }) {
                eprintln!("failed to auto-dismiss Slint toast {id}: {error}");
            }
        });
    }
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

#[derive(Clone, Default)]
pub struct SettingsRuntimeState {
    state: Arc<Mutex<SettingsDraftState>>,
    section_observer: Arc<Mutex<Option<SettingsSectionObserver>>>,
}

type SettingsSectionObserver = Arc<dyn Fn(SettingsSection) + Send + Sync>;

impl SettingsRuntimeState {
    pub fn apply_snapshot_to_main_window(&self, ui: &MainWindow, snapshot: &DesktopSnapshot) {
        {
            let mut state = self.state.lock().expect("settings state lock");
            state.adopt_incoming_settings(snapshot.settings.clone());
        }
        self.render(ui);
    }

    pub fn open(&self, ui: &MainWindow) {
        {
            let mut state = self.state.lock().expect("settings state lock");
            state.visible = true;
            state.unsaved_prompt_visible = false;
            state.error_text.clear();
        }
        self.render(ui);
    }

    pub fn request_close(&self, ui: &MainWindow) {
        {
            let mut state = self.state.lock().expect("settings state lock");
            if state.dirty() {
                state.unsaved_prompt_visible = true;
            } else {
                state.visible = false;
                state.unsaved_prompt_visible = false;
            }
        }
        self.render(ui);
    }

    pub fn cancel_unsaved_prompt(&self, ui: &MainWindow) {
        {
            let mut state = self.state.lock().expect("settings state lock");
            state.unsaved_prompt_visible = false;
        }
        self.render(ui);
    }

    pub fn discard_and_close(&self, ui: &MainWindow) {
        {
            let mut state = self.state.lock().expect("settings state lock");
            state.discard();
            state.visible = false;
        }
        self.render(ui);
    }

    pub fn change_section(&self, ui: &MainWindow, section_id: &str) {
        if let Some(section) = SettingsSection::from_id(section_id) {
            self.update(ui, |state| {
                state.active_section = section;
                state.unsaved_prompt_visible = false;
            });
            let observer = self
                .section_observer
                .lock()
                .expect("settings section observer lock")
                .clone();
            if let Some(observer) = observer {
                observer(section);
            }
        }
    }

    pub fn set_section_observer(&self, observer: impl Fn(SettingsSection) + Send + Sync + 'static) {
        *self
            .section_observer
            .lock()
            .expect("settings section observer lock") = Some(Arc::new(observer));
    }

    pub fn apply_saved_settings(&self, ui: &MainWindow, settings: Settings) {
        {
            let mut state = self.state.lock().expect("settings state lock");
            state.saved = settings.clone();
            state.draft = settings;
            state.visible = false;
            state.saving = false;
            state.unsaved_prompt_visible = false;
            state.error_text.clear();
        }
        self.render(ui);
    }

    pub fn draft_for_save(&self) -> Settings {
        let mut settings = self
            .state
            .lock()
            .expect("settings state lock")
            .draft
            .clone();
        settings.accent_color = normalize_accent_color(&settings.accent_color);
        settings.torrent =
            normalize_torrent_settings(settings.torrent.clone(), &settings.download_directory);
        settings
    }

    pub fn set_saving(&self, ui: &MainWindow, saving: bool) {
        self.update(ui, |state| state.saving = saving);
    }

    pub fn set_cache_clearing(&self, ui: &MainWindow, clearing: bool) {
        self.update(ui, |state| state.cache_clearing = clearing);
    }

    pub fn set_error(&self, ui: &MainWindow, error: String) {
        self.update(ui, |state| {
            state.error_text = error;
            state.saving = false;
            state.cache_clearing = false;
        });
    }

    pub fn set_download_directory(&self, ui: &MainWindow, directory: String) {
        self.update_draft(ui, |settings| {
            update_download_directory_with_torrent_default(settings, directory);
        });
    }

    pub fn set_torrent_download_directory(&self, ui: &MainWindow, directory: String) {
        self.update_draft(ui, |settings| {
            settings.torrent.download_directory = directory;
        });
    }

    pub fn set_max_concurrent_downloads(&self, ui: &MainWindow, value: String) {
        self.update_draft_u32(ui, value, 1, 64, |settings, value| {
            settings.max_concurrent_downloads = value
        });
    }

    pub fn set_auto_retry_attempts(&self, ui: &MainWindow, value: String) {
        self.update_draft_u32(ui, value, 0, 25, |settings, value| {
            settings.auto_retry_attempts = value
        });
    }

    pub fn set_speed_limit_kib_per_second(&self, ui: &MainWindow, value: String) {
        self.update_draft_u32(ui, value, 0, 1_048_576, |settings, value| {
            settings.speed_limit_kib_per_second = value
        });
    }

    pub fn set_download_performance_mode(&self, ui: &MainWindow, value: &str) {
        if let Some(mode) = performance_mode_from_id(value) {
            self.update_draft(ui, |settings| settings.download_performance_mode = mode);
        }
    }

    pub fn set_notifications_enabled(&self, ui: &MainWindow, value: bool) {
        self.update_draft(ui, |settings| settings.notifications_enabled = value);
    }

    pub fn set_show_details_on_click(&self, ui: &MainWindow, value: bool) {
        self.update_draft(ui, |settings| settings.show_details_on_click = value);
    }

    pub fn set_queue_row_size(&self, ui: &MainWindow, value: &str) {
        if let Some(size) = queue_row_size_from_id(value) {
            self.update_draft(ui, |settings| settings.queue_row_size = size);
        }
    }

    pub fn set_start_on_startup(&self, ui: &MainWindow, value: bool) {
        self.update_draft(ui, |settings| settings.start_on_startup = value);
    }

    pub fn set_startup_launch_mode(&self, ui: &MainWindow, value: &str) {
        if let Some(mode) = startup_launch_mode_from_id(value) {
            self.update_draft(ui, |settings| settings.startup_launch_mode = mode);
        }
    }

    pub fn set_theme(&self, ui: &MainWindow, value: &str) {
        if let Some(theme) = theme_from_id(value) {
            self.update_draft(ui, |settings| settings.theme = theme);
        }
    }

    pub fn set_accent_color(&self, ui: &MainWindow, value: String) {
        self.update_draft(ui, |settings| {
            settings.accent_color = normalize_accent_color(&value)
        });
    }

    pub fn set_torrent_enabled(&self, ui: &MainWindow, value: bool) {
        self.update_draft(ui, |settings| settings.torrent.enabled = value);
    }

    pub fn set_torrent_seed_mode(&self, ui: &MainWindow, value: &str) {
        if let Some(mode) = torrent_seed_mode_from_id(value) {
            self.update_draft(ui, |settings| settings.torrent.seed_mode = mode);
        }
    }

    pub fn set_torrent_seed_ratio_limit(&self, ui: &MainWindow, value: String) {
        if let Ok(parsed) = value.trim().parse::<f64>() {
            self.update_draft(ui, |settings| settings.torrent.seed_ratio_limit = parsed);
        }
    }

    pub fn set_torrent_seed_time_limit_minutes(&self, ui: &MainWindow, value: String) {
        self.update_draft_u32(ui, value, 1, 525_600, |settings, value| {
            settings.torrent.seed_time_limit_minutes = value
        });
    }

    pub fn set_torrent_upload_limit_kib_per_second(&self, ui: &MainWindow, value: String) {
        self.update_draft_u32(ui, value, 0, 1_048_576, |settings, value| {
            settings.torrent.upload_limit_kib_per_second = value
        });
    }

    pub fn set_torrent_port_forwarding_enabled(&self, ui: &MainWindow, value: bool) {
        self.update_draft(ui, |settings| {
            settings.torrent.port_forwarding_enabled = value
        });
    }

    pub fn set_torrent_port_forwarding_port(&self, ui: &MainWindow, value: String) {
        self.update_draft_u32(ui, value, 1, 65_535, |settings, value| {
            settings.torrent.port_forwarding_port = value
        });
    }

    pub fn set_torrent_peer_watchdog_mode(&self, ui: &MainWindow, value: &str) {
        if let Some(mode) = torrent_peer_watchdog_mode_from_id(value) {
            self.update_draft(ui, |settings| {
                settings.torrent.peer_connection_watchdog_mode = mode
            });
        }
    }

    pub fn set_extension_enabled(&self, ui: &MainWindow, value: bool) {
        self.update_draft(ui, |settings| {
            settings.extension_integration.enabled = value
        });
    }

    pub fn set_extension_handoff_mode(&self, ui: &MainWindow, value: &str) {
        if let Some(mode) = handoff_mode_from_id(value) {
            self.update_draft(ui, |settings| {
                settings.extension_integration.download_handoff_mode = mode
            });
        }
    }

    pub fn set_extension_listen_port(&self, ui: &MainWindow, value: String) {
        self.update_draft_u32(ui, value, 1, 65_535, |settings, value| {
            settings.extension_integration.listen_port = value
        });
    }

    pub fn set_extension_context_menu_enabled(&self, ui: &MainWindow, value: bool) {
        self.update_draft(ui, |settings| {
            settings.extension_integration.context_menu_enabled = value
        });
    }

    pub fn set_extension_show_progress_after_handoff(&self, ui: &MainWindow, value: bool) {
        self.update_draft(ui, |settings| {
            settings.extension_integration.show_progress_after_handoff = value
        });
    }

    pub fn set_extension_show_badge_status(&self, ui: &MainWindow, value: bool) {
        self.update_draft(ui, |settings| {
            settings.extension_integration.show_badge_status = value
        });
    }

    pub fn set_extension_authenticated_handoff_enabled(&self, ui: &MainWindow, value: bool) {
        self.update_draft(ui, |settings| {
            settings.extension_integration.authenticated_handoff_enabled = value
        });
    }

    pub fn set_excluded_host_input(&self, ui: &MainWindow, value: String) {
        self.update(ui, |state| state.excluded_host_input = value);
    }

    pub fn add_excluded_hosts_from_input(&self, ui: &MainWindow) {
        self.update(ui, |state| {
            let candidates = parse_excluded_host_input(&state.excluded_host_input);
            let result = add_excluded_hosts(
                state.draft.extension_integration.excluded_hosts.clone(),
                candidates,
            );
            state.draft.extension_integration.excluded_hosts = result.hosts;
            state.excluded_host_input.clear();
        });
    }

    pub fn remove_excluded_host(&self, ui: &MainWindow, host: String) {
        self.update_draft(ui, |settings| {
            settings.extension_integration.excluded_hosts =
                remove_excluded_host(&settings.extension_integration.excluded_hosts, &host);
        });
    }

    fn update_draft(&self, ui: &MainWindow, update: impl FnOnce(&mut Settings)) {
        self.update(ui, |state| {
            update(&mut state.draft);
            state.error_text.clear();
            state.unsaved_prompt_visible = false;
        });
    }

    fn update_draft_u32(
        &self,
        ui: &MainWindow,
        value: String,
        min: u32,
        max: u32,
        update: impl FnOnce(&mut Settings, u32),
    ) {
        if let Ok(parsed) = value.trim().parse::<u32>() {
            self.update_draft(ui, |settings| update(settings, parsed.max(min).min(max)));
        }
    }

    fn update(&self, ui: &MainWindow, update: impl FnOnce(&mut SettingsDraftState)) {
        {
            let mut state = self.state.lock().expect("settings state lock");
            update(&mut state);
        }
        self.render(ui);
    }

    fn render(&self, ui: &MainWindow) {
        let state = self.state.lock().expect("settings state lock").clone();
        apply_settings_view_model_to_main_window(ui, settings_view_model_from_state(&state));
    }
}

#[derive(Clone, Default)]
pub struct DiagnosticsRuntimeState {
    state: Arc<Mutex<DiagnosticsUiState>>,
}

#[derive(Debug, Clone, Default)]
struct DiagnosticsUiState {
    diagnostics: Option<DiagnosticsSnapshot>,
    loading: bool,
    action_status_text: String,
    error_text: String,
}

impl DiagnosticsRuntimeState {
    pub fn render(&self, ui: &MainWindow) {
        let state = self.state.lock().expect("diagnostics state lock").clone();
        apply_diagnostics_view_model_to_main_window(
            ui,
            diagnostics_view_model_from_snapshot(
                state.diagnostics.as_ref(),
                state.loading,
                &state.action_status_text,
                &state.error_text,
            ),
        );
    }

    pub fn needs_refresh(&self) -> bool {
        self.state
            .lock()
            .expect("diagnostics state lock")
            .diagnostics
            .is_none()
    }

    pub fn current_report(&self) -> Option<String> {
        self.state
            .lock()
            .expect("diagnostics state lock")
            .diagnostics
            .as_ref()
            .map(format_diagnostics_report)
    }

    fn set_loading(&self, ui: &MainWindow, loading: bool) {
        {
            let mut state = self.state.lock().expect("diagnostics state lock");
            state.loading = loading;
            if loading {
                state.error_text.clear();
            }
        }
        self.render(ui);
    }

    fn apply_snapshot(&self, ui: &MainWindow, diagnostics: DiagnosticsSnapshot, status: String) {
        {
            let mut state = self.state.lock().expect("diagnostics state lock");
            state.diagnostics = Some(diagnostics);
            state.loading = false;
            state.action_status_text = status;
            state.error_text.clear();
        }
        self.render(ui);
    }

    fn set_action_status(&self, ui: &MainWindow, status: String) {
        {
            let mut state = self.state.lock().expect("diagnostics state lock");
            state.loading = false;
            state.action_status_text = status;
            state.error_text.clear();
        }
        self.render(ui);
    }

    fn set_error(&self, ui: &MainWindow, error: String) {
        {
            let mut state = self.state.lock().expect("diagnostics state lock");
            state.loading = false;
            state.error_text = error;
        }
        self.render(ui);
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
    settings_state: SettingsRuntimeState,
    toast_state: ToastRuntimeState,
}

impl MainWindowDispatcher {
    pub fn new(
        window: &MainWindow,
        state: SharedState,
        queue_view: QueueViewRuntimeState,
        settings_state: SettingsRuntimeState,
        toast_state: ToastRuntimeState,
    ) -> Self {
        Self {
            window: window.as_weak(),
            state,
            queue_view,
            settings_state,
            toast_state,
        }
    }
}

impl UiDispatcher for MainWindowDispatcher {
    fn dispatch(&self, action: UiAction) -> Result<(), String> {
        let window = self.window.clone();
        let state = self.state.clone();
        let queue_view = self.queue_view.clone();
        let settings_state = self.settings_state.clone();
        let toast_state = self.toast_state.clone();
        slint::invoke_from_event_loop(move || {
            let Some(ui) = window.upgrade() else {
                return;
            };

            match action {
                UiAction::ApplySnapshot(snapshot) => {
                    queue_view.apply_snapshot_to_main_window(&ui, &snapshot);
                    settings_state.apply_snapshot_to_main_window(&ui, &snapshot);
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
                UiAction::ShowToast(toast) => {
                    toast_state.add_toast(&ui, *toast);
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
                    dispatcher.dispatch(UiAction::ShowToast(Box::new(toast_for_shell_error(
                        &error.operation,
                        &error.message,
                    ))))?;
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueCommandOutput {
    pub external_use: Option<ExternalUseResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCommand {
    Check(UpdateCheckMode),
    Install,
}

pub trait QueueCommandSink: Send + Sync + 'static {
    fn run_queue_command(&self, command: QueueCommand) -> BackendFuture<'_, QueueCommandOutput>;
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

pub trait SettingsCommandSink: Send + Sync + 'static {
    fn save_settings_from_slint(&self, settings: Settings) -> BackendFuture<'_, Settings>;
    fn browse_settings_directory(&self) -> BackendFuture<'_, Option<String>>;
    fn clear_settings_torrent_session_cache(
        &self,
    ) -> BackendFuture<'_, TorrentSessionCacheClearResult>;
}

pub trait DiagnosticsCommandSink: Send + Sync + 'static {
    fn get_diagnostics_for_slint(&self) -> BackendFuture<'_, DiagnosticsSnapshot>;
    fn export_diagnostics_report_from_slint(&self) -> BackendFuture<'_, Option<String>>;
    fn copy_diagnostics_report_from_slint(&self, report: String) -> BackendFuture<'_, ()>;
    fn open_install_docs_from_slint(&self) -> BackendFuture<'_, ()>;
    fn repair_host_registration_from_slint(&self) -> BackendFuture<'_, ()>;
    fn test_extension_handoff_from_slint(&self) -> BackendFuture<'_, ()>;
}

pub trait PromptWindowCommandSink: Send + Sync + 'static {
    fn browse_prompt_directory(&self) -> BackendFuture<'_, Option<String>>;
    fn confirm_prompt_from_slint(&self, request: ConfirmPromptRequest) -> BackendFuture<'_, ()>;
    fn cancel_prompt_from_slint(&self, id: String) -> BackendFuture<'_, ()>;
    fn swap_prompt_from_slint(&self, id: String) -> BackendFuture<'_, ()>;
}

pub trait ProgressPopupCommandSink: Send + Sync + 'static {
    fn run_progress_popup_action(
        &self,
        action: shell::popups::ProgressPopupAction,
    ) -> BackendFuture<'_, ()>;
}

pub trait UpdateCommandSink: Send + Sync + 'static {
    fn run_update_command(
        &self,
        command: UpdateCommand,
    ) -> BackendFuture<'_, Option<AppUpdateMetadata>>;
}

type SlintStringCallback = Box<dyn FnMut(slint::SharedString)>;
type SlintBoolCallback = Box<dyn FnMut(bool)>;
type StringCommandRegistrar = fn(&MainWindow, SlintStringCallback);
type BoolSettingsRegistrar = fn(&MainWindow, SlintBoolCallback);

impl<S> QueueCommandSink for CoreDesktopBackend<S>
where
    S: ShellServices + 'static,
{
    fn run_queue_command(&self, command: QueueCommand) -> BackendFuture<'_, QueueCommandOutput> {
        match command {
            QueueCommand::Pause(id) => Box::pin(async move {
                self.pause_job(id).await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::Resume(id) => Box::pin(async move {
                self.resume_job(id).await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::Cancel(id) => Box::pin(async move {
                self.cancel_job(id).await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::Retry(id) => Box::pin(async move {
                self.retry_job(id).await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::Restart(id) => Box::pin(async move {
                self.restart_job(id).await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::Remove(id) => Box::pin(async move {
                self.remove_job(id).await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::DeleteMany {
                ids,
                delete_from_disk,
            } => Box::pin(async move {
                for id in ids {
                    self.delete_job(id, delete_from_disk).await?;
                }
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::Rename { id, filename } => Box::pin(async move {
                self.rename_job(id, filename).await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::OpenFile(id) => Box::pin(async move {
                let result = self.open_job_file(id).await?;
                Ok(QueueCommandOutput {
                    external_use: Some(result),
                })
            }),
            QueueCommand::RevealInFolder(id) => Box::pin(async move {
                let result = self.reveal_job_in_folder(id).await?;
                Ok(QueueCommandOutput {
                    external_use: Some(result),
                })
            }),
            QueueCommand::SwapFailedToBrowser(id) => Box::pin(async move {
                self.swap_failed_download_to_browser(id).await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::OpenProgress(id) => Box::pin(async move {
                self.open_progress_window(id).await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::PauseAll => Box::pin(async move {
                self.pause_all_jobs().await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::ResumeAll => Box::pin(async move {
                self.resume_all_jobs().await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::RetryFailed => Box::pin(async move {
                self.retry_failed_jobs().await?;
                Ok(QueueCommandOutput::default())
            }),
            QueueCommand::ClearCompleted => Box::pin(async move {
                self.clear_completed_jobs().await?;
                Ok(QueueCommandOutput::default())
            }),
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

impl<S> SettingsCommandSink for CoreDesktopBackend<S>
where
    S: ShellServices + 'static,
{
    fn save_settings_from_slint(&self, settings: Settings) -> BackendFuture<'_, Settings> {
        self.save_settings(settings)
    }

    fn browse_settings_directory(&self) -> BackendFuture<'_, Option<String>> {
        self.browse_directory()
    }

    fn clear_settings_torrent_session_cache(
        &self,
    ) -> BackendFuture<'_, TorrentSessionCacheClearResult> {
        self.clear_torrent_session_cache()
    }
}

impl<S> DiagnosticsCommandSink for CoreDesktopBackend<S>
where
    S: ShellServices + 'static,
{
    fn get_diagnostics_for_slint(&self) -> BackendFuture<'_, DiagnosticsSnapshot> {
        self.get_diagnostics()
    }

    fn export_diagnostics_report_from_slint(&self) -> BackendFuture<'_, Option<String>> {
        self.export_diagnostics_report()
    }

    fn copy_diagnostics_report_from_slint(&self, report: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || shell::clipboard::write_text(&report))
                .await
                .map_err(|error| format!("Could not start clipboard copy: {error}"))?
        })
    }

    fn open_install_docs_from_slint(&self) -> BackendFuture<'_, ()> {
        self.open_install_docs()
    }

    fn repair_host_registration_from_slint(&self) -> BackendFuture<'_, ()> {
        self.run_host_registration_fix()
    }

    fn test_extension_handoff_from_slint(&self) -> BackendFuture<'_, ()> {
        self.test_extension_handoff()
    }
}

impl<S> PromptWindowCommandSink for CoreDesktopBackend<S>
where
    S: ShellServices + 'static,
{
    fn browse_prompt_directory(&self) -> BackendFuture<'_, Option<String>> {
        self.browse_directory()
    }

    fn confirm_prompt_from_slint(&self, request: ConfirmPromptRequest) -> BackendFuture<'_, ()> {
        self.confirm_download_prompt(request)
    }

    fn cancel_prompt_from_slint(&self, id: String) -> BackendFuture<'_, ()> {
        self.cancel_download_prompt(id)
    }

    fn swap_prompt_from_slint(&self, id: String) -> BackendFuture<'_, ()> {
        self.swap_download_prompt(id)
    }
}

impl<S> ProgressPopupCommandSink for CoreDesktopBackend<S>
where
    S: ShellServices + 'static,
{
    fn run_progress_popup_action(
        &self,
        action: shell::popups::ProgressPopupAction,
    ) -> BackendFuture<'_, ()> {
        match action {
            shell::popups::ProgressPopupAction::Pause(id) => self.pause_job(id),
            shell::popups::ProgressPopupAction::Resume(id) => self.resume_job(id),
            shell::popups::ProgressPopupAction::Retry(id) => self.retry_job(id),
            shell::popups::ProgressPopupAction::Cancel(id) => self.cancel_job(id),
            shell::popups::ProgressPopupAction::OpenFile(id) => Box::pin(async move {
                self.open_job_file(id).await?;
                Ok(())
            }),
            shell::popups::ProgressPopupAction::RevealInFolder(id) => Box::pin(async move {
                self.reveal_job_in_folder(id).await?;
                Ok(())
            }),
            shell::popups::ProgressPopupAction::SwapFailedToBrowser(id) => {
                self.swap_failed_download_to_browser(id)
            }
            shell::popups::ProgressPopupAction::BatchPause(ids) => Box::pin(async move {
                for id in ids {
                    self.pause_job(id).await?;
                }
                Ok(())
            }),
            shell::popups::ProgressPopupAction::BatchResume(ids) => Box::pin(async move {
                for id in ids {
                    self.resume_job(id).await?;
                }
                Ok(())
            }),
            shell::popups::ProgressPopupAction::BatchCancel(ids) => Box::pin(async move {
                for id in ids {
                    self.cancel_job(id).await?;
                }
                Ok(())
            }),
            shell::popups::ProgressPopupAction::BatchRevealCompleted(id) => Box::pin(async move {
                self.reveal_job_in_folder(id).await?;
                Ok(())
            }),
        }
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
    let settings_state = SettingsRuntimeState::default();
    let toast_state = ToastRuntimeState::default();
    let dispatcher = MainWindowDispatcher::new(
        ui,
        state.clone(),
        queue_view.clone(),
        settings_state.clone(),
        toast_state.clone(),
    );
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
    settings_state.apply_snapshot_to_main_window(ui, &snapshot);
    let add_download_state = AddDownloadRuntimeState::default();
    let diagnostics_state = DiagnosticsRuntimeState::default();
    wire_main_window_lifecycle_callbacks(
        ui,
        Arc::new(MainWindowLifecycleController::new(ui, state.clone())),
    );
    wire_queue_command_callbacks(
        ui,
        runtime.clone(),
        backend.clone(),
        queue_view.clone(),
        toast_state.clone(),
    );
    wire_toast_callbacks(ui, toast_state.clone());
    wire_add_download_callbacks(
        ui,
        runtime.clone(),
        backend.clone(),
        queue_view.clone(),
        add_download_state,
        toast_state.clone(),
    );
    wire_settings_callbacks(
        ui,
        runtime.clone(),
        backend.clone(),
        settings_state.clone(),
        queue_view,
        toast_state.clone(),
    );
    wire_diagnostics_callbacks(
        ui,
        runtime.clone(),
        backend.clone(),
        settings_state,
        diagnostics_state.clone(),
        toast_state.clone(),
    );
    wire_update_callbacks(
        ui,
        runtime.clone(),
        backend.clone(),
        update_state.clone(),
        toast_state,
    );
    wire_prompt_window_action_bridge(runtime.clone(), backend.clone());
    wire_progress_popup_action_bridge(runtime.clone(), backend.clone());
    start_diagnostics_refresh(
        runtime.clone(),
        backend.clone(),
        diagnostics_state,
        ui.as_weak(),
        true,
        String::new(),
        None,
    );
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
    SettingsRuntimeState::default().apply_snapshot_to_main_window(ui, snapshot);
}

pub fn wire_prompt_window_action_bridge<C>(runtime: Arc<Runtime>, command_sink: Arc<C>)
where
    C: PromptWindowCommandSink,
{
    shell::popups::install_prompt_action_dispatcher(Arc::new(move |action| {
        let runtime = runtime.clone();
        let command_sink = command_sink.clone();
        runtime.spawn(async move {
            match action {
                shell::popups::PromptWindowAction::BrowseDirectory => {
                    match command_sink.browse_prompt_directory().await {
                        Ok(directory) => {
                            dispatch_prompt_popup_update(move |registry| {
                                registry.apply_prompt_directory_override(directory);
                            });
                        }
                        Err(error) => {
                            dispatch_prompt_popup_update(move |registry| {
                                registry.set_prompt_error(error);
                            });
                        }
                    }
                }
                shell::popups::PromptWindowAction::Confirm(request) => {
                    match command_sink.confirm_prompt_from_slint(request).await {
                        Ok(()) => {
                            dispatch_prompt_popup_update(|registry| {
                                registry.set_prompt_busy(false)
                            });
                        }
                        Err(error) => {
                            dispatch_prompt_popup_update(move |registry| {
                                registry.set_prompt_error(error);
                            });
                        }
                    }
                }
                shell::popups::PromptWindowAction::Cancel(id) => {
                    match command_sink.cancel_prompt_from_slint(id).await {
                        Ok(()) => {
                            dispatch_prompt_popup_update(|registry| {
                                registry.set_prompt_busy(false)
                            });
                        }
                        Err(error) => {
                            dispatch_prompt_popup_update(move |registry| {
                                registry.set_prompt_error(error);
                            });
                        }
                    }
                }
                shell::popups::PromptWindowAction::Swap(id) => {
                    match command_sink.swap_prompt_from_slint(id).await {
                        Ok(()) => {
                            dispatch_prompt_popup_update(|registry| {
                                registry.set_prompt_busy(false)
                            });
                        }
                        Err(error) => {
                            dispatch_prompt_popup_update(move |registry| {
                                registry.set_prompt_error(error);
                            });
                        }
                    }
                }
            }
        });
    }));
}

pub fn wire_progress_popup_action_bridge<C>(runtime: Arc<Runtime>, command_sink: Arc<C>)
where
    C: ProgressPopupCommandSink,
{
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    runtime.spawn(async move {
        while let Some(action) = receiver.recv().await {
            let target = progress_popup_action_job_target(&action);
            let result = command_sink.run_progress_popup_action(action).await;
            match result {
                Ok(()) => {
                    if let Some(id) = target {
                        dispatch_progress_popup_update(move |registry| {
                            registry.set_progress_action_busy(&id, false);
                        });
                    }
                }
                Err(error) => {
                    if let Some(id) = target {
                        dispatch_progress_popup_update(move |registry| {
                            registry.set_progress_action_error(&id, error);
                        });
                    }
                }
            }
        }
    });

    shell::popups::install_progress_popup_action_dispatcher(Arc::new(move |action| {
        let _ = sender.send(action);
    }));
}

fn progress_popup_action_job_target(action: &shell::popups::ProgressPopupAction) -> Option<String> {
    match action {
        shell::popups::ProgressPopupAction::Pause(id)
        | shell::popups::ProgressPopupAction::Resume(id)
        | shell::popups::ProgressPopupAction::Retry(id)
        | shell::popups::ProgressPopupAction::Cancel(id)
        | shell::popups::ProgressPopupAction::OpenFile(id)
        | shell::popups::ProgressPopupAction::RevealInFolder(id)
        | shell::popups::ProgressPopupAction::SwapFailedToBrowser(id)
        | shell::popups::ProgressPopupAction::BatchRevealCompleted(id) => Some(id.clone()),
        shell::popups::ProgressPopupAction::BatchPause(_)
        | shell::popups::ProgressPopupAction::BatchResume(_)
        | shell::popups::ProgressPopupAction::BatchCancel(_) => None,
    }
}

fn dispatch_prompt_popup_update(
    update: impl FnOnce(&mut shell::popups::PopupRegistry) + Send + 'static,
) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        shell::popups::with_popup_registry(update);
    }) {
        eprintln!("failed to update prompt popup state: {error}");
    }
}

fn dispatch_progress_popup_update(
    update: impl FnOnce(&mut shell::popups::PopupRegistry) + Send + 'static,
) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        shell::popups::with_popup_registry(update);
    }) {
        eprintln!("failed to update progress popup state: {error}");
    }
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

pub fn apply_settings_view_model_to_main_window(
    ui: &MainWindow,
    model: crate::controller::SettingsViewModel,
) {
    let sections = model
        .sections
        .into_iter()
        .map(slint_settings_nav_item_from_item)
        .collect::<Vec<_>>();
    let excluded_hosts = model
        .extension_excluded_hosts
        .into_iter()
        .map(slint::SharedString::from)
        .collect::<Vec<_>>();

    ui.set_settings_sections(ModelRc::from(Rc::new(VecModel::from(sections))));
    ui.set_settings_view_visible(model.visible);
    ui.set_settings_active_section(model.active_section_id.into());
    ui.set_settings_dirty(model.dirty);
    ui.set_settings_saving(model.saving);
    ui.set_settings_cache_clearing(model.cache_clearing);
    ui.set_settings_error_text(model.error_text.into());
    ui.set_settings_unsaved_prompt_visible(model.unsaved_prompt_visible);
    ui.set_settings_download_directory(model.download_directory.into());
    ui.set_settings_max_concurrent_downloads(model.max_concurrent_downloads.into());
    ui.set_settings_auto_retry_attempts(model.auto_retry_attempts.into());
    ui.set_settings_speed_limit_kib_per_second(model.speed_limit_kib_per_second.into());
    ui.set_settings_download_performance_mode(model.download_performance_mode_id.into());
    ui.set_settings_notifications_enabled(model.notifications_enabled);
    ui.set_settings_show_details_on_click(model.show_details_on_click);
    ui.set_settings_queue_row_size(model.queue_row_size_id.into());
    ui.set_settings_start_on_startup(model.start_on_startup);
    ui.set_settings_startup_launch_mode(model.startup_launch_mode_id.into());
    ui.set_settings_theme(model.theme_id.into());
    ui.set_settings_accent_color(model.accent_color.into());
    ui.set_settings_torrent_enabled(model.torrent_enabled);
    ui.set_settings_torrent_download_directory(model.torrent_download_directory.into());
    ui.set_settings_torrent_seed_mode(model.torrent_seed_mode_id.into());
    ui.set_settings_torrent_seed_ratio_limit(model.torrent_seed_ratio_limit.into());
    ui.set_settings_torrent_seed_time_limit_minutes(model.torrent_seed_time_limit_minutes.into());
    ui.set_settings_torrent_upload_limit_kib_per_second(
        model.torrent_upload_limit_kib_per_second.into(),
    );
    ui.set_settings_torrent_port_forwarding_enabled(model.torrent_port_forwarding_enabled);
    ui.set_settings_torrent_port_forwarding_port(model.torrent_port_forwarding_port.into());
    ui.set_settings_torrent_peer_watchdog_mode(model.torrent_peer_watchdog_mode_id.into());
    ui.set_settings_extension_enabled(model.extension_enabled);
    ui.set_settings_extension_handoff_mode(model.extension_handoff_mode_id.into());
    ui.set_settings_extension_listen_port(model.extension_listen_port.into());
    ui.set_settings_extension_context_menu_enabled(model.extension_context_menu_enabled);
    ui.set_settings_extension_show_progress_after_handoff(
        model.extension_show_progress_after_handoff,
    );
    ui.set_settings_extension_show_badge_status(model.extension_show_badge_status);
    ui.set_settings_extension_authenticated_handoff_enabled(
        model.extension_authenticated_handoff_enabled,
    );
    ui.set_settings_extension_excluded_host_input(model.extension_excluded_host_input.into());
    ui.set_settings_extension_excluded_hosts_summary(model.extension_excluded_hosts_summary.into());
    ui.set_settings_extension_excluded_hosts(ModelRc::from(Rc::new(VecModel::from(
        excluded_hosts,
    ))));
}

pub fn apply_diagnostics_view_model_to_main_window(
    ui: &MainWindow,
    model: crate::controller::DiagnosticsViewModel,
) {
    let host_entries = model
        .host_entries
        .into_iter()
        .map(slint_host_registration_entry_from_row)
        .collect::<Vec<_>>();
    let events = model
        .recent_events
        .into_iter()
        .map(slint_diagnostic_event_from_row)
        .collect::<Vec<_>>();
    let torrents = model
        .torrent_diagnostics
        .into_iter()
        .map(slint_torrent_diagnostic_from_row)
        .collect::<Vec<_>>();

    ui.set_diagnostics_loading(model.loading);
    ui.set_diagnostics_has_snapshot(model.has_snapshot);
    ui.set_diagnostics_status_label(model.status_label.into());
    ui.set_diagnostics_status_message(model.status_message.into());
    ui.set_diagnostics_status_tone(model.status_tone.into());
    ui.set_diagnostics_last_host_contact(model.last_host_contact_text.into());
    ui.set_diagnostics_queue_summary(model.queue_summary_text.into());
    ui.set_diagnostics_action_status_text(model.action_status_text.into());
    ui.set_diagnostics_error_text(model.error_text.into());
    ui.set_diagnostics_host_entries(ModelRc::from(Rc::new(VecModel::from(host_entries))));
    ui.set_diagnostics_recent_events(ModelRc::from(Rc::new(VecModel::from(events))));
    ui.set_diagnostics_torrent_diagnostics(ModelRc::from(Rc::new(VecModel::from(torrents))));
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

pub fn wire_toast_callbacks(ui: &MainWindow, toast_state: ToastRuntimeState) {
    toast_state.render(ui);
    let window = ui.as_weak();
    ui.on_toast_dismiss_requested(move |id| {
        if let Some(ui) = window.upgrade() {
            toast_state.dismiss_toast(&ui, id.as_str());
        }
    });
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
    toast_state: ToastRuntimeState,
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
    let delete_confirm_toasts = toast_state.clone();
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
        let window = delete_confirm_window.clone();
        let toast_state = delete_confirm_toasts.clone();
        delete_confirm_runtime.spawn(async move {
            let command = QueueCommand::DeleteMany {
                ids,
                delete_from_disk,
            };
            let result = command_sink.run_queue_command(command.clone()).await;
            dispatch_queue_command_result_to_weak(window, toast_state, command, result);
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
    let rename_confirm_toasts = toast_state.clone();
    ui.on_rename_confirmed(move || {
        let Some(ui) = rename_confirm_window.upgrade() else {
            return;
        };
        let Some((id, filename)) = rename_confirm_state.confirm_rename_prompt(&ui) else {
            return;
        };
        let command_sink = rename_confirm_sink.clone();
        let window = rename_confirm_window.clone();
        let toast_state = rename_confirm_toasts.clone();
        rename_confirm_runtime.spawn(async move {
            let command = QueueCommand::Rename { id, filename };
            let result = command_sink.run_queue_command(command.clone()).await;
            dispatch_queue_command_result_to_weak(window, toast_state, command, result);
        });
    });

    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_pause_job_requested,
        QueueCommand::Pause,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_resume_job_requested,
        QueueCommand::Resume,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_cancel_job_requested,
        QueueCommand::Cancel,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_retry_job_requested,
        QueueCommand::Retry,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_restart_job_requested,
        QueueCommand::Restart,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_open_progress_requested,
        QueueCommand::OpenProgress,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_open_job_file_requested,
        QueueCommand::OpenFile,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_reveal_job_requested,
        QueueCommand::RevealInFolder,
    );
    wire_string_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_swap_failed_to_browser_requested,
        QueueCommand::SwapFailedToBrowser,
    );
    wire_void_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_pause_all_requested,
        QueueCommand::PauseAll,
    );
    wire_void_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_resume_all_requested,
        QueueCommand::ResumeAll,
    );
    wire_void_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state.clone(),
        MainWindow::on_retry_failed_requested,
        QueueCommand::RetryFailed,
    );
    wire_void_command(
        ui,
        runtime.clone(),
        command_sink.clone(),
        toast_state,
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
    toast_state: ToastRuntimeState,
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
    let import_toast_state = toast_state.clone();
    ui.on_add_download_import_torrent_requested(move || {
        let Some(ui) = import_window.upgrade() else {
            return;
        };
        import_state.set_importing(&ui, true);
        let window = import_window.clone();
        let add_state = import_state.clone();
        let command_sink = import_sink.clone();
        let toast_state = import_toast_state.clone();
        import_runtime.spawn(async move {
            match command_sink.browse_torrent_file_for_add_download().await {
                Ok(path) => dispatch_add_download_import_to_weak(window, add_state, path),
                Err(error) => dispatch_add_download_error_to_weak(
                    window,
                    add_state,
                    Some(toast_state),
                    error,
                    false,
                    true,
                ),
            }
        });
    });

    let submit_state = add_state.clone();
    let submit_window = ui.as_weak();
    let submit_runtime = runtime;
    let submit_toast_state = toast_state;
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
        let toast_state = submit_toast_state.clone();
        submit_runtime.spawn(async move {
            match run_add_download_submission(command_sink, submission).await {
                Ok(outcome) => dispatch_add_download_success_to_weak(
                    window,
                    add_state,
                    queue_view,
                    toast_state,
                    outcome,
                ),
                Err(error) => dispatch_add_download_error_to_weak(
                    window,
                    add_state,
                    Some(toast_state),
                    error,
                    true,
                    false,
                ),
            }
        });
    });
}

pub fn wire_settings_callbacks<C>(
    ui: &MainWindow,
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    settings_state: SettingsRuntimeState,
    _queue_view: QueueViewRuntimeState,
    toast_state: ToastRuntimeState,
) where
    C: SettingsCommandSink,
{
    settings_state.render(ui);

    let open_state = settings_state.clone();
    let open_window = ui.as_weak();
    ui.on_settings_requested(move || {
        if let Some(ui) = open_window.upgrade() {
            open_state.open(&ui);
        }
    });

    let section_state = settings_state.clone();
    let section_window = ui.as_weak();
    ui.on_settings_section_requested(move |section| {
        if let Some(ui) = section_window.upgrade() {
            section_state.change_section(&ui, section.as_str());
        }
    });

    let cancel_state = settings_state.clone();
    let cancel_window = ui.as_weak();
    ui.on_settings_cancel_requested(move || {
        if let Some(ui) = cancel_window.upgrade() {
            cancel_state.request_close(&ui);
        }
    });

    let discard_state = settings_state.clone();
    let discard_window = ui.as_weak();
    ui.on_settings_discard_confirmed(move || {
        if let Some(ui) = discard_window.upgrade() {
            discard_state.discard_and_close(&ui);
        }
    });

    let unsaved_cancel_state = settings_state.clone();
    let unsaved_cancel_window = ui.as_weak();
    ui.on_settings_unsaved_cancelled(move || {
        if let Some(ui) = unsaved_cancel_window.upgrade() {
            unsaved_cancel_state.cancel_unsaved_prompt(&ui);
        }
    });

    let save_state = settings_state.clone();
    let save_window = ui.as_weak();
    let save_runtime = runtime.clone();
    let save_sink = command_sink.clone();
    let save_toast_state = toast_state.clone();
    ui.on_settings_save_requested(move || {
        let Some(ui) = save_window.upgrade() else {
            return;
        };
        let draft = save_state.draft_for_save();
        save_state.set_saving(&ui, true);
        let window = save_window.clone();
        let state = save_state.clone();
        let sink = save_sink.clone();
        let toast_state = save_toast_state.clone();
        save_runtime.spawn(async move {
            match sink.save_settings_from_slint(draft).await {
                Ok(settings) => {
                    dispatch_settings_saved_to_weak(window, state, Some(toast_state), settings)
                }
                Err(error) => dispatch_settings_error_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    "Save Failed",
                    error,
                ),
            }
        });
    });

    let browse_download_state = settings_state.clone();
    let browse_download_window = ui.as_weak();
    let browse_download_runtime = runtime.clone();
    let browse_download_sink = command_sink.clone();
    let browse_download_toast_state = toast_state.clone();
    ui.on_settings_browse_download_directory_requested(move || {
        let window = browse_download_window.clone();
        let state = browse_download_state.clone();
        let sink = browse_download_sink.clone();
        let toast_state = browse_download_toast_state.clone();
        browse_download_runtime.spawn(async move {
            match sink.browse_settings_directory().await {
                Ok(Some(directory)) => dispatch_settings_directory_to_weak(
                    window,
                    state,
                    directory,
                    SettingsDirectoryTarget::Download,
                ),
                Ok(None) => {}
                Err(error) => dispatch_settings_error_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    "Browse Failed",
                    error,
                ),
            }
        });
    });

    let browse_torrent_state = settings_state.clone();
    let browse_torrent_window = ui.as_weak();
    let browse_torrent_runtime = runtime.clone();
    let browse_torrent_sink = command_sink.clone();
    let browse_torrent_toast_state = toast_state.clone();
    ui.on_settings_browse_torrent_directory_requested(move || {
        let window = browse_torrent_window.clone();
        let state = browse_torrent_state.clone();
        let sink = browse_torrent_sink.clone();
        let toast_state = browse_torrent_toast_state.clone();
        browse_torrent_runtime.spawn(async move {
            match sink.browse_settings_directory().await {
                Ok(Some(directory)) => dispatch_settings_directory_to_weak(
                    window,
                    state,
                    directory,
                    SettingsDirectoryTarget::Torrent,
                ),
                Ok(None) => {}
                Err(error) => dispatch_settings_error_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    "Browse Failed",
                    error,
                ),
            }
        });
    });

    let cache_state = settings_state.clone();
    let cache_window = ui.as_weak();
    let cache_runtime = runtime.clone();
    let cache_sink = command_sink;
    let cache_toast_state = toast_state;
    ui.on_settings_clear_torrent_cache_requested(move || {
        let Some(ui) = cache_window.upgrade() else {
            return;
        };
        cache_state.set_cache_clearing(&ui, true);
        let window = cache_window.clone();
        let state = cache_state.clone();
        let sink = cache_sink.clone();
        let toast_state = cache_toast_state.clone();
        cache_runtime.spawn(async move {
            match sink.clear_settings_torrent_session_cache().await {
                Ok(result) => {
                    dispatch_settings_cache_cleared_to_weak(window, state, toast_state, result)
                }
                Err(error) => dispatch_settings_error_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    "Cache Clear Failed",
                    error,
                ),
            }
        });
    });

    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_download_directory_changed,
        SettingsRuntimeState::set_download_directory,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_max_concurrent_downloads_changed,
        SettingsRuntimeState::set_max_concurrent_downloads,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_auto_retry_attempts_changed,
        SettingsRuntimeState::set_auto_retry_attempts,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_speed_limit_kib_per_second_changed,
        SettingsRuntimeState::set_speed_limit_kib_per_second,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_download_performance_mode_changed,
        |state, ui, value| state.set_download_performance_mode(ui, &value),
    );
    wire_settings_bool_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_notifications_enabled_changed,
        SettingsRuntimeState::set_notifications_enabled,
    );
    wire_settings_bool_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_show_details_on_click_changed,
        SettingsRuntimeState::set_show_details_on_click,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_queue_row_size_changed,
        |state, ui, value| state.set_queue_row_size(ui, &value),
    );
    wire_settings_bool_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_start_on_startup_changed,
        SettingsRuntimeState::set_start_on_startup,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_startup_launch_mode_changed,
        |state, ui, value| state.set_startup_launch_mode(ui, &value),
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_theme_changed,
        |state, ui, value| state.set_theme(ui, &value),
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_accent_color_changed,
        SettingsRuntimeState::set_accent_color,
    );
    wire_settings_bool_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_torrent_enabled_changed,
        SettingsRuntimeState::set_torrent_enabled,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_torrent_download_directory_changed,
        SettingsRuntimeState::set_torrent_download_directory,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_torrent_seed_mode_changed,
        |state, ui, value| state.set_torrent_seed_mode(ui, &value),
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_torrent_seed_ratio_limit_changed,
        SettingsRuntimeState::set_torrent_seed_ratio_limit,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_torrent_seed_time_limit_minutes_changed,
        SettingsRuntimeState::set_torrent_seed_time_limit_minutes,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_torrent_upload_limit_kib_per_second_changed,
        SettingsRuntimeState::set_torrent_upload_limit_kib_per_second,
    );
    wire_settings_bool_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_torrent_port_forwarding_enabled_changed,
        SettingsRuntimeState::set_torrent_port_forwarding_enabled,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_torrent_port_forwarding_port_changed,
        SettingsRuntimeState::set_torrent_port_forwarding_port,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_torrent_peer_watchdog_mode_changed,
        |state, ui, value| state.set_torrent_peer_watchdog_mode(ui, &value),
    );
    wire_settings_bool_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_extension_enabled_changed,
        SettingsRuntimeState::set_extension_enabled,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_extension_handoff_mode_changed,
        |state, ui, value| state.set_extension_handoff_mode(ui, &value),
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_extension_listen_port_changed,
        SettingsRuntimeState::set_extension_listen_port,
    );
    wire_settings_bool_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_extension_context_menu_enabled_changed,
        SettingsRuntimeState::set_extension_context_menu_enabled,
    );
    wire_settings_bool_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_extension_show_progress_after_handoff_changed,
        SettingsRuntimeState::set_extension_show_progress_after_handoff,
    );
    wire_settings_bool_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_extension_show_badge_status_changed,
        SettingsRuntimeState::set_extension_show_badge_status,
    );
    wire_settings_bool_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_extension_authenticated_handoff_enabled_changed,
        SettingsRuntimeState::set_extension_authenticated_handoff_enabled,
    );
    wire_settings_string_callback(
        ui,
        settings_state.clone(),
        MainWindow::on_settings_extension_excluded_host_input_changed,
        SettingsRuntimeState::set_excluded_host_input,
    );

    let add_host_state = settings_state.clone();
    let add_host_window = ui.as_weak();
    ui.on_settings_extension_excluded_host_add_requested(move || {
        if let Some(ui) = add_host_window.upgrade() {
            add_host_state.add_excluded_hosts_from_input(&ui);
        }
    });

    let remove_host_state = settings_state;
    let remove_host_window = ui.as_weak();
    ui.on_settings_extension_excluded_host_remove_requested(move |host| {
        if let Some(ui) = remove_host_window.upgrade() {
            remove_host_state.remove_excluded_host(&ui, host.to_string());
        }
    });
}

pub fn wire_diagnostics_callbacks<C>(
    ui: &MainWindow,
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    settings_state: SettingsRuntimeState,
    diagnostics_state: DiagnosticsRuntimeState,
    toast_state: ToastRuntimeState,
) where
    C: DiagnosticsCommandSink,
{
    diagnostics_state.render(ui);

    let section_window = ui.as_weak();
    let section_runtime = runtime.clone();
    let section_sink = command_sink.clone();
    let section_diagnostics = diagnostics_state.clone();
    settings_state.set_section_observer(move |section| {
        if section != SettingsSection::NativeHost || !section_diagnostics.needs_refresh() {
            return;
        }
        start_diagnostics_refresh(
            section_runtime.clone(),
            section_sink.clone(),
            section_diagnostics.clone(),
            section_window.clone(),
            false,
            String::new(),
            None,
        );
    });

    let refresh_window = ui.as_weak();
    let refresh_runtime = runtime.clone();
    let refresh_sink = command_sink.clone();
    let refresh_state = diagnostics_state.clone();
    let refresh_toast_state = toast_state.clone();
    ui.on_diagnostics_refresh_requested(move || {
        start_diagnostics_refresh(
            refresh_runtime.clone(),
            refresh_sink.clone(),
            refresh_state.clone(),
            refresh_window.clone(),
            false,
            "Diagnostics refreshed.".into(),
            Some(refresh_toast_state.clone()),
        );
    });

    let copy_window = ui.as_weak();
    let copy_runtime = runtime.clone();
    let copy_sink = command_sink.clone();
    let copy_state = diagnostics_state.clone();
    let copy_toast_state = toast_state.clone();
    ui.on_diagnostics_copy_requested(move || {
        let Some(ui) = copy_window.upgrade() else {
            return;
        };
        let Some(report) = copy_state.current_report() else {
            copy_state.set_error(&ui, "Refresh diagnostics before copying the report.".into());
            return;
        };
        let window = copy_window.clone();
        let state = copy_state.clone();
        let sink = copy_sink.clone();
        let toast_state = copy_toast_state.clone();
        copy_runtime.spawn(async move {
            match sink.copy_diagnostics_report_from_slint(report).await {
                Ok(()) => dispatch_diagnostics_status_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    Some(toast_message(
                        ToastType::Success,
                        "Diagnostics Copied",
                        "The diagnostics report was copied to the clipboard.",
                    )),
                    "The diagnostics report was copied to the clipboard.".into(),
                ),
                Err(error) => dispatch_diagnostics_error_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    "Copy Failed",
                    error,
                ),
            }
        });
    });

    let export_window = ui.as_weak();
    let export_runtime = runtime.clone();
    let export_sink = command_sink.clone();
    let export_state = diagnostics_state.clone();
    let export_toast_state = toast_state.clone();
    ui.on_diagnostics_export_requested(move || {
        let window = export_window.clone();
        let state = export_state.clone();
        let sink = export_sink.clone();
        let toast_state = export_toast_state.clone();
        export_runtime.spawn(async move {
            match sink.export_diagnostics_report_from_slint().await {
                Ok(Some(path)) => dispatch_diagnostics_status_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    Some(toast_message(
                        ToastType::Success,
                        "Diagnostics Exported",
                        format!("Saved diagnostics to {path}."),
                    )),
                    format!("Saved diagnostics to {path}."),
                ),
                Ok(None) => dispatch_diagnostics_status_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    Some(toast_message(
                        ToastType::Info,
                        "Export Cancelled",
                        "No diagnostics report was saved.",
                    )),
                    "No diagnostics report was saved.".into(),
                ),
                Err(error) => dispatch_diagnostics_error_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    "Export Failed",
                    error,
                ),
            }
        });
    });

    let docs_window = ui.as_weak();
    let docs_runtime = runtime.clone();
    let docs_sink = command_sink.clone();
    let docs_state = diagnostics_state.clone();
    let docs_toast_state = toast_state.clone();
    ui.on_diagnostics_open_install_docs_requested(move || {
        let window = docs_window.clone();
        let state = docs_state.clone();
        let sink = docs_sink.clone();
        let toast_state = docs_toast_state.clone();
        docs_runtime.spawn(async move {
            match sink.open_install_docs_from_slint().await {
                Ok(()) => dispatch_diagnostics_status_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    None,
                    "Opened native host installation docs.".into(),
                ),
                Err(error) => dispatch_diagnostics_error_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    "Open Docs Failed",
                    error,
                ),
            }
        });
    });

    let repair_window = ui.as_weak();
    let repair_runtime = runtime.clone();
    let repair_sink = command_sink.clone();
    let repair_state = diagnostics_state.clone();
    let repair_toast_state = toast_state.clone();
    ui.on_diagnostics_repair_host_requested(move || {
        let Some(ui) = repair_window.upgrade() else {
            return;
        };
        repair_state.set_loading(&ui, true);
        let window = repair_window.clone();
        let state = repair_state.clone();
        let sink = repair_sink.clone();
        let toast_state = repair_toast_state.clone();
        repair_runtime.spawn(async move {
            match sink.repair_host_registration_from_slint().await {
                Ok(()) => match sink.get_diagnostics_for_slint().await {
                    Ok(diagnostics) => dispatch_diagnostics_snapshot_to_weak(
                        window,
                        state,
                        diagnostics,
                        "Native host registration was refreshed.".into(),
                        Some(toast_state),
                        Some(toast_message(
                            ToastType::Success,
                            "Native Host Repaired",
                            "Native host registration was refreshed.",
                        )),
                    ),
                    Err(error) => dispatch_diagnostics_error_to_weak(
                        window,
                        state,
                        Some(toast_state),
                        "Diagnostics Failed",
                        error,
                    ),
                },
                Err(error) => dispatch_diagnostics_error_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    "Repair Failed",
                    error,
                ),
            }
        });
    });

    let handoff_window = ui.as_weak();
    let handoff_runtime = runtime;
    let handoff_state = diagnostics_state;
    let handoff_toast_state = toast_state;
    ui.on_diagnostics_test_handoff_requested(move || {
        let window = handoff_window.clone();
        let state = handoff_state.clone();
        let sink = command_sink.clone();
        let toast_state = handoff_toast_state.clone();
        handoff_runtime.spawn(async move {
            match sink.test_extension_handoff_from_slint().await {
                Ok(()) => dispatch_diagnostics_status_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    Some(toast_message(
                        ToastType::Success,
                        "Test Prompt Opened",
                        "A browser-style download prompt was opened.",
                    )),
                    "A browser-style download prompt was opened.".into(),
                ),
                Err(error) => dispatch_diagnostics_error_to_weak(
                    window,
                    state,
                    Some(toast_state),
                    "Test Handoff Failed",
                    error,
                ),
            }
        });
    });
}

pub fn wire_update_callbacks<C>(
    ui: &MainWindow,
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    update_state: UpdateStateStore,
    toast_state: ToastRuntimeState,
) where
    C: UpdateCommandSink,
{
    apply_update_state_to_main_window(ui, &update_state.snapshot());

    let check_window = ui.as_weak();
    let check_state = update_state.clone();
    let check_runtime = runtime.clone();
    let check_sink = command_sink.clone();
    let check_toast_state = toast_state.clone();
    ui.on_check_update_requested(move || {
        let next =
            check_state.update(|state| update::start_update_check(state, UpdateCheckMode::Manual));
        apply_update_state_to_weak(&check_window, &next);

        let window = check_window.clone();
        let update_state = check_state.clone();
        let command_sink = check_sink.clone();
        let toast_state = check_toast_state.clone();
        check_runtime.spawn(async move {
            match command_sink
                .run_update_command(UpdateCommand::Check(UpdateCheckMode::Manual))
                .await
            {
                Ok(update) => {
                    let next =
                        update_state.update(|state| update::finish_update_check(state, update));
                    let toast = update_check_toast(&next);
                    dispatch_update_state_with_toast_to_weak(
                        window,
                        next,
                        Some(toast_state),
                        toast,
                    );
                }
                Err(error) => {
                    let next = update_state.update(|state| update::fail_update_check(state, error));
                    dispatch_update_state_with_toast_to_weak(
                        window,
                        next.clone(),
                        Some(toast_state),
                        Some(toast_message(
                            ToastType::Error,
                            "Update Check Failed",
                            next.error_message
                                .unwrap_or_else(|| "Update check failed.".into()),
                        )),
                    );
                }
            }
        });
    });

    let install_window = ui.as_weak();
    let install_state = update_state;
    let install_runtime = runtime;
    let install_toast_state = toast_state;
    ui.on_install_update_requested(move || {
        let next = install_state.update(update::begin_update_install);
        apply_update_state_to_weak(&install_window, &next);

        let window = install_window.clone();
        let update_state = install_state.clone();
        let command_sink = command_sink.clone();
        let toast_state = install_toast_state.clone();
        install_runtime.spawn(async move {
            if let Err(error) = command_sink
                .run_update_command(UpdateCommand::Install)
                .await
            {
                let next = update_state.update(|state| update::fail_update_install(state, error));
                dispatch_update_state_with_toast_to_weak(
                    window,
                    next.clone(),
                    Some(toast_state),
                    Some(ToastMessage::persistent(
                        ToastType::Error,
                        "Update Failed",
                        next.error_message
                            .unwrap_or_else(|| "Update installation failed.".into()),
                    )),
                );
            }
        });
    });
}

async fn run_add_download_submission<C>(
    command_sink: Arc<C>,
    submission: AddDownloadSubmission,
) -> Result<AddDownloadSubmissionOutcome, String>
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
            let progress_warning = if let Some(AddDownloadProgressIntent::Single { job_id }) =
                &outcome.progress_intent
            {
                command_sink
                    .open_add_download_progress_window(job_id.clone())
                    .await
                    .err()
            } else {
                None
            };
            Ok(AddDownloadSubmissionOutcome {
                outcome,
                progress_warning,
            })
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
            let progress_warning = if let Some(AddDownloadProgressIntent::Batch { context }) =
                &outcome.progress_intent
            {
                command_sink
                    .open_add_download_batch_progress_window(context.clone())
                    .await
                    .err()
            } else {
                None
            };
            Ok(AddDownloadSubmissionOutcome {
                outcome,
                progress_warning,
            })
        }
    }
}

struct AddDownloadSubmissionOutcome {
    outcome: crate::controller::AddDownloadOutcome,
    progress_warning: Option<String>,
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
    toast_state: ToastRuntimeState,
    submission_outcome: AddDownloadSubmissionOutcome,
) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            let outcome = submission_outcome.outcome;
            queue_view.change_view(&ui, outcome.view_id);
            if let Some(id) = outcome.primary_job_id.as_deref() {
                queue_view.select_job_in_main_window(&ui, id);
            }
            toast_state.add_toast(&ui, add_download_success_toast(&outcome));
            if let Some(error) = submission_outcome.progress_warning {
                toast_state.add_toast(
                    &ui,
                    toast_message(ToastType::Warning, "Progress Popup Failed", error),
                );
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
    toast_state: Option<ToastRuntimeState>,
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
            add_state.set_error(&ui, error.clone());
            if let Some(toast_state) = toast_state {
                toast_state.add_toast(
                    &ui,
                    toast_message(ToastType::Error, "Add Download Failed", error),
                );
            }
        }
    }) {
        eprintln!("failed to update Slint add-download error state: {dispatch_error}");
    }
}

fn add_download_success_toast(outcome: &crate::controller::AddDownloadOutcome) -> ToastMessage {
    if outcome.queued_count == 0 && outcome.duplicate_count > 0 {
        return toast_message(
            ToastType::Info,
            "Already in Queue",
            if outcome.total_count == 1 {
                match outcome.primary_filename.as_deref() {
                    Some(filename) if !filename.is_empty() => {
                        format!("{filename} is already in the download list.")
                    }
                    _ => "That download is already in the download list.".into(),
                }
            } else {
                "All submitted downloads are already in the list.".into()
            },
        );
    }

    match outcome.mode {
        DownloadMode::Torrent => toast_message(
            ToastType::Success,
            "Torrent Added",
            match outcome.primary_filename.as_deref() {
                Some(filename) if !filename.is_empty() => {
                    format!("{filename} was added to the queue.")
                }
                _ => "The torrent was added to the queue.".into(),
            },
        ),
        DownloadMode::Single => toast_message(
            ToastType::Success,
            "Download Added",
            match outcome.primary_filename.as_deref() {
                Some(filename) if !filename.is_empty() => {
                    format!("{filename} was added to the queue.")
                }
                _ => "The download was added to the queue.".into(),
            },
        ),
        DownloadMode::Bulk => toast_message(
            ToastType::Success,
            "Bulk Download Added",
            plural_downloads_added(outcome.queued_count),
        ),
        DownloadMode::Multi => toast_message(
            ToastType::Success,
            "Downloads Added",
            plural_downloads_added(outcome.queued_count),
        ),
    }
}

fn plural_downloads_added(count: usize) -> String {
    if count == 1 {
        "1 download was added to the queue.".into()
    } else {
        format!("{count} downloads were added to the queue.")
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

fn dispatch_update_state_with_toast_to_weak(
    window: slint::Weak<MainWindow>,
    state: AppUpdateState,
    toast_state: Option<ToastRuntimeState>,
    toast: Option<ToastMessage>,
) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        apply_update_state_to_weak(&window, &state);
        if let Some(ui) = window.upgrade() {
            if let (Some(toast_state), Some(toast)) = (toast_state, toast) {
                toast_state.add_toast(&ui, toast);
            }
        }
    }) {
        eprintln!("failed to update Slint updater panel toast: {error}");
    }
}

fn update_check_toast(state: &AppUpdateState) -> Option<ToastMessage> {
    match state.status.as_str() {
        "available" => state.available_update.as_ref().map(|metadata| {
            ToastMessage::persistent(
                ToastType::Info,
                "Update Available",
                format!(
                    "Simple Download Manager {} is ready to install.",
                    metadata.version
                ),
            )
        }),
        "not_available" => Some(toast_message(
            ToastType::Info,
            "No Update Available",
            "You are running the latest alpha build.",
        )),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsDirectoryTarget {
    Download,
    Torrent,
}

fn dispatch_settings_saved_to_weak(
    window: slint::Weak<MainWindow>,
    settings_state: SettingsRuntimeState,
    toast_state: Option<ToastRuntimeState>,
    settings: Settings,
) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            settings_state.apply_saved_settings(&ui, settings);
            if let Some(toast_state) = toast_state {
                toast_state.add_toast(
                    &ui,
                    toast_message(
                        ToastType::Success,
                        "Settings Saved",
                        "Preferences updated successfully.",
                    ),
                );
            }
        }
    }) {
        eprintln!("failed to update Slint settings save state: {error}");
    }
}

fn dispatch_settings_directory_to_weak(
    window: slint::Weak<MainWindow>,
    settings_state: SettingsRuntimeState,
    directory: String,
    target: SettingsDirectoryTarget,
) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            match target {
                SettingsDirectoryTarget::Download => {
                    settings_state.set_download_directory(&ui, directory);
                }
                SettingsDirectoryTarget::Torrent => {
                    settings_state.set_torrent_download_directory(&ui, directory);
                }
            }
        }
    }) {
        eprintln!("failed to update Slint settings directory state: {error}");
    }
}

fn dispatch_settings_cache_cleared_to_weak(
    window: slint::Weak<MainWindow>,
    settings_state: SettingsRuntimeState,
    toast_state: ToastRuntimeState,
    result: TorrentSessionCacheClearResult,
) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            settings_state.set_cache_clearing(&ui, false);
            let (title, message) = if result.pending_restart {
                (
                    "Cache Clear Scheduled",
                    format!(
                        "Torrent session cache is locked and will be cleared on next startup: {}",
                        result.session_path
                    ),
                )
            } else {
                (
                    "Torrent Session Cache Cleared",
                    format!("Torrent session cache cleared: {}", result.session_path),
                )
            };
            toast_state.add_toast(&ui, toast_message(ToastType::Success, title, message));
        }
    }) {
        eprintln!("failed to update Slint torrent cache clear state: {error}");
    }
}

fn dispatch_settings_error_to_weak(
    window: slint::Weak<MainWindow>,
    settings_state: SettingsRuntimeState,
    toast_state: Option<ToastRuntimeState>,
    title: &'static str,
    error: String,
) {
    if let Err(dispatch_error) = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            settings_state.set_error(&ui, error.clone());
            if let Some(toast_state) = toast_state {
                toast_state.add_toast(&ui, toast_message(ToastType::Error, title, error));
            }
        }
    }) {
        eprintln!("failed to update Slint settings error state: {dispatch_error}");
    }
}

fn start_diagnostics_refresh<C>(
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    diagnostics_state: DiagnosticsRuntimeState,
    window: slint::Weak<MainWindow>,
    silent_failure: bool,
    success_status: String,
    toast_state: Option<ToastRuntimeState>,
) where
    C: DiagnosticsCommandSink,
{
    if let Some(ui) = window.upgrade() {
        diagnostics_state.set_loading(&ui, true);
    }
    runtime.spawn(async move {
        match command_sink.get_diagnostics_for_slint().await {
            Ok(diagnostics) => {
                let toast = if success_status.is_empty() {
                    None
                } else {
                    Some(toast_message(
                        ToastType::Success,
                        "Diagnostics Refreshed",
                        success_status.clone(),
                    ))
                };
                dispatch_diagnostics_snapshot_to_weak(
                    window,
                    diagnostics_state,
                    diagnostics,
                    success_status,
                    toast_state,
                    toast,
                );
            }
            Err(error) if silent_failure => {
                dispatch_diagnostics_status_to_weak(
                    window,
                    diagnostics_state,
                    None,
                    None,
                    String::new(),
                );
                eprintln!("startup diagnostics refresh failed: {error}");
            }
            Err(error) => dispatch_diagnostics_error_to_weak(
                window,
                diagnostics_state,
                toast_state,
                "Diagnostics Failed",
                error,
            ),
        }
    });
}

fn dispatch_diagnostics_snapshot_to_weak(
    window: slint::Weak<MainWindow>,
    diagnostics_state: DiagnosticsRuntimeState,
    diagnostics: DiagnosticsSnapshot,
    status: String,
    toast_state: Option<ToastRuntimeState>,
    toast: Option<ToastMessage>,
) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            diagnostics_state.apply_snapshot(&ui, diagnostics, status);
            if let (Some(toast_state), Some(toast)) = (toast_state, toast) {
                toast_state.add_toast(&ui, toast);
            }
        }
    }) {
        eprintln!("failed to update Slint diagnostics snapshot: {error}");
    }
}

fn dispatch_diagnostics_status_to_weak(
    window: slint::Weak<MainWindow>,
    diagnostics_state: DiagnosticsRuntimeState,
    toast_state: Option<ToastRuntimeState>,
    toast: Option<ToastMessage>,
    status: String,
) {
    if let Err(error) = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            diagnostics_state.set_action_status(&ui, status);
            if let (Some(toast_state), Some(toast)) = (toast_state, toast) {
                toast_state.add_toast(&ui, toast);
            }
        }
    }) {
        eprintln!("failed to update Slint diagnostics status: {error}");
    }
}

fn dispatch_diagnostics_error_to_weak(
    window: slint::Weak<MainWindow>,
    diagnostics_state: DiagnosticsRuntimeState,
    toast_state: Option<ToastRuntimeState>,
    title: &'static str,
    error: String,
) {
    if let Err(dispatch_error) = slint::invoke_from_event_loop(move || {
        if let Some(ui) = window.upgrade() {
            diagnostics_state.set_error(&ui, error.clone());
            if let Some(toast_state) = toast_state {
                toast_state.add_toast(&ui, toast_message(ToastType::Error, title, error));
            }
        }
    }) {
        eprintln!("failed to update Slint diagnostics error: {dispatch_error}");
    }
}

fn update_download_directory_with_torrent_default(settings: &mut Settings, directory: String) {
    let previous_default = default_torrent_download_directory(&settings.download_directory);
    let should_update_torrent_directory = settings.torrent.download_directory.trim().is_empty()
        || settings.torrent.download_directory == previous_default;
    settings.download_directory = directory;
    if should_update_torrent_directory {
        settings.torrent.download_directory =
            default_torrent_download_directory(&settings.download_directory);
    }
}

fn wire_settings_string_callback(
    ui: &MainWindow,
    settings_state: SettingsRuntimeState,
    register: StringCommandRegistrar,
    update: impl Fn(&SettingsRuntimeState, &MainWindow, String) + 'static,
) {
    let window = ui.as_weak();
    register(
        ui,
        Box::new(move |value| {
            if let Some(ui) = window.upgrade() {
                update(&settings_state, &ui, value.to_string());
            }
        }),
    );
}

fn wire_settings_bool_callback(
    ui: &MainWindow,
    settings_state: SettingsRuntimeState,
    register: BoolSettingsRegistrar,
    update: impl Fn(&SettingsRuntimeState, &MainWindow, bool) + 'static,
) {
    let window = ui.as_weak();
    register(
        ui,
        Box::new(move |value| {
            if let Some(ui) = window.upgrade() {
                update(&settings_state, &ui, value);
            }
        }),
    );
}

fn dispatch_queue_command_result_to_weak(
    window: slint::Weak<MainWindow>,
    toast_state: ToastRuntimeState,
    command: QueueCommand,
    result: Result<QueueCommandOutput, String>,
) {
    if let Err(dispatch_error) = slint::invoke_from_event_loop(move || {
        let Some(ui) = window.upgrade() else {
            return;
        };
        match result {
            Ok(output) => {
                for toast in queue_command_success_toasts(&command, &output) {
                    toast_state.add_toast(&ui, toast);
                }
            }
            Err(error) => {
                toast_state.add_toast(
                    &ui,
                    toast_message(ToastType::Error, queue_command_error_title(&command), error),
                );
            }
        }
    }) {
        eprintln!("failed to dispatch Slint queue command toast: {dispatch_error}");
    }
}

fn queue_command_error_title(command: &QueueCommand) -> &'static str {
    match command {
        QueueCommand::Pause(_) => "Pause Failed",
        QueueCommand::Resume(_) => "Resume Failed",
        QueueCommand::Cancel(_) => "Cancel Failed",
        QueueCommand::Retry(_) => "Retry Failed",
        QueueCommand::Restart(_) => "Restart Failed",
        QueueCommand::Remove(_) | QueueCommand::DeleteMany { .. } => "Delete Failed",
        QueueCommand::Rename { .. } => "Rename Failed",
        QueueCommand::OpenFile(_) => "Open Failed",
        QueueCommand::RevealInFolder(_) => "Reveal Failed",
        QueueCommand::SwapFailedToBrowser(_) => "Swap Failed",
        QueueCommand::OpenProgress(_) => "Progress Popup Failed",
        QueueCommand::PauseAll => "Pause Queue Failed",
        QueueCommand::ResumeAll => "Resume Queue Failed",
        QueueCommand::RetryFailed => "Retry Failed Downloads Failed",
        QueueCommand::ClearCompleted => "Clear Completed Failed",
    }
}

fn queue_command_success_toasts(
    command: &QueueCommand,
    output: &QueueCommandOutput,
) -> Vec<ToastMessage> {
    let mut toasts = Vec::new();
    match command {
        QueueCommand::Retry(_) => toasts.push(toast_message(
            ToastType::Info,
            "Retrying Download",
            "The download was added back to the queue.",
        )),
        QueueCommand::Restart(_) => toasts.push(toast_message(
            ToastType::Info,
            "Restarting Download",
            "Partial progress was cleared and the download was queued again.",
        )),
        QueueCommand::DeleteMany {
            ids,
            delete_from_disk,
        } => {
            if ids.len() == 1 {
                toasts.push(toast_message(
                    ToastType::Success,
                    "Download Deleted",
                    if *delete_from_disk {
                        "Removed from the list and deleted from disk."
                    } else {
                        "Removed from the download list."
                    },
                ));
            } else {
                toasts.push(toast_message(
                    ToastType::Success,
                    "Downloads Deleted",
                    if *delete_from_disk {
                        format!(
                            "Removed {} downloads from the list and deleted their files from disk.",
                            ids.len()
                        )
                    } else {
                        format!("Removed {} downloads from the download list.", ids.len())
                    },
                ));
            }
        }
        QueueCommand::Rename { filename, .. } => toasts.push(toast_message(
            ToastType::Success,
            "Download Renamed",
            format!("Renamed to {filename}."),
        )),
        QueueCommand::SwapFailedToBrowser(_) => toasts.push(toast_message(
            ToastType::Info,
            "Swapped to Browser",
            "The download URL was opened in your browser.",
        )),
        QueueCommand::PauseAll => toasts.push(toast_message(
            ToastType::Info,
            "Queue Paused",
            "Active and queued downloads were paused.",
        )),
        QueueCommand::ResumeAll => toasts.push(toast_message(
            ToastType::Info,
            "Queue Resumed",
            "Paused and interrupted downloads were queued again.",
        )),
        QueueCommand::RetryFailed => toasts.push(toast_message(
            ToastType::Info,
            "Retrying Failed Downloads",
            "Failed downloads were added back to the queue.",
        )),
        QueueCommand::ClearCompleted => toasts.push(toast_message(
            ToastType::Info,
            "Completed Downloads Cleared",
            "Completed downloads were removed from the list.",
        )),
        QueueCommand::OpenFile(_) => {
            append_external_use_toast(&mut toasts, output, "file");
        }
        QueueCommand::RevealInFolder(_) => {
            append_external_use_toast(&mut toasts, output, "folder");
        }
        QueueCommand::Pause(_)
        | QueueCommand::Resume(_)
        | QueueCommand::Cancel(_)
        | QueueCommand::Remove(_)
        | QueueCommand::OpenProgress(_) => {}
    }
    toasts
}

fn append_external_use_toast(
    toasts: &mut Vec<ToastMessage>,
    output: &QueueCommandOutput,
    target: &'static str,
) {
    let Some(result) = output.external_use.as_ref() else {
        return;
    };
    if result.paused_torrent {
        toasts.push(toast_message(
            ToastType::Info,
            "Torrent Paused",
            external_use_auto_reseed_message(
                target,
                result.auto_reseed_retry_seconds.unwrap_or(60),
            ),
        ));
    }
}

fn wire_string_command<C>(
    ui: &MainWindow,
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    toast_state: ToastRuntimeState,
    register: StringCommandRegistrar,
    command: fn(String) -> QueueCommand,
) where
    C: QueueCommandSink,
{
    let window = ui.as_weak();
    register(
        ui,
        Box::new(move |id| {
            let command_sink = command_sink.clone();
            let command = command(id.to_string());
            let window = window.clone();
            let toast_state = toast_state.clone();
            runtime.spawn(async move {
                let result = command_sink.run_queue_command(command.clone()).await;
                dispatch_queue_command_result_to_weak(window, toast_state, command, result);
            });
        }),
    );
}

fn wire_void_command<C>(
    ui: &MainWindow,
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
    toast_state: ToastRuntimeState,
    register: fn(&MainWindow, Box<dyn FnMut()>),
    command: QueueCommand,
) where
    C: QueueCommandSink,
{
    let window = ui.as_weak();
    register(
        ui,
        Box::new(move || {
            let command_sink = command_sink.clone();
            let command = command.clone();
            let window = window.clone();
            let toast_state = toast_state.clone();
            runtime.spawn(async move {
                let result = command_sink.run_queue_command(command.clone()).await;
                dispatch_queue_command_result_to_weak(window, toast_state, command, result);
            });
        }),
    );
}
