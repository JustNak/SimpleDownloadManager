use simple_download_manager_desktop_core::backend::{CoreDesktopBackend, ProgressBatchRegistry};
use simple_download_manager_desktop_core::contracts::{
    AppUpdateMetadata, BackendFuture, DesktopBackend, DesktopEvent, ProgressBatchContext,
    ProgressBatchKind, ShellServices, UpdateInstallProgressEvent,
};
use simple_download_manager_desktop_core::host_protocol::HostRequest;
use simple_download_manager_desktop_core::prompts::PromptRegistry;
use simple_download_manager_desktop_core::state::SharedState;
use simple_download_manager_desktop_core::storage::{
    ConnectionState, DesktopSnapshot, DownloadJob, DownloadPrompt, JobState, Settings, TransferKind,
};
use simple_download_manager_desktop_slint::MainWindow;
use simple_download_manager_desktop_slint::{
    runtime::{
        apply_snapshot_to_main_window, apply_update_state_to_main_window,
        wire_queue_command_callbacks, wire_update_callbacks, QueueCommand, QueueCommandSink,
        SlintShellServices, UiAction, UiDispatcher, UpdateCommand, UpdateCommandSink,
    },
    shell::main_window,
    update::{AppUpdateState, UpdateCheckMode, UpdateStateStore},
};
use slint::{CloseRequestResponse, ComponentHandle, Model, PhysicalSize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn main_window_runtime_applies_snapshot_and_wires_queue_callbacks() {
    let ui = MainWindow::new().expect("main window should construct for runtime tests");
    let snapshot = test_snapshot(vec![download_job("job_1", JobState::Downloading)]);

    apply_snapshot_to_main_window(&ui, &snapshot);

    assert_eq!(
        ui.get_status_text().as_str(),
        "Connected to browser handoff | 1 download"
    );
    let jobs = ui.get_jobs();
    assert_eq!(jobs.row_count(), 1);
    let row = jobs.row_data(0).expect("first row should be present");
    assert_eq!(row.id.as_str(), "job_1");
    assert_eq!(row.filename.as_str(), "file-job_1.bin");
    assert_eq!(row.state.as_str(), "Downloading");

    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime should build"),
    );
    let sink = Arc::new(RecordingQueueCommandSink::default());

    wire_queue_command_callbacks(&ui, runtime.clone(), sink.clone());
    ui.invoke_pause_job_requested("job_pause".into());
    ui.invoke_resume_job_requested("job_resume".into());
    ui.invoke_cancel_job_requested("job_cancel".into());
    ui.invoke_open_progress_requested("job_progress".into());
    runtime.block_on(async {
        for _ in 0..20 {
            if sink.commands().len() == 4 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    let commands = sink.commands();
    assert_eq!(commands.len(), 4);
    assert!(commands.contains(&QueueCommand::Pause("job_pause".into())));
    assert!(commands.contains(&QueueCommand::Resume("job_resume".into())));
    assert!(commands.contains(&QueueCommand::Cancel("job_cancel".into())));
    assert!(commands.contains(&QueueCommand::OpenProgress("job_progress".into())));

    let update_state = AppUpdateState {
        status: "available".into(),
        available_update: Some(AppUpdateMetadata {
            version: "0.3.53-alpha".into(),
            current_version: "0.3.52-alpha".into(),
            date: Some("2026-05-01".into()),
            body: Some("Updater polish".into()),
        }),
        ..Default::default()
    };

    apply_update_state_to_main_window(&ui, &update_state);

    assert_eq!(
        ui.get_update_status_text().as_str(),
        "Update 0.3.53-alpha is ready."
    );
    assert_eq!(ui.get_update_current_version().as_str(), "0.3.52-alpha");
    assert_eq!(ui.get_update_new_version().as_str(), "0.3.53-alpha");
    assert_eq!(ui.get_update_body().as_str(), "Updater polish");
    assert_eq!(ui.get_update_error_text().as_str(), "");
    assert!(ui.get_update_can_check());
    assert!(ui.get_update_can_install());

    let update_store = UpdateStateStore::default();
    let update_sink = Arc::new(RecordingUpdateCommandSink::default());

    wire_update_callbacks(&ui, runtime.clone(), update_sink.clone(), update_store);
    ui.invoke_check_update_requested();
    ui.invoke_install_update_requested();
    runtime.block_on(async {
        for _ in 0..20 {
            if update_sink.commands().len() == 2 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    assert_eq!(
        update_sink.commands(),
        vec![
            UpdateCommand::Check(UpdateCheckMode::Manual),
            UpdateCommand::Install,
        ]
    );

    let state = SharedState::for_tests(test_storage_path("slint-main-window-close"), Vec::new());
    ui.window().set_size(PhysicalSize::new(1380, 740));
    let response = main_window::handle_main_window_close(&ui, &state);
    assert_eq!(response, CloseRequestResponse::HideWindow);
    let persisted = state
        .main_window_state_sync()
        .expect("close handler should persist main-window state");
    assert_eq!(persisted.width, 1380);
    assert_eq!(persisted.height, 740);
}

#[tokio::test]
async fn slint_shell_dispatches_state_changed_to_ui_bridge() {
    let dispatcher = RecordingUiDispatcher::default();
    let shell = SlintShellServices::new(dispatcher.clone());
    let snapshot = test_snapshot(vec![download_job("job_2", JobState::Queued)]);

    shell
        .emit_event(DesktopEvent::StateChanged(Box::new(snapshot)))
        .await
        .expect("state event should dispatch");

    let actions = dispatcher.actions();
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        UiAction::ApplySnapshot(snapshot) => {
            assert_eq!(snapshot.jobs.len(), 1);
            assert_eq!(snapshot.jobs[0].id, "job_2");
        }
        other => panic!("expected snapshot action, got {other:?}"),
    }
}

#[tokio::test]
async fn slint_shell_dispatches_update_progress_to_ui_bridge() {
    let dispatcher = RecordingUiDispatcher::default();
    let update_store = UpdateStateStore::default();
    let shell = SlintShellServices::with_update_state(
        dispatcher.clone(),
        ProgressBatchRegistry::default(),
        update_store.clone(),
        Default::default(),
    );

    shell
        .emit_event(DesktopEvent::UpdateInstallProgress(
            UpdateInstallProgressEvent::Started {
                content_length: Some(400),
            },
        ))
        .await
        .expect("install start should dispatch update state");
    shell
        .emit_event(DesktopEvent::UpdateInstallProgress(
            UpdateInstallProgressEvent::Progress { chunk_length: 100 },
        ))
        .await
        .expect("install progress should dispatch update state");

    assert_eq!(update_store.snapshot().status, "downloading");
    assert_eq!(update_store.snapshot().downloaded_bytes, 100);
    assert!(dispatcher.actions().iter().any(|action| {
        matches!(
            action,
            UiAction::ApplyUpdateState(state)
                if state.status == "downloading" && state.downloaded_bytes == 100
        )
    }));
}

#[tokio::test]
async fn slint_shell_schedule_downloads_delegates_to_core_scheduler() {
    let state = SharedState::for_tests(
        test_storage_path("slint-schedule-downloads"),
        vec![download_job("job_3", JobState::Queued)],
    );
    let dispatcher = RecordingUiDispatcher::default();
    let shell = SlintShellServices::new(dispatcher.clone());
    let backend = CoreDesktopBackend::new(
        state.clone(),
        PromptRegistry::default(),
        Default::default(),
        shell.clone(),
    );

    shell
        .schedule_downloads(state.clone())
        .await
        .expect("Slint shell should delegate scheduler to desktop-core");

    let snapshot = backend
        .get_app_snapshot()
        .await
        .expect("snapshot should still load after scheduling");
    assert_eq!(snapshot.jobs[0].state, JobState::Starting);
    assert!(
        dispatcher
            .actions()
            .iter()
            .any(|action| matches!(action, UiAction::ApplySnapshot(_))),
        "scheduler should emit a snapshot through the Slint shell"
    );
}

#[tokio::test]
async fn slint_shell_close_and_exit_dispatch_lifecycle_actions() {
    let dispatcher = RecordingUiDispatcher::default();
    let shell = SlintShellServices::new(dispatcher.clone());

    shell
        .close_to_tray()
        .await
        .expect("close-to-tray should dispatch hide action");
    shell
        .request_exit()
        .await
        .expect("request-exit should dispatch exit action");

    let actions = dispatcher.actions();
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, UiAction::HideMainWindow)),
        "close-to-tray should hide the main window"
    );
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, UiAction::RequestExit)),
        "request-exit should quit through the UI event loop"
    );
}

