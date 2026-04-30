use serde_json::{json, Value};
use simple_download_manager_desktop_core::backend::{CoreDesktopBackend, ProgressBatchRegistry};
use simple_download_manager_desktop_core::contracts::{
    BackendFuture, BrowserDownloadAccessProbe, BrowserDownloadAccessProbeFuture, DesktopEvent,
    ShellServices,
};
use simple_download_manager_desktop_core::host_protocol::{
    validate_host_request, HostRequest, PROTOCOL_VERSION,
};
use simple_download_manager_desktop_core::prompts::{
    PromptDecision, PromptDuplicateAction, PromptRegistry,
};
use simple_download_manager_desktop_core::state::SharedState;
use simple_download_manager_desktop_core::storage::{
    ConnectionState, DownloadSource, HandoffAuth, Settings,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

type ProbeRecord = (DownloadSource, String, Option<HandoffAuth>);

#[derive(Clone, Default)]
struct RecordingShell {
    events: Arc<Mutex<Vec<DesktopEvent>>>,
    focused_main_window_count: Arc<Mutex<usize>>,
    scheduled_downloads: Arc<Mutex<usize>>,
    progress_windows: Arc<Mutex<Vec<String>>>,
    probes: Arc<Mutex<Vec<ProbeRecord>>>,
}

impl RecordingShell {
    fn state_event_count(&self) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, DesktopEvent::StateChanged(_)))
            .count()
    }
}

impl ShellServices for RecordingShell {
    fn emit_event(&self, event: DesktopEvent) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
            Ok(())
        })
    }

    fn focus_main_window(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            *self.focused_main_window_count.lock().unwrap() += 1;
            Ok(())
        })
    }

    fn show_download_prompt_window(&self) -> BackendFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn show_progress_window(
        &self,
        id: String,
        _transfer_kind: simple_download_manager_desktop_core::storage::TransferKind,
    ) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.progress_windows.lock().unwrap().push(id);
            Ok(())
        })
    }

    fn schedule_downloads(&self, _state: SharedState) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            *self.scheduled_downloads.lock().unwrap() += 1;
            Ok(())
        })
    }

    fn probe_browser_download_access(
        &self,
        _state: SharedState,
        source: DownloadSource,
        url: String,
        handoff_auth: Option<HandoffAuth>,
    ) -> BrowserDownloadAccessProbeFuture<'_> {
        Box::pin(async move {
            self.probes
                .lock()
                .unwrap()
                .push((source, url, handoff_auth));
            Ok(BrowserDownloadAccessProbe { status: 206 })
        })
    }
}

#[test]
fn host_request_validation_rejects_oversized_request_ids() {
    let mut request = host_request("ping", json!({}));
    request.request_id = "x".repeat(129);

    let error = validate_host_request(&request).expect_err("oversized request id should reject");

    assert_eq!(error.code, Some("INVALID_PAYLOAD"));
}

#[test]
fn host_request_validation_rejects_handoff_auth_outside_browser_downloads() {
    let request = host_request(
        "enqueue_download",
        json!({
            "url": "https://example.com/file.zip",
            "source": {
                "entryPoint": "popup",
                "browser": "firefox",
                "extensionVersion": "0.3.52-alpha"
            },
            "handoffAuth": {
                "headers": [{ "name": "Cookie", "value": "session=abc" }]
            }
        }),
    );

    let error = validate_host_request(&request).expect_err("auth should reject");

    assert_eq!(error.code, Some("INVALID_PAYLOAD"));
}

#[tokio::test]
async fn host_ping_registers_contact_and_returns_ready_payload() {
    let shell = RecordingShell::default();
    let backend = backend_with_shell(shell.clone()).await;

    let response = backend
        .handle_host_request(host_request("ping", json!({})))
        .await;

    assert!(response.ok);
    assert_eq!(response.message_type, "ready");
    assert_eq!(
        response.payload.as_ref().and_then(|payload| {
            payload
                .get("connectionState")
                .and_then(|state| serde_json::from_value::<ConnectionState>(state.clone()).ok())
        }),
        Some(ConnectionState::Connected)
    );
    assert_eq!(shell.state_event_count(), 1);
}

#[tokio::test]
async fn host_open_app_focuses_main_window_and_returns_launched() {
    let shell = RecordingShell::default();
    let backend = backend_with_shell(shell.clone()).await;

    let response = backend
        .handle_host_request(host_request(
            "show_window",
            json!({ "reason": "reconnect" }),
        ))
        .await;

    assert!(response.ok);
    assert_eq!(response.message_type, "ready");
    assert_eq!(
        response
            .payload
            .as_ref()
            .and_then(|payload| payload.get("appState"))
            .and_then(Value::as_str),
        Some("launched")
    );
    assert_eq!(*shell.focused_main_window_count.lock().unwrap(), 1);
}

