use simple_download_manager_desktop_core::backend::{CoreDesktopBackend, ProgressBatchRegistry};
use simple_download_manager_desktop_core::contracts::{
    AddJobRequest, DesktopBackend, DesktopEvent, ExternalUseResult, ProgressBatchContext,
    ProgressBatchKind, ShellServices,
};
use simple_download_manager_desktop_core::prompts::{
    PromptDecision, PromptDuplicateAction, PromptRegistry,
};
use simple_download_manager_desktop_core::state::SharedState;
use simple_download_manager_desktop_core::storage::{
    DownloadJob, DownloadPrompt, DownloadSource, HostRegistrationDiagnostics, JobState,
    ResumeSupport, Settings, TorrentSettings, TransferKind,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct RecordingShell {
    events: Arc<Mutex<Vec<DesktopEvent>>>,
    scheduled_downloads: Arc<Mutex<usize>>,
    autostart_values: Arc<Mutex<Vec<bool>>>,
    applied_torrent_settings: Arc<Mutex<Vec<TorrentSettings>>>,
    diagnostics_reports: Arc<Mutex<Vec<String>>>,
    progress_windows: Arc<Mutex<Vec<(String, TransferKind)>>>,
    batch_windows: Arc<Mutex<Vec<String>>>,
    focused_jobs: Arc<Mutex<Vec<String>>>,
    opened_urls: Arc<Mutex<Vec<String>>>,
    opened_paths: Arc<Mutex<Vec<String>>>,
    revealed_paths: Arc<Mutex<Vec<String>>>,
    external_reseeds: Arc<Mutex<Vec<String>>>,
    host_registration: Arc<Mutex<HostRegistrationDiagnostics>>,
}

impl RecordingShell {
    fn take_events(&self) -> Vec<DesktopEvent> {
        std::mem::take(&mut self.events.lock().unwrap())
    }
}

impl ShellServices for RecordingShell {
    fn emit_event(
        &self,
        event: DesktopEvent,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
            Ok(())
        })
    }

    fn show_download_prompt_window(
        &self,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn close_download_prompt_window(
        &self,
        _remember_position: bool,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn save_diagnostics_report(
        &self,
        report: String,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, Option<String>> {
        Box::pin(async move {
            self.diagnostics_reports.lock().unwrap().push(report);
            Ok(Some("diagnostics.json".into()))
        })
    }

    fn gather_host_registration_diagnostics(
        &self,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<
        '_,
        HostRegistrationDiagnostics,
    > {
        Box::pin(async move { Ok(self.host_registration.lock().unwrap().clone()) })
    }

    fn sync_autostart_setting(
        &self,
        enabled: bool,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async move {
            self.autostart_values.lock().unwrap().push(enabled);
            Ok(())
        })
    }

    fn schedule_downloads(
        &self,
        _state: SharedState,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async move {
            *self.scheduled_downloads.lock().unwrap() += 1;
            Ok(())
        })
    }

    fn apply_torrent_runtime_settings(
        &self,
        settings: TorrentSettings,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async move {
            self.applied_torrent_settings.lock().unwrap().push(settings);
            Ok(())
        })
    }

    fn open_url(
        &self,
        url: String,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async move {
            self.opened_urls.lock().unwrap().push(url);
            Ok(())
        })
    }

    fn open_path(
        &self,
        path: String,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async move {
            self.opened_paths.lock().unwrap().push(path);
            Ok(())
        })
    }

    fn reveal_path(
        &self,
        path: String,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async move {
            self.revealed_paths.lock().unwrap().push(path);
            Ok(())
        })
    }

    fn show_progress_window(
        &self,
        id: String,
        transfer_kind: TransferKind,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async move {
            self.progress_windows
                .lock()
                .unwrap()
                .push((id, transfer_kind));
            Ok(())
        })
    }

    fn show_batch_progress_window(
        &self,
        batch_id: String,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async move {
            self.batch_windows.lock().unwrap().push(batch_id);
            Ok(())
        })
    }

    fn focus_job_in_main_window(
        &self,
        id: String,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async move {
            self.focused_jobs.lock().unwrap().push(id);
            Ok(())
        })
    }

    fn schedule_external_reseed(
        &self,
        _state: SharedState,
        id: String,
    ) -> simple_download_manager_desktop_core::contracts::BackendFuture<'_, ()> {
        Box::pin(async move {
            self.external_reseeds.lock().unwrap().push(id);
            Ok(())
        })
    }
}