#[tokio::test]
async fn host_show_window_request_dispatches_focus_action() {
    let state = SharedState::for_tests(test_storage_path("slint-host-show-window"), Vec::new());
    let dispatcher = RecordingUiDispatcher::default();
    let shell = SlintShellServices::new(dispatcher.clone());
    let backend =
        CoreDesktopBackend::new(state, PromptRegistry::default(), Default::default(), shell);
    let request: HostRequest = serde_json::from_str(
        r#"{"protocolVersion":1,"requestId":"wake","type":"show_window","payload":{"reason":"user_request"}}"#,
    )
    .expect("show_window host request should parse");

    let response = backend.handle_host_request(request).await;

    assert!(response.ok);
    assert_eq!(response.message_type, "ready");
    assert_eq!(
        response
            .payload
            .as_ref()
            .and_then(|payload| payload.get("appState"))
            .and_then(|value| value.as_str()),
        Some("launched")
    );
    assert!(
        dispatcher
            .actions()
            .iter()
            .any(|action| matches!(action, UiAction::FocusMainWindow)),
        "show_window host requests should focus the Slint main window"
    );
}

#[tokio::test]
async fn slint_shell_dispatches_prompt_and_progress_popup_actions() {
    let dispatcher = RecordingUiDispatcher::default();
    let progress_batches = ProgressBatchRegistry::default();
    progress_batches.store(ProgressBatchContext {
        batch_id: "batch_1".into(),
        kind: ProgressBatchKind::Multi,
        job_ids: vec!["job_http".into(), "job_torrent".into()],
        title: "Two downloads".into(),
        archive_name: None,
    });
    let shell = SlintShellServices::with_progress_batches(dispatcher.clone(), progress_batches);
    let prompt = download_prompt("prompt_1");

    shell
        .emit_event(DesktopEvent::DownloadPromptChanged(Some(Box::new(
            prompt.clone(),
        ))))
        .await
        .expect("prompt change should dispatch");
    shell
        .show_download_prompt_window()
        .await
        .expect("prompt window should dispatch");
    shell
        .close_download_prompt_window(true)
        .await
        .expect("prompt close should dispatch");
    shell
        .show_progress_window("job_http".into(), TransferKind::Http)
        .await
        .expect("HTTP progress window should dispatch");
    shell
        .show_progress_window("job_torrent".into(), TransferKind::Torrent)
        .await
        .expect("torrent progress window should dispatch");
    shell
        .show_batch_progress_window("batch_1".into())
        .await
        .expect("batch progress window should dispatch");

    let actions = dispatcher.actions();
    assert!(actions.iter().any(|action| {
        matches!(
            action,
            UiAction::DownloadPromptChanged(Some(next_prompt))
                if next_prompt.id == prompt.id
        )
    }));
    assert!(actions
        .iter()
        .any(|action| matches!(action, UiAction::ShowDownloadPromptWindow)));
    assert!(actions.iter().any(|action| {
        matches!(
            action,
            UiAction::CloseDownloadPromptWindow {
                remember_position: true
            }
        )
    }));
    assert!(actions.iter().any(|action| {
        matches!(
            action,
            UiAction::ShowProgressWindow {
                id,
                transfer_kind: TransferKind::Http
            } if id == "job_http"
        )
    }));
    assert!(actions.iter().any(|action| {
        matches!(
            action,
            UiAction::ShowProgressWindow {
                id,
                transfer_kind: TransferKind::Torrent
            } if id == "job_torrent"
        )
    }));
    assert!(actions.iter().any(|action| {
        matches!(
            action,
            UiAction::ShowBatchProgressWindow {
                batch_id,
                context: Some(context)
            } if batch_id == "batch_1" && context.title == "Two downloads"
        )
    }));
}

