use crate::controller::{slint_rows_from_snapshot, status_text_from_snapshot};
use crate::MainWindow;
use simple_download_manager_desktop_core::backend::{
    CoreDesktopBackend, ProgressBatchRegistry,
};
use simple_download_manager_desktop_core::contracts::{
    AppUpdateMetadata, BackendFuture, DesktopBackend, DesktopEvent, ShellServices,
};
use simple_download_manager_desktop_core::prompts::PromptRegistry;
use simple_download_manager_desktop_core::state::SharedState;
use simple_download_manager_desktop_core::storage::{
    DesktopSnapshot, HostRegistrationDiagnostics, TorrentInfo, TorrentSettings, TransferKind,
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
    Notify {
        title: String,
        body: String,
    },
    FocusMainWindow,
    ShowProgressWindow {
        id: String,
        transfer_kind: TransferKind,
    },
    ShowBatchProgressWindow {
        batch_id: String,
    },
}

pub trait UiDispatcher: Clone + Send + Sync + 'static {
    fn dispatch(&self, action: UiAction) -> Result<(), String>;
}

#[derive(Clone)]
pub struct MainWindowDispatcher {
    window: slint::Weak<MainWindow>,
}

impl MainWindowDispatcher {
    pub fn new(window: &MainWindow) -> Self {
        Self {
            window: window.as_weak(),
        }
    }
}

impl UiDispatcher for MainWindowDispatcher {
    fn dispatch(&self, action: UiAction) -> Result<(), String> {
        let window = self.window.clone();
        slint::invoke_from_event_loop(move || {
            let Some(ui) = window.upgrade() else {
                return;
            };

            match action {
                UiAction::ApplySnapshot(snapshot) => apply_snapshot_to_main_window(&ui, &snapshot),
                UiAction::Notify { title, body } => {
                    eprintln!("{title}: {body}");
                }
                UiAction::FocusMainWindow => {
                    let _ = ui.show();
                }
                UiAction::ShowProgressWindow { id, transfer_kind } => {
                    eprintln!("progress window requested for {id} ({transfer_kind:?})");
                }
                UiAction::ShowBatchProgressWindow { batch_id } => {
                    eprintln!("batch progress window requested for {batch_id}");
                }
            }
        })
        .map_err(|error| error.to_string())
    }
}

#[derive(Clone)]
pub struct SlintShellServices<D>
where
    D: UiDispatcher,
{
    dispatcher: D,
}

impl<D> SlintShellServices<D>
where
    D: UiDispatcher,
{
    pub fn new(dispatcher: D) -> Self {
        Self { dispatcher }
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
                DesktopEvent::DownloadPromptChanged(_) => {}
                DesktopEvent::SelectJobRequested(_) => {
                    dispatcher.dispatch(UiAction::FocusMainWindow)?;
                }
                DesktopEvent::UpdateInstallProgress(_) => {}
                DesktopEvent::ShellError(error) => {
                    eprintln!("shell error during {}: {}", error.operation, error.message);
                }
            }
            Ok(())
        })
    }

    fn notify(&self, title: String, body: String) -> BackendFuture<'_, ()> {
        let dispatcher = self.dispatcher.clone();
        Box::pin(async move {
            dispatcher.dispatch(UiAction::Notify { title, body })?;
            Ok(())
        })
    }

    fn focus_main_window(&self) -> BackendFuture<'_, ()> {
        let dispatcher = self.dispatcher.clone();
        Box::pin(async move {
            dispatcher.dispatch(UiAction::FocusMainWindow)?;
            Ok(())
        })
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
        Box::pin(async move {
            dispatcher.dispatch(UiAction::ShowBatchProgressWindow { batch_id })?;
            Ok(())
        })
    }

    fn gather_host_registration_diagnostics(
        &self,
    ) -> BackendFuture<'_, HostRegistrationDiagnostics> {
        Box::pin(async { Ok(HostRegistrationDiagnostics::default()) })
    }

    fn sync_autostart_setting(&self, _enabled: bool) -> BackendFuture<'_, ()> {
        Box::pin(async { Ok(()) })
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

    fn check_for_update(&self) -> BackendFuture<'_, Option<AppUpdateMetadata>> {
        Box::pin(async { Ok(None) })
    }

    fn install_update(&self) -> BackendFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueCommand {
    Pause(String),
    Resume(String),
    Cancel(String),
    OpenProgress(String),
}

pub trait QueueCommandSink: Send + Sync + 'static {
    fn run_queue_command(&self, command: QueueCommand) -> BackendFuture<'_, ()>;
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

pub struct SlintRuntime<D>
where
    D: UiDispatcher,
{
    runtime: Arc<Runtime>,
    backend: Arc<SlintBackend<D>>,
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
}

pub fn run_app() -> Result<(), String> {
    let ui = MainWindow::new().map_err(|error| error.to_string())?;
    let _runtime = bootstrap_main_window(&ui)?;
    ui.run().map_err(|error| error.to_string())
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
    let runtime = Arc::new(
        Builder::new_multi_thread()
            .enable_all()
            .thread_name("sdm-slint-backend")
            .build()
            .map_err(|error| format!("Could not initialize Slint backend runtime: {error}"))?,
    );
    let dispatcher = MainWindowDispatcher::new(ui);
    let shell = SlintShellServices::new(dispatcher);
    let backend = Arc::new(CoreDesktopBackend::new(
        state.clone(),
        PromptRegistry::default(),
        ProgressBatchRegistry::default(),
        shell.clone(),
    ));

    let snapshot = runtime.block_on(backend.get_app_snapshot())?;
    apply_snapshot_to_main_window(ui, &snapshot);
    wire_queue_command_callbacks(ui, runtime.clone(), backend.clone());

    let startup_shell = shell.clone();
    runtime.spawn(async move {
        if let Err(error) = startup_shell.schedule_downloads(state).await {
            eprintln!("failed to schedule persisted downloads: {error}");
        }
    });

    Ok(SlintRuntime { runtime, backend })
}

pub fn apply_snapshot_to_main_window(ui: &MainWindow, snapshot: &DesktopSnapshot) {
    let rows = slint_rows_from_snapshot(snapshot);
    let model = Rc::new(VecModel::from(rows));
    ui.set_jobs(ModelRc::from(model));
    ui.set_status_text(status_text_from_snapshot(snapshot).into());
}

pub fn wire_queue_command_callbacks<C>(
    ui: &MainWindow,
    runtime: Arc<Runtime>,
    command_sink: Arc<C>,
) where
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