#[tokio::test]
async fn backend_add_job_emits_snapshot_and_schedules_queued_download() {
    let shell = RecordingShell::default();
    let backend = backend_with_jobs(shell.clone(), Vec::new()).await;

    let result = backend
        .add_job(AddJobRequest {
            url: "https://example.com/file.zip".into(),
            directory_override: None,
            filename_hint: Some("file.zip".into()),
            expected_sha256: None,
            transfer_kind: Some(TransferKind::Http),
        })
        .await
        .expect("add job should enqueue");

    assert_eq!(result.status.as_protocol_value(), "queued");
    assert_eq!(*shell.scheduled_downloads.lock().unwrap(), 1);
    assert!(shell.take_events().iter().any(
        |event| matches!(event, DesktopEvent::StateChanged(snapshot) if snapshot.jobs.len() == 1)
    ));
}

#[tokio::test]
async fn backend_save_settings_validates_syncs_autostart_and_applies_torrent_settings() {
    let shell = RecordingShell::default();
    let backend = backend_with_jobs(shell.clone(), Vec::new()).await;
    let download_directory = test_runtime_dir("settings").join("downloads");
    let torrent_directory = test_runtime_dir("settings").join("torrents");
    let mut settings = Settings {
        download_directory: download_directory.display().to_string(),
        start_on_startup: true,
        ..Settings::default()
    };
    settings.torrent.download_directory = torrent_directory.display().to_string();

    let saved = backend
        .save_settings(settings.clone())
        .await
        .expect("settings should save");

    assert_eq!(saved.download_directory, settings.download_directory);
    assert_eq!(shell.autostart_values.lock().unwrap().as_slice(), &[true]);
    assert_eq!(shell.applied_torrent_settings.lock().unwrap().len(), 1);
    assert_eq!(*shell.scheduled_downloads.lock().unwrap(), 1);
}

#[tokio::test]
async fn backend_exports_diagnostics_report_through_shell_service() {
    let shell = RecordingShell::default();
    let backend = backend_with_jobs(shell.clone(), Vec::new()).await;

    let path = backend
        .export_diagnostics_report()
        .await
        .expect("diagnostics export should succeed");

    assert_eq!(path.as_deref(), Some("diagnostics.json"));
    let reports = shell.diagnostics_reports.lock().unwrap();
    assert!(reports[0].contains("\"hostRegistration\""));
}

#[tokio::test]
async fn backend_prompt_confirmation_resolves_registry_and_emits_next_prompt() {
    let shell = RecordingShell::default();
    let prompts = PromptRegistry::default();
    let first = prompt("prompt_1");
    let second = prompt("prompt_2");
    let first_receiver = prompts.enqueue(first.clone()).await;
    let _second_receiver = prompts.enqueue(second.clone()).await;
    let backend = backend_with_parts(
        shell.clone(),
        Vec::new(),
        prompts,
        ProgressBatchRegistry::default(),
    )
    .await;

    backend
        .confirm_download_prompt(
            simple_download_manager_desktop_core::contracts::ConfirmPromptRequest {
                id: first.id,
                directory_override: None,
                duplicate_action: PromptDuplicateAction::DownloadAnyway,
                renamed_filename: None,
            },
        )
        .await
        .expect("prompt should resolve");

    assert!(matches!(
        first_receiver.await,
        Ok(PromptDecision::Download {
            duplicate_action: PromptDuplicateAction::DownloadAnyway,
            ..
        })
    ));
    assert!(shell.take_events().iter().any(
        |event| matches!(event, DesktopEvent::DownloadPromptChanged(Some(prompt)) if prompt.id == "prompt_2")
    ));
}