#[tokio::test]
async fn slint_shell_test_extension_handoff_opens_prompt_through_popup_lifecycle() {
    let state = SharedState::for_tests(
        test_storage_path("slint-test-extension-handoff"),
        Vec::new(),
    );
    let prompts = PromptRegistry::default();
    let dispatcher = RecordingUiDispatcher::default();
    let shell = SlintShellServices::new(dispatcher.clone());

    shell
        .test_extension_handoff(state, prompts.clone())
        .await
        .expect("Slint shell should create the extension handoff test prompt");

    let active_prompt = prompts
        .active_prompt()
        .await
        .expect("extension handoff test should enqueue an active prompt");
    assert!(active_prompt.id.starts_with("test_handoff_"));
    assert_eq!(
        active_prompt.url,
        "https://example.com/simple-download-manager-test.bin"
    );
    assert_eq!(active_prompt.filename, "simple-download-manager-test.bin");
    assert_eq!(active_prompt.total_bytes, Some(1_048_576));
    let source = active_prompt
        .source
        .expect("test prompt should carry source metadata");
    assert_eq!(source.entry_point, "browser_download");
    assert_eq!(source.browser, "chrome");
    assert_eq!(source.extension_version, "settings-test");

    let actions = dispatcher.actions();
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, UiAction::ShowDownloadPromptWindow)),
        "handoff test should show the Slint prompt window"
    );
    assert!(actions.iter().any(|action| {
        matches!(
            action,
            UiAction::DownloadPromptChanged(Some(prompt))
                if prompt.filename == "simple-download-manager-test.bin"
        )
    }));
}