#[tokio::test]
async fn host_save_extension_settings_persists_and_emits_snapshot() {
    let shell = RecordingShell::default();
    let backend = backend_with_shell(shell.clone()).await;

    let response = backend
        .handle_host_request(host_request(
            "save_extension_settings",
            json!({
                "enabled": false,
                "downloadHandoffMode": "ask",
                "listenPort": 17654,
                "contextMenuEnabled": false,
                "showProgressAfterHandoff": false,
                "showBadgeStatus": false,
                "excludedHosts": [],
                "ignoredFileExtensions": [],
                "authenticatedHandoffEnabled": true,
                "authenticatedHandoffHosts": []
            }),
        ))
        .await;

    assert!(response.ok);
    assert!(
        !backend
            .state()
            .extension_integration_settings()
            .await
            .enabled
    );
    assert_eq!(shell.state_event_count(), 2);
}

#[tokio::test]
async fn host_enqueue_download_probes_schedules_and_returns_queued_result() {
    let shell = RecordingShell::default();
    let backend = backend_with_shell(shell.clone()).await;

    let response = backend
        .handle_host_request(host_request("enqueue_download", valid_enqueue_payload()))
        .await;

    assert!(response.ok);
    assert_eq!(response.message_type, "queued");
    assert_eq!(*shell.scheduled_downloads.lock().unwrap(), 1);
    assert_eq!(shell.probes.lock().unwrap().len(), 1);
    assert_eq!(
        response
            .payload
            .as_ref()
            .and_then(|payload| payload.get("status"))
            .and_then(Value::as_str),
        Some("queued")
    );
}

#[tokio::test]
async fn host_prompt_download_resolves_download_decision_into_enqueue_result() {
    let shell = RecordingShell::default();
    let prompts = PromptRegistry::default();
    let backend = backend_with_parts(shell.clone(), prompts.clone()).await;
    let handler = backend.clone();

    let task = tokio::spawn(async move {
        handler
            .handle_host_request(host_request("prompt_download", valid_enqueue_payload()))
            .await
    });

    let active_prompt = loop {
        if let Some(prompt) = prompts.active_prompt().await {
            break prompt;
        }
        tokio::task::yield_now().await;
    };
    prompts
        .resolve(
            &active_prompt.id,
            PromptDecision::Download {
                directory_override: None,
                duplicate_action: PromptDuplicateAction::DownloadAnyway,
                renamed_filename: None,
            },
        )
        .await
        .expect("prompt should resolve");

    let response = task.await.expect("prompt handler should join");

    assert!(response.ok);
    assert_eq!(response.message_type, "queued");
    assert_eq!(*shell.scheduled_downloads.lock().unwrap(), 1);
    assert_eq!(shell.probes.lock().unwrap().len(), 1);
}

fn host_request(message_type: &str, payload: Value) -> HostRequest {
    HostRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: format!("request-{message_type}"),
        message_type: message_type.into(),
        payload,
    }
}

fn valid_enqueue_payload() -> Value {
    json!({
        "url": "https://example.com/file.zip",
        "source": {
            "entryPoint": "browser_download",
            "browser": "firefox",
            "extensionVersion": "0.3.52-alpha"
        },
        "handoffAuth": {
            "headers": [
                { "name": "Cookie", "value": "session=abc" },
                { "name": "User-Agent", "value": "Firefox" }
            ]
        }
    })
}

async fn backend_with_shell(shell: RecordingShell) -> CoreDesktopBackend<RecordingShell> {
    backend_with_parts(shell, PromptRegistry::default()).await
}

async fn backend_with_parts(
    shell: RecordingShell,
    prompts: PromptRegistry,
) -> CoreDesktopBackend<RecordingShell> {
    let runtime_dir = test_runtime_dir("host-protocol");
    let state = SharedState::for_tests(runtime_dir.join("state.json"), Vec::new());
    let mut settings = Settings {
        download_directory: runtime_dir.join("downloads").display().to_string(),
        ..Settings::default()
    };
    settings.torrent.download_directory = runtime_dir.join("torrents").display().to_string();
    state
        .save_settings(settings)
        .await
        .expect("test state settings should save");
    CoreDesktopBackend::new(state, prompts, ProgressBatchRegistry::default(), shell)
}

fn test_runtime_dir(name: &str) -> PathBuf {
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::current_dir()
        .unwrap()
        .join("test-runtime")
        .join(format!("{name}-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