#[tokio::test]
async fn backend_progress_batch_registry_preserves_context_shape() {
    let shell = RecordingShell::default();
    let registry = ProgressBatchRegistry::default();
    let backend = backend_with_parts(
        shell.clone(),
        Vec::new(),
        PromptRegistry::default(),
        registry,
    )
    .await;
    let context = ProgressBatchContext {
        batch_id: "batch_123".into(),
        kind: ProgressBatchKind::Bulk,
        job_ids: vec!["job_1".into(), "job_2".into()],
        title: "Archive progress".into(),
        archive_name: Some("bundle.zip".into()),
    };

    let batch_id = backend
        .open_batch_progress_window(context.clone())
        .await
        .expect("batch window should open");
    let stored = backend
        .get_progress_batch_context(batch_id.clone())
        .await
        .expect("context lookup should succeed");

    assert_eq!(batch_id, "batch_123");
    assert_eq!(stored, Some(context));
    assert_eq!(
        shell.batch_windows.lock().unwrap().as_slice(),
        &["batch_123"]
    );
}

#[tokio::test]
async fn backend_open_and_reveal_delegate_paths_and_return_external_use_result() {
    let shell = RecordingShell::default();
    let target_path = test_runtime_dir("external-use").join("file.zip");
    std::fs::write(&target_path, b"finished").unwrap();
    let backend =
        backend_with_jobs(shell.clone(), vec![completed_job("job_1", &target_path)]).await;

    let opened = backend
        .open_job_file("job_1".into())
        .await
        .expect("file should open");
    let revealed = backend
        .reveal_job_in_folder("job_1".into())
        .await
        .expect("file should reveal");

    assert_eq!(
        opened,
        ExternalUseResult {
            paused_torrent: false,
            auto_reseed_retry_seconds: None
        }
    );
    assert!(!revealed.paused_torrent);
    assert_eq!(
        shell.opened_paths.lock().unwrap().as_slice(),
        &[target_path.display().to_string()]
    );
    assert_eq!(
        shell.revealed_paths.lock().unwrap().as_slice(),
        &[target_path.display().to_string()]
    );
}

async fn backend_with_jobs(
    shell: RecordingShell,
    jobs: Vec<DownloadJob>,
) -> CoreDesktopBackend<RecordingShell> {
    backend_with_parts(
        shell,
        jobs,
        PromptRegistry::default(),
        ProgressBatchRegistry::default(),
    )
    .await
}

async fn backend_with_parts(
    shell: RecordingShell,
    jobs: Vec<DownloadJob>,
    prompts: PromptRegistry,
    progress_batches: ProgressBatchRegistry,
) -> CoreDesktopBackend<RecordingShell> {
    let runtime_dir = test_runtime_dir("state");
    let state = SharedState::for_tests(runtime_dir.join("state.json"), jobs);
    let mut settings = Settings {
        download_directory: runtime_dir.join("downloads").display().to_string(),
        ..Settings::default()
    };
    settings.torrent.download_directory = runtime_dir.join("torrents").display().to_string();
    state
        .save_settings(settings)
        .await
        .expect("test state settings should save");
    CoreDesktopBackend::new(state, prompts, progress_batches, shell)
}

fn prompt(id: &str) -> DownloadPrompt {
    DownloadPrompt {
        id: id.into(),
        url: format!("https://example.com/{id}.zip"),
        filename: format!("{id}.zip"),
        source: None,
        total_bytes: None,
        default_directory: "C:/Downloads".into(),
        target_path: format!("C:/Downloads/{id}.zip"),
        duplicate_job: None,
        duplicate_path: None,
        duplicate_filename: None,
        duplicate_reason: None,
    }
}

fn completed_job(id: &str, target_path: &std::path::Path) -> DownloadJob {
    DownloadJob {
        id: id.into(),
        url: "https://example.com/file.zip".into(),
        filename: "file.zip".into(),
        source: Some(DownloadSource {
            entry_point: "browser_download".into(),
            browser: "chrome".into(),
            extension_version: "0.3.52".into(),
            page_url: None,
            page_title: None,
            referrer: None,
            incognito: Some(false),
        }),
        transfer_kind: TransferKind::Http,
        integrity_check: None,
        torrent: None,
        state: JobState::Completed,
        created_at: 0,
        progress: 100.0,
        total_bytes: 8,
        downloaded_bytes: 8,
        speed: 0,
        eta: 0,
        error: None,
        failure_category: None,
        resume_support: ResumeSupport::Supported,
        retry_attempts: 0,
        target_path: target_path.display().to_string(),
        temp_path: format!("{}.part", target_path.display()),
        artifact_exists: Some(true),
        bulk_archive: None,
    }
}

fn test_runtime_dir(name: &str) -> PathBuf {
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("backend-{name}-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
