use simple_download_manager_desktop_core::contracts::{
    DesktopEvent, ShellError, SELECT_JOB_EVENT, STATE_CHANGED_EVENT, UPDATE_INSTALL_PROGRESS_EVENT,
};
use simple_download_manager_desktop_core::prompts::{
    PromptDecision, PromptRegistry, PROMPT_CHANGED_EVENT,
};
use simple_download_manager_desktop_core::storage::{ConnectionState, DesktopSnapshot, Settings};

#[test]
fn desktop_event_names_match_existing_tauri_contracts() {
    assert_eq!(STATE_CHANGED_EVENT, "app://state-changed");
    assert_eq!(PROMPT_CHANGED_EVENT, "app://download-prompt-changed");
    assert_eq!(SELECT_JOB_EVENT, "app://select-job");
    assert_eq!(
        UPDATE_INSTALL_PROGRESS_EVENT,
        "app://update-install-progress"
    );
}

#[test]
fn desktop_events_carry_existing_snapshot_and_shell_error_payloads() {
    let snapshot = DesktopSnapshot {
        connection_state: ConnectionState::Connected,
        jobs: Vec::new(),
        settings: Settings::default(),
    };

    let state_event = DesktopEvent::StateChanged(Box::new(snapshot.clone()));
    let error_event = DesktopEvent::ShellError(ShellError {
        operation: "open_path".into(),
        message: "No associated app".into(),
    });

    assert!(matches!(
        state_event,
        DesktopEvent::StateChanged(value) if value.connection_state == ConnectionState::Connected
    ));
    assert!(matches!(
        error_event,
        DesktopEvent::ShellError(value)
            if value.operation == "open_path" && value.message == "No associated app"
    ));
}

#[tokio::test]
async fn prompt_registry_remains_available_from_core_contract_crate() {
    let registry = PromptRegistry::default();
    let receiver = registry
        .enqueue(
            simple_download_manager_desktop_core::storage::DownloadPrompt {
                id: "prompt_core".into(),
                url: "https://example.com/file.zip".into(),
                filename: "file.zip".into(),
                source: None,
                total_bytes: None,
                default_directory: "C:/Downloads".into(),
                target_path: "C:/Downloads/file.zip".into(),
                duplicate_job: None,
                duplicate_path: None,
                duplicate_filename: None,
                duplicate_reason: None,
            },
        )
        .await;

    registry
        .resolve("prompt_core", PromptDecision::Cancel)
        .await
        .expect("active prompt should resolve");

    assert!(matches!(receiver.await, Ok(PromptDecision::Cancel)));
}
