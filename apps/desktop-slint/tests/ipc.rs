use simple_download_manager_desktop_core::backend::CoreDesktopBackend;
use simple_download_manager_desktop_core::contracts::{
    BackendFuture, DesktopBackend, DesktopEvent, ShellServices,
};
use simple_download_manager_desktop_core::prompts::PromptRegistry;
use simple_download_manager_desktop_core::state::SharedState;
use simple_download_manager_desktop_core::storage::{DiagnosticLevel, HostRegistrationDiagnostics};
use simple_download_manager_desktop_slint::ipc::{
    handle_request_line, read_limited_request_line_for_tests, PIPE_NAME,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn limited_reader_accepts_newline_framed_request() {
    let data = b"{\"type\":\"ping\"}\ntrailing".to_vec();
    let mut reader = tokio::io::BufReader::new(data.as_slice());
    let request = read_limited_request_line_for_tests(&mut reader, 1_024)
        .await
        .expect("newline-framed request should be accepted");

    assert_eq!(PIPE_NAME, r"\\.\pipe\myapp.downloads.v1");
    assert_eq!(request, "{\"type\":\"ping\"}\n");
}

#[tokio::test]
async fn limited_reader_rejects_oversized_payload_and_invalid_utf8() {
    let oversized_data = vec![b'a'; 8];
    let mut oversized = tokio::io::BufReader::new(oversized_data.as_slice());
    let error = read_limited_request_line_for_tests(&mut oversized, 4)
        .await
        .expect_err("oversized request should fail");
    assert!(error.contains("exceeds 4 bytes"));

    let invalid_utf8_data = vec![0xff, b'\n'];
    let mut invalid_utf8 = tokio::io::BufReader::new(invalid_utf8_data.as_slice());
    let error = read_limited_request_line_for_tests(&mut invalid_utf8, 8)
        .await
        .expect_err("invalid UTF-8 should fail");
    assert!(error.contains("not valid UTF-8"));
}

#[tokio::test]
async fn request_handler_ignores_empty_lines_and_frames_successful_responses() {
    let shell = RecordingShell::default();
    let backend = test_backend(shell.clone());

    let empty = handle_request_line(&backend, "\n")
        .await
        .expect("empty requests should be ignored");
    assert_eq!(empty, None);

    let response = handle_request_line(
        &backend,
        r#"{"protocolVersion":1,"requestId":"wake","type":"show_window","payload":{"reason":"user_request"}}"#,
    )
    .await
    .expect("show_window request should succeed")
    .expect("show_window should produce a response frame");

    assert!(response.ends_with('\n'));
    let parsed: serde_json::Value =
        serde_json::from_str(response.trim()).expect("response frame should be host JSON");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["requestId"], "wake");
    assert_eq!(parsed["type"], "ready");
    assert_eq!(parsed["payload"]["appState"], "launched");
    assert_eq!(shell.focus_count(), 1);
}

#[tokio::test]
async fn request_handler_records_warning_for_rejected_host_responses() {
    let shell = RecordingShell::default();
    let backend = test_backend(shell);

    let response = handle_request_line(
        &backend,
        r#"{"protocolVersion":1,"requestId":"bad","type":"unknown","payload":{}}"#,
    )
    .await
    .expect("invalid host request should still serialize a response")
    .expect("invalid host request should return an error frame");

    let parsed: serde_json::Value =
        serde_json::from_str(response.trim()).expect("response frame should be host JSON");
    assert_eq!(parsed["ok"], false);

    let diagnostics = backend
        .get_diagnostics()
        .await
        .expect("diagnostics should load");
    assert!(
        diagnostics.recent_events.iter().any(|event| {
            event.level == DiagnosticLevel::Warning && event.category == "native_host"
        }),
        "rejected host responses should record a native-host warning"
    );
}

#[derive(Clone, Default)]
struct RecordingShell {
    focus_count: Arc<Mutex<usize>>,
}

impl RecordingShell {
    fn focus_count(&self) -> usize {
        *self.focus_count.lock().unwrap()
    }
}

impl ShellServices for RecordingShell {
    fn focus_main_window(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            *self.focus_count.lock().unwrap() += 1;
            Ok(())
        })
    }

    fn emit_event(&self, _event: DesktopEvent) -> BackendFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn gather_host_registration_diagnostics(
        &self,
    ) -> BackendFuture<'_, HostRegistrationDiagnostics> {
        Box::pin(async { Ok(HostRegistrationDiagnostics::default()) })
    }
}

fn test_backend(shell: RecordingShell) -> CoreDesktopBackend<RecordingShell> {
    CoreDesktopBackend::new(
        SharedState::for_tests(test_storage_path(), Vec::new()),
        PromptRegistry::default(),
        Default::default(),
        shell,
    )
}

fn test_storage_path() -> PathBuf {
    let dir = std::env::temp_dir()
        .join("test-slint-ipc")
        .join(format!("{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("state.json")
}
