use simple_download_manager_desktop_core::contracts::{
    AppUpdateMetadata, UpdateInstallProgressEvent,
};
use simple_download_manager_desktop_slint::update::{
    apply_install_progress_event, begin_update_install, check_for_update_with, fail_update_check,
    finish_update_check, format_update_progress, metadata_from_update_fields, progress_percent,
    start_update_check, updater_config, AppUpdateState, PendingUpdateState, UpdateCheckMode,
    UpdateCommandError, SLINT_UPDATER_ENDPOINT, SLINT_UPDATER_FEED_NAME,
    TAURI_TRANSITION_FEED_NAME, TAURI_UPDATER_PUBLIC_KEY,
};

#[test]
fn updater_feed_names_preserve_tauri_transition_and_add_slint_channel() {
    assert_eq!(TAURI_TRANSITION_FEED_NAME, "latest-alpha.json");
    assert_eq!(SLINT_UPDATER_FEED_NAME, "latest-alpha-slint.json");
}

#[test]
fn updater_config_uses_slint_feed_public_key_and_passive_windows_install() {
    let config = updater_config().expect("updater config should build");

    assert_eq!(SLINT_UPDATER_ENDPOINT, "https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-alpha/latest-alpha-slint.json");
    assert_eq!(config.endpoints.len(), 1);
    assert_eq!(config.endpoints[0].as_str(), SLINT_UPDATER_ENDPOINT);
    assert_eq!(config.pubkey, TAURI_UPDATER_PUBLIC_KEY);
    let windows = config.windows.expect("Windows updater config");
    assert_eq!(format!("{:?}", windows.install_mode), "Some(Passive)");
}

#[test]
fn update_errors_preserve_command_codes_and_messages() {
    let no_pending = UpdateCommandError::no_pending_update();
    let updater = UpdateCommandError::updater("network unavailable");
    let internal = UpdateCommandError::internal("lock poisoned");

    assert_eq!(no_pending.code, "NO_PENDING_UPDATE");
    assert_eq!(no_pending.message, "There is no pending update to install.");
    assert_eq!(updater.code, "UPDATER_ERROR");
    assert_eq!(updater.message, "network unavailable");
    assert_eq!(internal.code, "INTERNAL_ERROR");
    assert_eq!(internal.message, "lock poisoned");
}

#[test]
fn update_metadata_mapping_preserves_version_date_and_notes() {
    let metadata = metadata_from_update_fields(
        "0.3.53-alpha",
        "0.3.52-alpha",
        Some("2026-05-01 00:00:00.0 +00:00:00".into()),
        Some("Updater polish".into()),
    );

    assert_eq!(
        metadata,
        AppUpdateMetadata {
            version: "0.3.53-alpha".into(),
            current_version: "0.3.52-alpha".into(),
            date: Some("2026-05-01 00:00:00.0 +00:00:00".into()),
            body: Some("Updater polish".into()),
        }
    );
}

#[test]
fn update_state_transitions_match_tauri_ui_logic() {
    let state = AppUpdateState::default();

    let checking = start_update_check(&state, UpdateCheckMode::Manual);
    assert_eq!(checking.status, "checking");
    assert_eq!(checking.last_check_mode, Some(UpdateCheckMode::Manual));

    let available = finish_update_check(
        &checking,
        Some(AppUpdateMetadata {
            version: "0.3.53-alpha".into(),
            current_version: "0.3.52-alpha".into(),
            date: None,
            body: Some("Updater polish".into()),
        }),
    );
    assert_eq!(available.status, "available");
    assert_eq!(
        available.available_update.as_ref().unwrap().version,
        "0.3.53-alpha"
    );
    assert_eq!(available.error_message, None);

    let downloading = begin_update_install(&available);
    assert_eq!(downloading.status, "downloading");
    assert_eq!(downloading.downloaded_bytes, 0);
    assert_eq!(downloading.total_bytes, None);

    let started = apply_install_progress_event(
        &available,
        UpdateInstallProgressEvent::Started {
            content_length: Some(100),
        },
    );
    assert_eq!(started.status, "downloading");
    assert_eq!(started.downloaded_bytes, 0);
    assert_eq!(started.total_bytes, Some(100));

    let progressed = apply_install_progress_event(
        &started,
        UpdateInstallProgressEvent::Progress { chunk_length: 25 },
    );
    assert_eq!(progressed.downloaded_bytes, 25);
    assert_eq!(progressed.total_bytes, Some(100));

    let finished = apply_install_progress_event(&progressed, UpdateInstallProgressEvent::Finished);
    assert_eq!(finished.status, "installing");

    let errored = fail_update_check(&checking, "offline");
    assert_eq!(errored.status, "error");
    assert_eq!(errored.error_message.as_deref(), Some("offline"));
}

#[test]
fn update_progress_formatting_is_stable() {
    assert_eq!(progress_percent(25, Some(100)), 25.0);
    assert_eq!(progress_percent(25, None), 0.0);
    assert_eq!(progress_percent(125, Some(100)), 100.0);
    assert_eq!(format_update_progress(25, Some(100)), "25 B / 100 B");
    assert_eq!(format_update_progress(1536, None), "1.5 KiB downloaded");
}

#[test]
fn pending_update_check_replaces_pending_slot_with_checked_update() {
    let pending = PendingUpdateState::default();

    let result = check_for_update_with(&pending, || Ok(None::<cargo_packager_updater::Update>))
        .expect("no update should be a clean result");

    assert_eq!(result, None);
    assert!(!pending.has_pending_update());
}