#[tokio::test]
async fn slint_shell_queues_selected_job_request_once() {
    let dispatcher = RecordingUiDispatcher::default();
    let shell = SlintShellServices::new(dispatcher.clone());

    shell
        .focus_job_in_main_window("job_7".into())
        .await
        .expect("focus job should dispatch");

    assert_eq!(
        shell
            .take_pending_selected_job_request()
            .await
            .expect("pending selected job should be readable"),
        Some("job_7".into())
    );
    assert_eq!(
        shell
            .take_pending_selected_job_request()
            .await
            .expect("pending selected job should be readable"),
        None
    );
    assert!(dispatcher
        .actions()
        .iter()
        .any(|action| matches!(action, UiAction::FocusJobInMainWindow { id } if id == "job_7")));
}

#[test]
fn runtime_wires_main_window_lifecycle_helpers() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let runtime_source = std::fs::read_to_string(manifest_dir.join("src/runtime.rs"))
        .expect("runtime source should load");
    let main_window_source = std::fs::read_to_string(manifest_dir.join("src/shell/main_window.rs"))
        .expect("main-window source should load");
    let popup_source = std::fs::read_to_string(manifest_dir.join("src/shell/popups.rs"))
        .expect("popup source should load");

    assert!(
        runtime_source.contains("main_window::initialize_main_window(ui, &state)"),
        "bootstrap should restore persisted main-window state and install close handling"
    );
    assert!(
        runtime_source.contains("main_window::show_main_window(&ui)"),
        "FocusMainWindow should route through the Slint main-window lifecycle helper"
    );
    assert!(
        runtime_source.contains("main_window::hide_main_window(&ui)"),
        "HideMainWindow should route through the Slint main-window lifecycle helper"
    );
    assert!(
        runtime_source.contains("main_window::request_exit(&ui, &state)"),
        "RequestExit should persist main-window state and quit the Slint event loop"
    );
    assert!(
        runtime_source.contains("shell::tray::create_system_tray")
            && runtime_source.contains("slint::run_event_loop_until_quit"),
        "run_app should create the tray before entering an event loop that survives hidden windows"
    );
    assert!(
        main_window_source.contains("on_close_requested")
            && main_window_source.contains("save_main_window_state_sync"),
        "main-window close handling should persist geometry through SharedState"
    );
    assert!(
        runtime_source.contains("shell::popups::with_popup_registry"),
        "runtime should route popup actions through the Slint popup lifecycle registry"
    );
    assert!(
        !runtime_source.contains("progress window requested for")
            && !runtime_source.contains("batch progress window requested for"),
        "runtime should not keep placeholder progress popup logging"
    );
    assert!(
        popup_source.contains("DownloadPromptWindow")
            && popup_source.contains("HttpProgressWindow")
            && popup_source.contains("TorrentProgressWindow")
            && popup_source.contains("BatchProgressWindow"),
        "popup registry should own Slint popup window components"
    );
}

