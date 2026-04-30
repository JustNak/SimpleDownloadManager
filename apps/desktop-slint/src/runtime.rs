use crate::controller::{slint_rows_from_snapshot, status_text_from_snapshot};
use crate::shell::{self, lifecycle, main_window};
use crate::update::{self, AppUpdateState, PendingUpdateState, UpdateCheckMode, UpdateStateStore};
use crate::MainWindow;
use simple_download_manager_desktop_core::backend::{
    prompt_enqueue_details, CoreDesktopBackend, ProgressBatchRegistry,
};
use simple_download_manager_desktop_core::contracts::{
    AppUpdateMetadata, BackendFuture, DesktopBackend, DesktopEvent, ProgressBatchContext,
    ShellServices,
};
use simple_download_manager_desktop_core::prompts::{PromptDecision, PromptRegistry};
use simple_download_manager_desktop_core::state::{EnqueueOptions, EnqueueStatus, SharedState};
use simple_download_manager_desktop_core::storage::{
    DesktopSnapshot, DownloadPrompt, DownloadSource, HostRegistrationDiagnostics, TorrentInfo,
    TorrentSettings, TransferKind,
};
use simple_download_manager_desktop_core::transfer::{self, TransferShell};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;
use std::sync::Arc;
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

#[derive(Clone)]
pub struct MainWindowDispatcher {
    window: slint::Weak<MainWindow>,
    state: SharedState,
}

impl MainWindowDispatcher {
    pub fn new(window: &MainWindow, state: SharedState) -> Self {
        Self {
            window: window.as_weak(),
            state,
        }
    }
}

impl UiDispatcher for MainWindowDispatcher {
    fn dispatch(&self, action: UiAction) -> Result<(), String> {
        let window = self.window.clone();
        let state = self.state.clone();
        slint::invoke_from_event_loop(move || {
            let Some(ui) = window.upgrade() else {
                return;
            };

            match action {
                UiAction::ApplySnapshot(snapshot) => {
                    apply_snapshot_to_main_window(&ui, &snapshot);
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
                DesktopEvent::SelectJobRequested(_) => {
                    dispatcher.dispatch(UiAction::FocusMainWindow)?;
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
    OpenProgress(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCommand {
    Check(UpdateCheckMode),
    Install,
}

pub trait QueueCommandSink: Send + Sync + 'static {
    fn run_queue_command(&self, command: QueueCommand) -> BackendFuture<'_, ()>;
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
            QueueCommand::OpenProgress(id) => self.open_progress_window(id),
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
    ui.show().map_err(|error| error.to_string())?;
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
    let dispatcher = MainWindowDispatcher::new(ui, state.clone());
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
    apply_snapshot_to_main_window(ui, &snapshot);
    wire_queue_command_callbacks(ui, runtime.clone(), backend.clone());
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
    let rows = slint_rows_from_snapshot(snapshot);
    let model = Rc::new(VecModel::from(rows));
    ui.set_jobs(ModelRc::from(model));
    ui.set_status_text(status_text_from_snapshot(snapshot).into());
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

pub fn wire_queue_command_callbacks<C>(ui: &MainWindow, runtime: Arc<Runtime>, command_sink: Arc<C>)
where
    C: QueueCommandSink,
{
    ui.on_add_download_requested(|| {
        eprintln!("add-download UI is not implemented in the Slint runtime yet");
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
        runtime,
        command_sink,
        MainWindow::on_open_progress_requested,
        QueueCommand::OpenProgress,
    );
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
