use simple_download_manager_desktop_core::backend::CoreDesktopBackend;
use simple_download_manager_desktop_core::contracts::{
    BackendFuture, DesktopBackend, DesktopEvent, ShellServices,
};
use simple_download_manager_desktop_core::prompts::PromptRegistry;
use simple_download_manager_desktop_core::state::SharedState;
use simple_download_manager_desktop_core::storage::{
    ConnectionState, DesktopSnapshot, DownloadJob, JobState, Settings, TransferKind,
};
use simple_download_manager_desktop_slint::runtime::{
    apply_snapshot_to_main_window, wire_queue_command_callbacks, QueueCommand, QueueCommandSink,
    SlintShellServices, UiAction, UiDispatcher,
};
use simple_download_manager_desktop_slint::MainWindow;
use slint::Model;
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

fn test_storage_path(name: &str) -> PathBuf {
    let dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("state.json")
}