#[test]
fn slint_shell_services_delegate_native_shell_effects_through_shell_module() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let runtime_source = std::fs::read_to_string(manifest_dir.join("src/runtime.rs"))
        .expect("runtime source should load");

    for expected_call in [
        "shell::windows::browse_directory",
        "shell::windows::browse_torrent_file",
        "shell::windows::save_diagnostics_report",
        "shell::notifications::show_notification",
        "shell::native_host::gather_host_registration_diagnostics",
        "shell::native_host::register_native_host",
        "run_test_extension_handoff(self.clone(), state, prompts)",
        "update::check_for_update(&pending_update)",
        "update::install_update_with_progress",
        "shell::windows::open_url",
        "shell::windows::open_path",
        "shell::windows::reveal_path",
        "shell::windows::open_install_docs",
        "shell::windows::sync_autostart_setting",
    ] {
        assert!(
            runtime_source.contains(expected_call),
            "SlintShellServices should delegate native shell effect through {expected_call}"
        );
    }
}

#[derive(Clone, Default)]
struct RecordingUiDispatcher {
    actions: Arc<Mutex<Vec<UiAction>>>,
}

impl RecordingUiDispatcher {
    fn actions(&self) -> Vec<UiAction> {
        self.actions.lock().unwrap().clone()
    }
}

impl UiDispatcher for RecordingUiDispatcher {
    fn dispatch(&self, action: UiAction) -> Result<(), String> {
        self.actions.lock().unwrap().push(action);
        Ok(())
    }
}

#[derive(Default)]
struct RecordingQueueCommandSink {
    commands: Mutex<Vec<QueueCommand>>,
}

impl RecordingQueueCommandSink {
    fn commands(&self) -> Vec<QueueCommand> {
        self.commands.lock().unwrap().clone()
    }
}

impl QueueCommandSink for RecordingQueueCommandSink {
    fn run_queue_command(&self, command: QueueCommand) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.commands.lock().unwrap().push(command);
            Ok(())
        })
    }
}

#[derive(Default)]
struct RecordingUpdateCommandSink {
    commands: Mutex<Vec<UpdateCommand>>,
}

impl RecordingUpdateCommandSink {
    fn commands(&self) -> Vec<UpdateCommand> {
        self.commands.lock().unwrap().clone()
    }
}

impl UpdateCommandSink for RecordingUpdateCommandSink {
    fn run_update_command(
        &self,
        command: UpdateCommand,
    ) -> BackendFuture<'_, Option<AppUpdateMetadata>> {
        Box::pin(async move {
            self.commands.lock().unwrap().push(command);
            Ok(None)
        })
    }
}

fn test_snapshot(jobs: Vec<DownloadJob>) -> DesktopSnapshot {
    DesktopSnapshot {
        connection_state: ConnectionState::Connected,
        jobs,
        settings: Settings::default(),
    }
}

fn download_job(id: &str, state: JobState) -> DownloadJob {
    DownloadJob {
        id: id.into(),
        url: format!("https://example.test/{id}.bin"),
        filename: format!("file-{id}.bin"),
        source: None,
        transfer_kind: TransferKind::Http,
        integrity_check: None,
        torrent: None,
        state,
        created_at: 1,
        progress: 12.5,
        total_bytes: 200,
        downloaded_bytes: 25,
        speed: 0,
        eta: 0,
        error: None,
        failure_category: None,
        resume_support: Default::default(),
        retry_attempts: 0,
        target_path: format!("C:/Downloads/file-{id}.bin"),
        temp_path: format!("C:/Downloads/file-{id}.bin.part"),
        artifact_exists: None,
        bulk_archive: None,
    }
}

fn download_prompt(id: &str) -> DownloadPrompt {
    DownloadPrompt {
        id: id.into(),
        url: "https://example.test/archive.zip".into(),
        filename: "archive.zip".into(),
        source: None,
        total_bytes: Some(4096),
        default_directory: "C:/Downloads".into(),
        target_path: "C:/Downloads/archive.zip".into(),
        duplicate_job: None,
        duplicate_path: None,
        duplicate_filename: None,
        duplicate_reason: None,
    }
}

fn test_storage_path(name: &str) -> PathBuf {
    let dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("state.json")
}
