use simple_download_manager_desktop_core::backend::{CoreDesktopBackend, ProgressBatchRegistry};
use simple_download_manager_desktop_core::contracts::{
    AddJobRequest, AddJobResult, AddJobStatus, AddJobsRequest, AddJobsResult, AppUpdateMetadata,
    BackendFuture, ConfirmPromptRequest, DesktopBackend, DesktopEvent, ExternalUseResult,
    ProgressBatchContext, ProgressBatchKind, ShellError, ShellServices, UpdateInstallProgressEvent,
};
use simple_download_manager_desktop_core::host_protocol::HostRequest;
use simple_download_manager_desktop_core::prompts::{PromptDuplicateAction, PromptRegistry};
use simple_download_manager_desktop_core::state::{SharedState, TorrentSessionCacheClearResult};
use simple_download_manager_desktop_core::storage::{
    ConnectionState, DesktopSnapshot, DiagnosticEvent, DiagnosticLevel, DiagnosticsSnapshot,
    DownloadHandoffMode, DownloadJob, DownloadPrompt, DownloadSource, HostRegistrationDiagnostics,
    HostRegistrationEntry, HostRegistrationStatus, JobState, QueueSummary, Settings, TorrentInfo,
    TorrentPeerConnectionWatchdogMode, TorrentSeedMode, TransferKind,
};
use simple_download_manager_desktop_slint::MainWindow;
use simple_download_manager_desktop_slint::{
    runtime::{
        apply_snapshot_to_main_window, apply_update_state_to_main_window,
        wire_add_download_callbacks, wire_diagnostics_callbacks,
        wire_main_window_lifecycle_callbacks, wire_progress_popup_action_bridge,
        wire_prompt_window_action_bridge, wire_queue_command_callbacks, wire_settings_callbacks,
        wire_toast_callbacks, wire_update_callbacks, AddDownloadCommandSink,
        AddDownloadRuntimeState, DiagnosticsCommandSink, DiagnosticsRuntimeState,
        MainWindowLifecycleCommand, MainWindowLifecycleSink, ProgressPopupCommandSink,
        PromptWindowCommandSink, QueueCommand, QueueCommandOutput, QueueCommandSink,
        QueueViewRuntimeState, SettingsCommandSink, SettingsRuntimeState, SlintShellServices,
        ToastRuntimeState, UiAction, UiDispatcher, UpdateCommand, UpdateCommandSink,
    },
    shell::{main_window, popups},
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

    let queue_view = QueueViewRuntimeState::default();
    let toast_state = ToastRuntimeState::default();
    wire_toast_callbacks(&ui, toast_state.clone());
    let manual_toast_id = toast_state.add_toast(
        &ui,
        simple_download_manager_desktop_slint::controller::toast_message(
            simple_download_manager_desktop_slint::controller::ToastType::Info,
            "Queue Paused",
            "Active and queued downloads were paused.",
        ),
    );
    assert_eq!(ui.get_toasts().row_count(), 1);
    let manual_toast = ui
        .get_toasts()
        .row_data(0)
        .expect("manual toast row should be visible");
    assert_eq!(manual_toast.id.as_str(), manual_toast_id);
    assert_eq!(manual_toast.toast_type.as_str(), "info");
    assert_eq!(manual_toast.title.as_str(), "Queue Paused");
    assert!(manual_toast.auto_close);
    toast_state.dismiss_toast(&ui, &manual_toast_id);
    assert_eq!(ui.get_toasts().row_count(), 0);

    queue_view.apply_snapshot_to_main_window(&ui, &snapshot);
    wire_queue_command_callbacks(
        &ui,
        runtime.clone(),
        sink.clone(),
        queue_view.clone(),
        toast_state.clone(),
    );
    assert_eq!(ui.get_queue_title().as_str(), "All downloads");
    assert_eq!(ui.get_queue_selected_count(), 0);
    assert_eq!(ui.get_nav_items().row_count(), 18);

    ui.invoke_view_change_requested("completed".into());
    assert_eq!(ui.get_queue_title().as_str(), "Completed");
    assert_eq!(ui.get_jobs().row_count(), 0);
    ui.invoke_view_change_requested("all".into());
    ui.invoke_search_query_changed("file-job_1".into());
    assert_eq!(ui.get_jobs().row_count(), 1);
    ui.invoke_sort_column_requested("name".into());
    assert_eq!(ui.get_queue_sort_column().as_str(), "name");
    ui.invoke_job_selection_requested("job_1".into());
    assert_eq!(ui.get_queue_selected_count(), 1);
    ui.invoke_clear_selection_requested();
    assert_eq!(ui.get_queue_selected_count(), 0);

    ui.invoke_pause_job_requested("job_pause".into());
    ui.invoke_resume_job_requested("job_resume".into());
    ui.invoke_cancel_job_requested("job_cancel".into());
    ui.invoke_retry_job_requested("job_retry".into());
    ui.invoke_restart_job_requested("job_restart".into());
    ui.invoke_open_progress_requested("job_progress".into());
    ui.invoke_pause_all_requested();
    ui.invoke_resume_all_requested();
    ui.invoke_retry_failed_requested();
    ui.invoke_clear_completed_requested();
    runtime.block_on(async {
        for _ in 0..20 {
            if sink.commands().len() == 10 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    let commands = sink.commands();
    assert_eq!(commands.len(), 10);
    assert!(commands.contains(&QueueCommand::Pause("job_pause".into())));
    assert!(commands.contains(&QueueCommand::Resume("job_resume".into())));
    assert!(commands.contains(&QueueCommand::Cancel("job_cancel".into())));
    assert!(commands.contains(&QueueCommand::Retry("job_retry".into())));
    assert!(commands.contains(&QueueCommand::Restart("job_restart".into())));
    assert!(commands.contains(&QueueCommand::OpenProgress("job_progress".into())));
    assert!(commands.contains(&QueueCommand::PauseAll));
    assert!(commands.contains(&QueueCommand::ResumeAll));
    assert!(commands.contains(&QueueCommand::RetryFailed));
    assert!(commands.contains(&QueueCommand::ClearCompleted));
    drain_slint_events();
    wait_for_toast_present(&toast_state, "Retrying Download", "added back to the queue");

    sink.set_next_error("cancel failed");
    ui.invoke_cancel_job_requested("job_cancel_error".into());
    runtime.block_on(async {
        for _ in 0..20 {
            if sink.commands().len() == 11 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    wait_for_toast_present(&toast_state, "Cancel Failed", "cancel failed");

    let mixed_snapshot = test_snapshot(vec![download_job("job_http", JobState::Queued), {
        let mut job = download_job("job_torrent", JobState::Downloading);
        job.transfer_kind = TransferKind::Torrent;
        job
    }]);
    queue_view.apply_snapshot_to_main_window(&ui, &mixed_snapshot);
    queue_view.select_job_in_main_window(&ui, "job_torrent");
    assert_eq!(ui.get_queue_view_id().as_str(), "torrents");
    assert_eq!(ui.get_queue_selected_count(), 1);
    let selected_row = ui
        .get_jobs()
        .row_data(0)
        .expect("torrent job should be visible after focus");
    assert_eq!(selected_row.id.as_str(), "job_torrent");
    assert!(selected_row.selected);

    let command_snapshot = test_snapshot(vec![
        download_job("job_delete", JobState::Queued),
        download_job("job_extra", JobState::Canceled),
        download_job("job_active", JobState::Downloading),
        {
            let mut job = download_job("job_seed", JobState::Paused);
            job.transfer_kind = TransferKind::Torrent;
            job.filename = "Seeded Torrent".into();
            job.target_path = "E:/Download/Other/Seeded Torrent".into();
            job.torrent = Some(TorrentInfo {
                uploaded_bytes: 2048,
                fetched_bytes: 4096,
                ratio: 0.5,
                seeding_started_at: Some(123_456),
                ..Default::default()
            });
            job
        },
        failed_browser_download_job("job_swap"),
    ]);
    queue_view.apply_snapshot_to_main_window(&ui, &command_snapshot);
    ui.invoke_view_change_requested("all".into());
    ui.invoke_request_delete_job("job_seed".into());
    assert!(ui.get_delete_prompt_visible());
    assert!(ui.get_delete_prompt_delete_from_disk());
    assert_eq!(ui.get_delete_prompt_title().as_str(), "Delete Download");
    assert_eq!(ui.get_delete_prompt_jobs().row_count(), 1);
    assert_eq!(
        ui.get_delete_prompt_jobs()
            .row_data(0)
            .expect("delete prompt job should be present")
            .filename
            .as_str(),
        "Seeded Torrent"
    );
    ui.invoke_delete_cancelled();
    assert!(!ui.get_delete_prompt_visible());

    ui.invoke_job_selection_toggled("job_delete".into(), true);
    ui.invoke_job_selection_toggled("job_extra".into(), true);
    ui.invoke_job_selection_toggled("job_active".into(), true);
    ui.invoke_request_delete_selected();
    assert!(ui.get_delete_prompt_visible());
    assert_eq!(ui.get_delete_prompt_jobs().row_count(), 2);
    assert!(!ui.get_delete_prompt_delete_from_disk());
    ui.invoke_delete_from_disk_changed(true);
    assert!(ui.get_delete_prompt_delete_from_disk());
    ui.invoke_delete_confirmed();
    runtime.block_on(async {
        for _ in 0..20 {
            if sink.commands().len() == 12 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    assert!(!ui.get_delete_prompt_visible());
    assert_eq!(ui.get_queue_selected_count(), 0);
    assert!(sink.commands().contains(&QueueCommand::DeleteMany {
        ids: vec!["job_delete".into(), "job_extra".into()],
        delete_from_disk: true,
    }));

    ui.invoke_request_rename_job("job_delete".into());
    assert!(ui.get_rename_prompt_visible());
    assert_eq!(ui.get_rename_base_name().as_str(), "file-job_delete");
    assert_eq!(ui.get_rename_extension().as_str(), "bin");
    ui.invoke_rename_base_name_changed("renamed".into());
    ui.invoke_rename_extension_changed(".zip".into());
    assert_eq!(ui.get_rename_preview_filename().as_str(), "renamed.zip");
    ui.invoke_rename_confirmed();
    runtime.block_on(async {
        for _ in 0..20 {
            if sink.commands().len() == 13 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    assert!(!ui.get_rename_prompt_visible());
    assert!(sink.commands().contains(&QueueCommand::Rename {
        id: "job_delete".into(),
        filename: "renamed.zip".into(),
    }));

    ui.invoke_request_rename_job("job_delete".into());
    ui.invoke_rename_base_name_changed("   ".into());
    assert!(!ui.get_rename_can_confirm());
    ui.invoke_rename_confirmed();
    runtime.block_on(async {
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(5)).await;
    });
    assert_eq!(
        sink.commands()
            .iter()
            .filter(|command| matches!(command, QueueCommand::Rename { .. }))
            .count(),
        1
    );
    ui.invoke_rename_cancelled();
    assert!(!ui.get_rename_prompt_visible());

    sink.set_external_use_result(ExternalUseResult {
        paused_torrent: true,
        auto_reseed_retry_seconds: Some(60),
    });
    ui.invoke_open_job_file_requested("job_delete".into());
    ui.invoke_reveal_job_requested("job_delete".into());
    ui.invoke_swap_failed_to_browser_requested("job_swap".into());
    runtime.block_on(async {
        for _ in 0..20 {
            if sink.commands().len() == 16 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    assert!(sink
        .commands()
        .contains(&QueueCommand::OpenFile("job_delete".into())));
    assert!(sink
        .commands()
        .contains(&QueueCommand::RevealInFolder("job_delete".into())));
    assert!(sink
        .commands()
        .contains(&QueueCommand::SwapFailedToBrowser("job_swap".into())));
    drain_slint_events();
    wait_for_toast_present(&toast_state, "Torrent Paused", "resume seeding every 60s");

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

    wire_update_callbacks(
        &ui,
        runtime.clone(),
        update_sink.clone(),
        update_store,
        toast_state.clone(),
    );
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
    wait_for_toast_present(&toast_state, "No Update Available", "latest alpha build");

    let lifecycle_sink = Arc::new(RecordingMainWindowLifecycleSink::default());
    wire_main_window_lifecycle_callbacks(&ui, lifecycle_sink.clone());
    ui.invoke_minimize_main_window_requested();
    ui.invoke_toggle_main_window_maximize_requested();
    ui.invoke_close_main_window_requested();
    ui.invoke_start_main_window_drag_requested();
    ui.invoke_titlebar_double_clicked();

    assert_eq!(
        lifecycle_sink.commands(),
        vec![
            MainWindowLifecycleCommand::Minimize,
            MainWindowLifecycleCommand::ToggleMaximize,
            MainWindowLifecycleCommand::CloseToTray,
            MainWindowLifecycleCommand::StartDrag,
            MainWindowLifecycleCommand::ToggleMaximize,
        ]
    );

    exercise_add_download_modal(
        &ui,
        runtime.clone(),
        queue_view.clone(),
        toast_state.clone(),
    );
    exercise_settings_view(
        &ui,
        runtime.clone(),
        queue_view.clone(),
        toast_state.clone(),
    );
    exercise_diagnostics_native_host_view(&ui, runtime.clone(), queue_view.clone(), toast_state);

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

fn exercise_settings_view(
    ui: &MainWindow,
    runtime: Arc<tokio::runtime::Runtime>,
    queue_view: QueueViewRuntimeState,
    toast_state: ToastRuntimeState,
) {
    let mut snapshot = test_snapshot(Vec::new());
    snapshot.settings.download_directory = "C:/Downloads".into();
    snapshot.settings.torrent.download_directory = "C:/Downloads/Torrent".into();
    snapshot.settings.extension_integration.excluded_hosts = vec!["web.telegram.org".into()];

    let settings_state = SettingsRuntimeState::default();
    settings_state.apply_snapshot_to_main_window(ui, &snapshot);
    let sink = Arc::new(RecordingSettingsCommandSink::default());
    wire_settings_callbacks(
        ui,
        runtime.clone(),
        sink.clone(),
        settings_state.clone(),
        queue_view,
        toast_state.clone(),
    );

    ui.invoke_settings_requested();
    assert!(ui.get_settings_view_visible());
    assert_eq!(
        ui.get_settings_download_directory().as_str(),
        "C:/Downloads"
    );
    assert_eq!(
        ui.get_settings_torrent_download_directory().as_str(),
        "C:/Downloads/Torrent"
    );
    assert_eq!(ui.get_settings_sections().row_count(), 6);
    assert_eq!(ui.get_settings_active_section().as_str(), "general");
    assert!(!ui.get_settings_dirty());

    ui.invoke_settings_theme_changed("dark".into());
    ui.invoke_settings_accent_color_changed("ABCDEF".into());
    assert!(ui.get_settings_dirty());
    assert_eq!(ui.get_settings_theme().as_str(), "dark");
    assert_eq!(ui.get_settings_accent_color().as_str(), "#abcdef");

    let mut refreshed = snapshot.clone();
    refreshed.settings.download_directory = "D:/Incoming".into();
    settings_state.apply_snapshot_to_main_window(ui, &refreshed);
    assert_eq!(
        ui.get_settings_download_directory().as_str(),
        "C:/Downloads",
        "dirty settings draft should not be overwritten by background snapshots"
    );
    assert_eq!(ui.get_settings_theme().as_str(), "dark");

    ui.invoke_settings_cancel_requested();
    assert!(ui.get_settings_unsaved_prompt_visible());
    ui.invoke_settings_unsaved_cancelled();
    assert!(!ui.get_settings_unsaved_prompt_visible());
    assert!(ui.get_settings_view_visible());
    ui.invoke_settings_cancel_requested();
    ui.invoke_settings_discard_confirmed();
    assert!(!ui.get_settings_view_visible());
    assert!(!ui.get_settings_dirty());

    ui.invoke_settings_requested();
    sink.set_browse_directory_result(Some("E:/Incoming".into()));
    ui.invoke_settings_browse_download_directory_requested();
    runtime.block_on(async {
        for _ in 0..20 {
            if ui.get_settings_download_directory().as_str() == "E:/Incoming" {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    assert_eq!(ui.get_settings_download_directory().as_str(), "E:/Incoming");
    assert_eq!(
        ui.get_settings_torrent_download_directory().as_str(),
        "E:/Incoming/Torrent"
    );

    ui.invoke_settings_max_concurrent_downloads_changed("8".into());
    ui.invoke_settings_torrent_seed_mode_changed("ratio_or_time".into());
    ui.invoke_settings_torrent_seed_ratio_limit_changed("1.75".into());
    ui.invoke_settings_torrent_peer_watchdog_mode_changed("experimental".into());
    ui.invoke_settings_extension_handoff_mode_changed("auto".into());
    ui.invoke_settings_extension_excluded_host_input_changed(
        "Example.com, web.telegram.org".into(),
    );
    ui.invoke_settings_extension_excluded_host_add_requested();
    assert_eq!(
        ui.get_settings_extension_excluded_hosts_summary().as_str(),
        "2 excluded sites"
    );

    ui.invoke_settings_clear_torrent_cache_requested();
    runtime.block_on(async {
        for _ in 0..20 {
            if sink.clear_torrent_cache_calls() == 1 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    assert_eq!(sink.clear_torrent_cache_calls(), 1);

    ui.invoke_settings_save_requested();
    runtime.block_on(async {
        for _ in 0..20 {
            if sink.saved_settings().len() == 1 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    let saved_settings = sink.saved_settings();
    assert_eq!(saved_settings.len(), 1);
    assert_eq!(saved_settings[0].download_directory, "E:/Incoming");
    assert_eq!(saved_settings[0].max_concurrent_downloads, 8);
    assert_eq!(
        saved_settings[0].torrent.download_directory,
        "E:/Incoming/Torrent"
    );
    assert_eq!(
        saved_settings[0].torrent.seed_mode,
        TorrentSeedMode::RatioOrTime
    );
    assert_eq!(
        saved_settings[0].torrent.peer_connection_watchdog_mode,
        TorrentPeerConnectionWatchdogMode::Experimental
    );
    assert_eq!(
        saved_settings[0]
            .extension_integration
            .download_handoff_mode,
        DownloadHandoffMode::Auto
    );
    assert!(saved_settings[0]
        .extension_integration
        .excluded_hosts
        .contains(&"example.com".into()));
    assert!(!ui.get_settings_view_visible());
    assert!(!ui.get_settings_dirty());
    wait_for_toast_present(&toast_state, "Settings Saved", "Preferences updated");
}

fn exercise_diagnostics_native_host_view(
    ui: &MainWindow,
    runtime: Arc<tokio::runtime::Runtime>,
    queue_view: QueueViewRuntimeState,
    toast_state: ToastRuntimeState,
) {
    let settings_state = SettingsRuntimeState::default();
    let settings_sink = Arc::new(RecordingSettingsCommandSink::default());
    wire_settings_callbacks(
        ui,
        runtime.clone(),
        settings_sink,
        settings_state.clone(),
        queue_view,
        toast_state.clone(),
    );

    let diagnostics_state = DiagnosticsRuntimeState::default();
    let diagnostics_sink = Arc::new(RecordingDiagnosticsCommandSink::default());
    diagnostics_sink.push_diagnostics_result(diagnostics_snapshot(
        HostRegistrationStatus::Configured,
        "Chrome",
        "Initial event",
    ));
    diagnostics_sink.push_export_result(Some("C:/Temp/sdm-diagnostics.txt".into()));
    wire_diagnostics_callbacks(
        ui,
        runtime.clone(),
        diagnostics_sink.clone(),
        settings_state,
        diagnostics_state,
        toast_state.clone(),
    );

    ui.invoke_diagnostics_copy_requested();
    assert_eq!(
        ui.get_diagnostics_error_text().as_str(),
        "Refresh diagnostics before copying the report."
    );

    ui.invoke_settings_requested();
    ui.invoke_settings_section_requested("native_host".into());
    runtime.block_on(async {
        for _ in 0..30 {
            if diagnostics_sink.get_diagnostics_calls() == 1 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();

    assert_eq!(ui.get_settings_active_section().as_str(), "native_host");
    assert_eq!(ui.get_diagnostics_status_label().as_str(), "Ready");
    assert_eq!(
        ui.get_diagnostics_status_message().as_str(),
        "At least one browser has a valid native host registration and host binary path."
    );
    assert_eq!(ui.get_diagnostics_host_entries().row_count(), 1);
    assert_eq!(
        ui.get_diagnostics_host_entries()
            .row_data(0)
            .expect("host registration entry should render")
            .browser
            .as_str(),
        "Chrome"
    );
    assert_eq!(ui.get_diagnostics_recent_events().row_count(), 1);
    assert_eq!(
        ui.get_diagnostics_recent_events()
            .row_data(0)
            .expect("diagnostic event should render")
            .message
            .as_str(),
        "Initial event"
    );

    ui.invoke_diagnostics_copy_requested();
    runtime.block_on(async {
        for _ in 0..30 {
            if diagnostics_sink.copied_reports().len() == 1 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    assert!(diagnostics_sink.copied_reports()[0].contains("Simple Download Manager Diagnostics"));
    assert_eq!(
        ui.get_diagnostics_action_status_text().as_str(),
        "The diagnostics report was copied to the clipboard."
    );
    wait_for_toast_present(&toast_state, "Diagnostics Copied", "clipboard");

    ui.invoke_diagnostics_export_requested();
    runtime.block_on(async {
        for _ in 0..30 {
            if diagnostics_sink.export_calls() == 1 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    assert_eq!(
        ui.get_diagnostics_action_status_text().as_str(),
        "Saved diagnostics to C:/Temp/sdm-diagnostics.txt."
    );

    diagnostics_sink.push_diagnostics_result(diagnostics_snapshot(
        HostRegistrationStatus::Broken,
        "Firefox",
        "Repair event",
    ));
    ui.invoke_diagnostics_repair_host_requested();
    runtime.block_on(async {
        for _ in 0..30 {
            if diagnostics_sink.repair_calls() == 1 && diagnostics_sink.get_diagnostics_calls() == 2
            {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    assert_eq!(diagnostics_sink.repair_calls(), 1);
    assert_eq!(ui.get_diagnostics_status_label().as_str(), "Repair");
    assert_eq!(
        ui.get_diagnostics_action_status_text().as_str(),
        "Native host registration was refreshed."
    );

    ui.invoke_diagnostics_open_install_docs_requested();
    runtime.block_on(async {
        for _ in 0..30 {
            if diagnostics_sink.open_docs_calls() == 1 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    assert_eq!(diagnostics_sink.open_docs_calls(), 1);
    assert_eq!(
        ui.get_diagnostics_action_status_text().as_str(),
        "Opened native host installation docs."
    );

    ui.invoke_diagnostics_test_handoff_requested();
    runtime.block_on(async {
        for _ in 0..30 {
            if diagnostics_sink.test_handoff_calls() == 1 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    assert_eq!(diagnostics_sink.test_handoff_calls(), 1);
    assert_eq!(
        ui.get_diagnostics_action_status_text().as_str(),
        "A browser-style download prompt was opened."
    );
    wait_for_toast_present(&toast_state, "Test Prompt Opened", "browser-style");
}

fn exercise_add_download_modal(
    ui: &MainWindow,
    runtime: Arc<tokio::runtime::Runtime>,
    queue_view: QueueViewRuntimeState,
    toast_state: ToastRuntimeState,
) {
    let snapshot = test_snapshot(vec![
        download_job("job_file", JobState::Queued),
        {
            let mut job = download_job("job_torrent", JobState::Queued);
            job.transfer_kind = TransferKind::Torrent;
            job
        },
        download_job("job_bulk_a", JobState::Queued),
        download_job("job_bulk_b", JobState::Queued),
    ]);
    queue_view.apply_snapshot_to_main_window(ui, &snapshot);

    let sink = Arc::new(RecordingAddDownloadCommandSink::default());
    wire_add_download_callbacks(
        ui,
        runtime.clone(),
        sink.clone(),
        queue_view.clone(),
        AddDownloadRuntimeState::default(),
        toast_state.clone(),
    );

    ui.invoke_add_download_requested();
    assert!(ui.get_add_download_visible());
    assert_eq!(ui.get_add_download_mode().as_str(), "single");
    assert_eq!(
        ui.get_add_download_submit_label().as_str(),
        "Start Download"
    );
    ui.invoke_add_download_cancelled();
    assert!(!ui.get_add_download_visible());

    ui.invoke_add_download_requested();
    ui.invoke_add_download_single_url_changed("https://example.com/file.zip".into());
    ui.invoke_add_download_single_sha256_changed("abc123".into());
    ui.invoke_add_download_submit_requested();
    assert_eq!(
        ui.get_add_download_error_text().as_str(),
        "SHA-256 checksum must be 64 hexadecimal characters."
    );
    assert!(sink.add_job_requests().is_empty());

    sink.set_add_job_result(AddJobResult {
        job_id: "job_file".into(),
        filename: "file.zip".into(),
        status: AddJobStatus::Queued,
    });
    ui.invoke_add_download_single_sha256_changed(
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
    );
    ui.invoke_add_download_submit_requested();
    runtime.block_on(async {
        for _ in 0..20 {
            if sink.add_job_requests().len() == 1 && sink.open_progress_ids().len() == 1 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    assert!(!ui.get_add_download_visible());
    assert_eq!(ui.get_queue_view_id().as_str(), "all");
    assert_eq!(ui.get_queue_selected_count(), 1);
    wait_for_toast_present(&toast_state, "Download Added", "added to the queue");
    assert_eq!(
        sink.add_job_requests()[0],
        AddJobRequest {
            url: "https://example.com/file.zip".into(),
            directory_override: None,
            filename_hint: None,
            expected_sha256: Some(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()
            ),
            transfer_kind: Some(TransferKind::Http),
        }
    );
    assert_eq!(sink.open_progress_ids(), vec!["job_file".to_string()]);

    ui.invoke_add_download_requested();
    ui.invoke_add_download_mode_changed("torrent".into());
    sink.set_browse_torrent_file_result(Some("C:/Downloads/example.torrent".into()));
    ui.invoke_add_download_import_torrent_requested();
    runtime.block_on(async {
        for _ in 0..20 {
            if ui.get_add_download_torrent_url().as_str() == "C:/Downloads/example.torrent" {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    sink.set_add_job_result(AddJobResult {
        job_id: "job_torrent".into(),
        filename: "example.torrent".into(),
        status: AddJobStatus::Queued,
    });
    ui.invoke_add_download_submit_requested();
    runtime.block_on(async {
        for _ in 0..20 {
            if sink.add_job_requests().len() == 2 && sink.open_progress_ids().len() == 2 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    assert_eq!(ui.get_queue_view_id().as_str(), "torrents");
    assert_eq!(
        sink.add_job_requests()[1].url,
        "C:/Downloads/example.torrent"
    );
    assert_eq!(
        sink.add_job_requests()[1].transfer_kind,
        Some(TransferKind::Torrent)
    );

    ui.invoke_add_download_requested();
    ui.invoke_add_download_mode_changed("bulk".into());
    ui.invoke_add_download_bulk_urls_changed(
        "https://example.com/a.bin\nhttps://example.com/b.bin".into(),
    );
    ui.invoke_add_download_archive_name_changed("bundle".into());
    assert_eq!(ui.get_add_download_archive_name().as_str(), "bundle.zip");
    sink.set_add_jobs_result(AddJobsResult {
        results: vec![
            AddJobResult {
                job_id: "job_bulk_a".into(),
                filename: "a.bin".into(),
                status: AddJobStatus::Queued,
            },
            AddJobResult {
                job_id: "job_bulk_b".into(),
                filename: "b.bin".into(),
                status: AddJobStatus::Queued,
            },
        ],
        queued_count: 2,
        duplicate_count: 0,
    });
    ui.invoke_add_download_submit_requested();
    runtime.block_on(async {
        for _ in 0..20 {
            if sink.add_jobs_requests().len() == 1 && sink.open_batch_contexts().len() == 1 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    drain_slint_events();
    assert!(!ui.get_add_download_visible());
    assert_eq!(
        sink.add_jobs_requests()[0],
        AddJobsRequest {
            urls: vec![
                "https://example.com/a.bin".into(),
                "https://example.com/b.bin".into()
            ],
            bulk_archive_name: Some("bundle.zip".into()),
        }
    );
    let batch_context = &sink.open_batch_contexts()[0];
    assert_eq!(batch_context.kind, ProgressBatchKind::Bulk);
    assert_eq!(batch_context.title, "Bulk download progress");
    assert_eq!(batch_context.archive_name.as_deref(), Some("bundle.zip"));
    assert_eq!(
        batch_context.job_ids,
        vec!["job_bulk_a".to_string(), "job_bulk_b".to_string()]
    );
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
async fn slint_shell_routes_shell_errors_to_toast_actions() {
    let dispatcher = RecordingUiDispatcher::default();
    let shell = SlintShellServices::new(dispatcher.clone());

    shell
        .emit_event(DesktopEvent::ShellError(ShellError {
            operation: "reveal path".into(),
            message: "Access is denied.".into(),
        }))
        .await
        .expect("shell error should dispatch toast");

    assert!(dispatcher.actions().iter().any(|action| {
        matches!(
            action,
            UiAction::ShowToast(toast)
                if toast.toast_type.id() == "error"
                    && toast.title == "Shell Error"
                    && toast.message == "reveal path failed: Access is denied."
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

#[test]
fn prompt_window_action_bridge_dispatches_backend_commands() {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime should build"),
    );
    let sink = Arc::new(RecordingPromptWindowCommandSink::default());
    sink.set_browse_directory_result(Some("D:/Incoming".into()));

    wire_prompt_window_action_bridge(runtime.clone(), sink.clone());
    popups::dispatch_prompt_window_action(popups::PromptWindowAction::BrowseDirectory)
        .expect("browse prompt action should dispatch");
    popups::dispatch_prompt_window_action(popups::PromptWindowAction::Confirm(
        ConfirmPromptRequest {
            id: "prompt_confirm".into(),
            directory_override: Some("D:/Incoming".into()),
            duplicate_action: PromptDuplicateAction::Overwrite,
            renamed_filename: None,
        },
    ))
    .expect("confirm prompt action should dispatch");
    popups::dispatch_prompt_window_action(popups::PromptWindowAction::Cancel(
        "prompt_cancel".into(),
    ))
    .expect("cancel prompt action should dispatch");
    popups::dispatch_prompt_window_action(popups::PromptWindowAction::Swap("prompt_swap".into()))
        .expect("swap prompt action should dispatch");

    runtime.block_on(async {
        for _ in 0..40 {
            if sink.browse_directory_calls() == 1
                && sink.confirm_requests().len() == 1
                && sink.cancel_ids().len() == 1
                && sink.swap_ids().len() == 1
            {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    popups::clear_prompt_action_dispatcher_for_tests();

    assert_eq!(sink.browse_directory_calls(), 1);
    assert_eq!(
        sink.confirm_requests(),
        vec![ConfirmPromptRequest {
            id: "prompt_confirm".into(),
            directory_override: Some("D:/Incoming".into()),
            duplicate_action: PromptDuplicateAction::Overwrite,
            renamed_filename: None,
        }]
    );
    assert_eq!(sink.cancel_ids(), vec!["prompt_cancel".to_string()]);
    assert_eq!(sink.swap_ids(), vec!["prompt_swap".to_string()]);
}

#[test]
fn progress_popup_action_bridge_dispatches_backend_commands() {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime should build"),
    );
    let sink = Arc::new(RecordingProgressPopupCommandSink::default());

    wire_progress_popup_action_bridge(runtime.clone(), sink.clone());
    for action in [
        popups::ProgressPopupAction::Pause("job_pause".into()),
        popups::ProgressPopupAction::Resume("job_resume".into()),
        popups::ProgressPopupAction::Retry("job_retry".into()),
        popups::ProgressPopupAction::Cancel("job_cancel".into()),
        popups::ProgressPopupAction::OpenFile("job_open".into()),
        popups::ProgressPopupAction::RevealInFolder("job_reveal".into()),
        popups::ProgressPopupAction::SwapFailedToBrowser("job_swap".into()),
        popups::ProgressPopupAction::BatchPause(vec!["job_a".into(), "job_b".into()]),
        popups::ProgressPopupAction::BatchCancel(vec!["job_c".into()]),
    ] {
        popups::dispatch_progress_popup_action(action)
            .expect("progress popup action should dispatch");
    }

    runtime.block_on(async {
        for _ in 0..40 {
            if sink.actions().len() == 9 {
                break;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    popups::clear_progress_popup_action_dispatcher_for_tests();

    assert_eq!(
        sink.actions(),
        vec![
            popups::ProgressPopupAction::Pause("job_pause".into()),
            popups::ProgressPopupAction::Resume("job_resume".into()),
            popups::ProgressPopupAction::Retry("job_retry".into()),
            popups::ProgressPopupAction::Cancel("job_cancel".into()),
            popups::ProgressPopupAction::OpenFile("job_open".into()),
            popups::ProgressPopupAction::RevealInFolder("job_reveal".into()),
            popups::ProgressPopupAction::SwapFailedToBrowser("job_swap".into()),
            popups::ProgressPopupAction::BatchPause(vec!["job_a".into(), "job_b".into()]),
            popups::ProgressPopupAction::BatchCancel(vec!["job_c".into()]),
        ]
    );
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
        runtime_source.contains("main_window::current_startup_window_action")
            && !runtime_source.contains("ui.show().map_err(|error| error.to_string())?"),
        "run_app should use Slint startup visibility policy instead of unconditionally showing the main window"
    );
    assert!(
        runtime_source.contains("wire_main_window_lifecycle_callbacks"),
        "runtime should wire frameless titlebar callbacks through shell lifecycle helpers"
    );
    assert!(
        main_window_source.contains("on_close_requested")
            && main_window_source.contains("save_main_window_state_sync"),
        "main-window close handling should persist geometry through SharedState"
    );
    assert!(
        main_window_source.contains("WinitWindowAccessor")
            && main_window_source.contains("drag_window()")
            && main_window_source.contains("focus_window()"),
        "native focus and titlebar drag should be isolated in the Slint shell main-window module"
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
fn slint_main_window_source_has_frameless_titlebar_controls() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ui_source = std::fs::read_to_string(manifest_dir.join("ui/main.slint"))
        .expect("main Slint source should load");

    for expected in [
        "no-frame: true",
        "start-main-window-drag-requested",
        "PointerEventKind.down",
        "PointerEventButton.left",
        "titlebar-double-clicked",
        "minimize-main-window-requested",
        "toggle-main-window-maximize-requested",
        "close-main-window-requested",
        "Download Manager",
    ] {
        assert!(
            ui_source.contains(expected),
            "MainWindow should expose frameless titlebar contract: {expected}"
        );
    }
}

#[test]
fn slint_main_window_source_exposes_queue_navigation_callbacks() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ui_source = std::fs::read_to_string(manifest_dir.join("ui/main.slint"))
        .expect("main Slint source should load");

    for expected in [
        "export struct QueueNavItem",
        "view-change-requested",
        "search-query-changed",
        "sort-column-requested",
        "job-selection-requested",
        "select-all-visible-requested",
        "clear-selection-requested",
        "request-delete-job",
        "request-delete-selected",
        "delete-from-disk-changed",
        "delete-confirmed",
        "delete-cancelled",
        "request-rename-job",
        "rename-base-name-changed",
        "rename-extension-changed",
        "rename-confirmed",
        "rename-cancelled",
        "open-job-file-requested",
        "reveal-job-requested",
        "swap-failed-to-browser-requested",
        "pause-all-requested",
        "resume-all-requested",
        "retry-failed-requested",
        "clear-completed-requested",
        "retry-job-requested",
        "restart-job-requested",
        "add-download-mode-changed",
        "add-download-single-url-changed",
        "add-download-single-sha256-changed",
        "add-download-torrent-url-changed",
        "add-download-multi-urls-changed",
        "add-download-bulk-urls-changed",
        "add-download-archive-name-changed",
        "add-download-combine-bulk-changed",
        "add-download-import-torrent-requested",
        "add-download-submit-requested",
        "add-download-cancelled",
    ] {
        assert!(
            ui_source.contains(expected),
            "Slint queue/add-download UI should expose {expected}"
        );
    }
    assert!(
        !ui_source.contains("Remove selected") && !ui_source.contains("remove-selected-requested"),
        "Phase 4B should route deletion through a confirmation prompt instead of direct removal"
    );
}

#[test]
fn slint_runtime_source_replaces_add_download_placeholder() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let runtime_source = std::fs::read_to_string(manifest_dir.join("src/runtime.rs"))
        .expect("runtime source should load");

    assert!(
        runtime_source.contains("wire_add_download_callbacks"),
        "runtime should wire the Slint add-download modal"
    );
    assert!(
        !runtime_source.contains("add-download UI is not implemented"),
        "Phase 4C should remove the add-download placeholder log"
    );
}

#[test]
fn slint_main_window_source_exposes_settings_workflow_callbacks() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ui_source = std::fs::read_to_string(manifest_dir.join("ui/main.slint"))
        .expect("main Slint source should load");
    let runtime_source = std::fs::read_to_string(manifest_dir.join("src/runtime.rs"))
        .expect("runtime source should load");

    for expected in [
        "export struct SettingsNavItem",
        "settings-requested",
        "settings-section-requested",
        "settings-save-requested",
        "settings-cancel-requested",
        "settings-discard-confirmed",
        "settings-unsaved-cancelled",
        "settings-browse-download-directory-requested",
        "settings-browse-torrent-directory-requested",
        "settings-clear-torrent-cache-requested",
        "settings-download-directory-changed",
        "settings-max-concurrent-downloads-changed",
        "settings-auto-retry-attempts-changed",
        "settings-speed-limit-kib-per-second-changed",
        "settings-download-performance-mode-changed",
        "settings-notifications-enabled-changed",
        "settings-show-details-on-click-changed",
        "settings-queue-row-size-changed",
        "settings-start-on-startup-changed",
        "settings-startup-launch-mode-changed",
        "settings-theme-changed",
        "settings-accent-color-changed",
        "settings-torrent-enabled-changed",
        "settings-torrent-download-directory-changed",
        "settings-torrent-seed-mode-changed",
        "settings-torrent-seed-ratio-limit-changed",
        "settings-torrent-seed-time-limit-minutes-changed",
        "settings-torrent-upload-limit-kib-per-second-changed",
        "settings-torrent-port-forwarding-enabled-changed",
        "settings-torrent-port-forwarding-port-changed",
        "settings-torrent-peer-watchdog-mode-changed",
        "settings-extension-enabled-changed",
        "settings-extension-handoff-mode-changed",
        "settings-extension-listen-port-changed",
        "settings-extension-context-menu-enabled-changed",
        "settings-extension-show-progress-after-handoff-changed",
        "settings-extension-show-badge-status-changed",
        "settings-extension-authenticated-handoff-enabled-changed",
        "settings-extension-excluded-host-input-changed",
        "settings-extension-excluded-host-add-requested",
        "settings-extension-excluded-host-remove-requested",
        "native_host",
        "diagnostics-refresh-requested",
        "diagnostics-copy-requested",
        "diagnostics-export-requested",
        "diagnostics-open-install-docs-requested",
        "diagnostics-repair-host-requested",
        "diagnostics-test-handoff-requested",
    ] {
        assert!(
            ui_source.contains(expected),
            "Slint settings/native-host UI should expose {expected}"
        );
    }

    assert!(
        runtime_source.contains("wire_settings_callbacks"),
        "runtime should wire settings callbacks through the controller state"
    );
    assert!(
        runtime_source.contains("wire_diagnostics_callbacks")
            && runtime_source.contains("DiagnosticsCommandSink"),
        "Phase 4E should wire diagnostics/native-host callbacks through runtime command sinks"
    );
}

#[test]
fn slint_prompt_window_source_exposes_duplicate_prompt_callbacks() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ui_source = std::fs::read_to_string(manifest_dir.join("ui/main.slint"))
        .expect("main Slint source should load");
    let runtime_source = std::fs::read_to_string(manifest_dir.join("src/runtime.rs"))
        .expect("runtime source should load");
    let popup_source = std::fs::read_to_string(manifest_dir.join("src/shell/popups.rs"))
        .expect("popup source should load");

    for expected in [
        "change-directory-requested",
        "cancel-requested",
        "download-requested",
        "swap-requested",
        "duplicate-menu-toggled",
        "duplicate-action-requested",
        "duplicate-rename-started",
        "duplicate-renamed-filename-changed",
        "duplicate-rename-confirmed",
        "duplicate-rename-cancelled",
        "Choose Action",
        "Overwrite",
        "Rename",
        "Download Anyway",
    ] {
        assert!(
            ui_source.contains(expected),
            "Slint prompt UI should expose {expected}"
        );
    }

    assert!(
        !ui_source.contains("Show Existing"),
        "Slint compact prompt should not reintroduce the removed Show Existing action"
    );
    assert!(
        runtime_source.contains("wire_prompt_window_action_bridge")
            && runtime_source.contains("PromptWindowCommandSink"),
        "runtime should wire prompt popup actions through a command sink"
    );
    assert!(
        popup_source.contains("PromptWindowAction")
            && popup_source.contains("install_prompt_action_dispatcher")
            && popup_source.contains("prompt_confirm_request"),
        "prompt action handling should stay isolated in shell::popups"
    );
}

#[test]
fn slint_progress_window_source_exposes_progress_actions_and_metrics() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ui_source = std::fs::read_to_string(manifest_dir.join("ui/main.slint"))
        .expect("main Slint source should load");
    let runtime_source = std::fs::read_to_string(manifest_dir.join("src/runtime.rs"))
        .expect("runtime source should load");
    let popup_source = std::fs::read_to_string(manifest_dir.join("src/shell/popups.rs"))
        .expect("popup source should load");

    for expected in [
        "progress-pause-requested",
        "progress-resume-requested",
        "progress-retry-requested",
        "progress-cancel-requested",
        "progress-open-requested",
        "progress-reveal-requested",
        "progress-swap-requested",
        "batch-pause-requested",
        "batch-resume-requested",
        "batch-cancel-requested",
        "batch-reveal-completed-requested",
        "Speed",
        "ETA",
        "Size",
        "Down",
        "Up",
        "Peers",
        "Seeds",
        "Ratio",
        "Pause all",
        "Resume all",
        "Cancel active",
        "Reveal completed",
    ] {
        assert!(
            ui_source.contains(expected),
            "Slint progress UI should expose {expected}"
        );
    }

    assert!(
        runtime_source.contains("wire_progress_popup_action_bridge")
            && runtime_source.contains("ProgressPopupCommandSink"),
        "runtime should wire progress popup actions through a command sink"
    );
    assert!(
        popup_source.contains("ProgressPopupAction")
            && popup_source.contains("install_progress_popup_action_dispatcher")
            && popup_source.contains("progress_details_from_job_with_state"),
        "progress popup action handling should stay isolated in shell::popups"
    );
}

#[test]
fn slint_toast_source_exposes_toast_area_and_shell_error_wiring() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ui_source =
        std::fs::read_to_string(manifest_dir.join("ui/main.slint")).expect("Slint UI should load");
    let runtime_source = std::fs::read_to_string(manifest_dir.join("src/runtime.rs"))
        .expect("runtime source should load");

    for expected in [
        "export struct ToastMessage",
        "in property <[ToastMessage]> toasts",
        "callback toast-dismiss-requested(string)",
        "ToastArea",
    ] {
        assert!(
            ui_source.contains(expected),
            "MainWindow should expose toast UI contract: {expected}"
        );
    }
    assert!(
        runtime_source.contains("ToastRuntimeState")
            && runtime_source.contains("UiAction::ShowToast")
            && runtime_source.contains("toast_for_shell_error"),
        "runtime should route shell/backend feedback through toast state"
    );
    assert!(
        !runtime_source.contains("eprintln!(\"shell error during"),
        "ShellError should no longer be handled only by logging"
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
    next_error: Mutex<Option<String>>,
    external_use_result: Mutex<Option<ExternalUseResult>>,
}

impl RecordingQueueCommandSink {
    fn commands(&self) -> Vec<QueueCommand> {
        self.commands.lock().unwrap().clone()
    }

    fn set_next_error(&self, error: &str) {
        *self.next_error.lock().unwrap() = Some(error.into());
    }

    fn set_external_use_result(&self, result: ExternalUseResult) {
        *self.external_use_result.lock().unwrap() = Some(result);
    }
}

impl QueueCommandSink for RecordingQueueCommandSink {
    fn run_queue_command(&self, command: QueueCommand) -> BackendFuture<'_, QueueCommandOutput> {
        Box::pin(async move {
            self.commands.lock().unwrap().push(command.clone());
            if let Some(error) = self.next_error.lock().unwrap().take() {
                return Err(error);
            }
            let external_use = match command {
                QueueCommand::OpenFile(_) | QueueCommand::RevealInFolder(_) => {
                    self.external_use_result.lock().unwrap().clone()
                }
                _ => None,
            };
            Ok(QueueCommandOutput { external_use })
        })
    }
}

#[derive(Default)]
struct RecordingAddDownloadCommandSink {
    add_job_requests: Mutex<Vec<AddJobRequest>>,
    add_jobs_requests: Mutex<Vec<AddJobsRequest>>,
    browse_torrent_file_result: Mutex<Option<String>>,
    add_job_result: Mutex<Option<AddJobResult>>,
    add_jobs_result: Mutex<Option<AddJobsResult>>,
    open_progress_ids: Mutex<Vec<String>>,
    open_batch_contexts: Mutex<Vec<ProgressBatchContext>>,
}

impl RecordingAddDownloadCommandSink {
    fn set_browse_torrent_file_result(&self, result: Option<String>) {
        *self.browse_torrent_file_result.lock().unwrap() = result;
    }

    fn set_add_job_result(&self, result: AddJobResult) {
        *self.add_job_result.lock().unwrap() = Some(result);
    }

    fn set_add_jobs_result(&self, result: AddJobsResult) {
        *self.add_jobs_result.lock().unwrap() = Some(result);
    }

    fn add_job_requests(&self) -> Vec<AddJobRequest> {
        self.add_job_requests.lock().unwrap().clone()
    }

    fn add_jobs_requests(&self) -> Vec<AddJobsRequest> {
        self.add_jobs_requests.lock().unwrap().clone()
    }

    fn open_progress_ids(&self) -> Vec<String> {
        self.open_progress_ids.lock().unwrap().clone()
    }

    fn open_batch_contexts(&self) -> Vec<ProgressBatchContext> {
        self.open_batch_contexts.lock().unwrap().clone()
    }
}

impl AddDownloadCommandSink for RecordingAddDownloadCommandSink {
    fn add_download_job(&self, request: AddJobRequest) -> BackendFuture<'_, AddJobResult> {
        Box::pin(async move {
            self.add_job_requests.lock().unwrap().push(request);
            self.add_job_result
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| "missing add job result".into())
        })
    }

    fn add_download_jobs(&self, request: AddJobsRequest) -> BackendFuture<'_, AddJobsResult> {
        Box::pin(async move {
            self.add_jobs_requests.lock().unwrap().push(request);
            self.add_jobs_result
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| "missing add jobs result".into())
        })
    }

    fn browse_torrent_file_for_add_download(&self) -> BackendFuture<'_, Option<String>> {
        Box::pin(async move { Ok(self.browse_torrent_file_result.lock().unwrap().clone()) })
    }

    fn open_add_download_progress_window(&self, id: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.open_progress_ids.lock().unwrap().push(id);
            Ok(())
        })
    }

    fn open_add_download_batch_progress_window(
        &self,
        context: ProgressBatchContext,
    ) -> BackendFuture<'_, String> {
        Box::pin(async move {
            let batch_id = context.batch_id.clone();
            self.open_batch_contexts.lock().unwrap().push(context);
            Ok(batch_id)
        })
    }
}

#[derive(Default)]
struct RecordingSettingsCommandSink {
    saved_settings: Mutex<Vec<Settings>>,
    browse_directory_result: Mutex<Option<String>>,
    clear_torrent_cache_calls: Mutex<usize>,
}

impl RecordingSettingsCommandSink {
    fn set_browse_directory_result(&self, result: Option<String>) {
        *self.browse_directory_result.lock().unwrap() = result;
    }

    fn saved_settings(&self) -> Vec<Settings> {
        self.saved_settings.lock().unwrap().clone()
    }

    fn clear_torrent_cache_calls(&self) -> usize {
        *self.clear_torrent_cache_calls.lock().unwrap()
    }
}

impl SettingsCommandSink for RecordingSettingsCommandSink {
    fn save_settings_from_slint(&self, settings: Settings) -> BackendFuture<'_, Settings> {
        Box::pin(async move {
            self.saved_settings.lock().unwrap().push(settings.clone());
            Ok(settings)
        })
    }

    fn browse_settings_directory(&self) -> BackendFuture<'_, Option<String>> {
        Box::pin(async move { Ok(self.browse_directory_result.lock().unwrap().clone()) })
    }

    fn clear_settings_torrent_session_cache(
        &self,
    ) -> BackendFuture<'_, TorrentSessionCacheClearResult> {
        Box::pin(async move {
            *self.clear_torrent_cache_calls.lock().unwrap() += 1;
            Ok(TorrentSessionCacheClearResult {
                cleared: true,
                pending_restart: false,
                session_path: "E:/Incoming/Torrent/.sdm-session".into(),
            })
        })
    }
}

#[derive(Default)]
struct RecordingDiagnosticsCommandSink {
    diagnostics_results: Mutex<Vec<DiagnosticsSnapshot>>,
    export_results: Mutex<Vec<Option<String>>>,
    copied_reports: Mutex<Vec<String>>,
    get_diagnostics_calls: Mutex<usize>,
    export_calls: Mutex<usize>,
    open_docs_calls: Mutex<usize>,
    repair_calls: Mutex<usize>,
    test_handoff_calls: Mutex<usize>,
}

impl RecordingDiagnosticsCommandSink {
    fn push_diagnostics_result(&self, snapshot: DiagnosticsSnapshot) {
        self.diagnostics_results.lock().unwrap().push(snapshot);
    }

    fn push_export_result(&self, result: Option<String>) {
        self.export_results.lock().unwrap().push(result);
    }

    fn copied_reports(&self) -> Vec<String> {
        self.copied_reports.lock().unwrap().clone()
    }

    fn get_diagnostics_calls(&self) -> usize {
        *self.get_diagnostics_calls.lock().unwrap()
    }

    fn export_calls(&self) -> usize {
        *self.export_calls.lock().unwrap()
    }

    fn open_docs_calls(&self) -> usize {
        *self.open_docs_calls.lock().unwrap()
    }

    fn repair_calls(&self) -> usize {
        *self.repair_calls.lock().unwrap()
    }

    fn test_handoff_calls(&self) -> usize {
        *self.test_handoff_calls.lock().unwrap()
    }
}

impl DiagnosticsCommandSink for RecordingDiagnosticsCommandSink {
    fn get_diagnostics_for_slint(&self) -> BackendFuture<'_, DiagnosticsSnapshot> {
        Box::pin(async move {
            *self.get_diagnostics_calls.lock().unwrap() += 1;
            let mut diagnostics = self.diagnostics_results.lock().unwrap();
            if diagnostics.is_empty() {
                Err("missing diagnostics result".into())
            } else {
                Ok(diagnostics.remove(0))
            }
        })
    }

    fn export_diagnostics_report_from_slint(&self) -> BackendFuture<'_, Option<String>> {
        Box::pin(async move {
            *self.export_calls.lock().unwrap() += 1;
            let mut results = self.export_results.lock().unwrap();
            if results.is_empty() {
                Ok(None)
            } else {
                Ok(results.remove(0))
            }
        })
    }

    fn copy_diagnostics_report_from_slint(&self, report: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.copied_reports.lock().unwrap().push(report);
            Ok(())
        })
    }

    fn open_install_docs_from_slint(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            *self.open_docs_calls.lock().unwrap() += 1;
            Ok(())
        })
    }

    fn repair_host_registration_from_slint(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            *self.repair_calls.lock().unwrap() += 1;
            Ok(())
        })
    }

    fn test_extension_handoff_from_slint(&self) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            *self.test_handoff_calls.lock().unwrap() += 1;
            Ok(())
        })
    }
}

#[derive(Default)]
struct RecordingPromptWindowCommandSink {
    browse_directory_calls: Mutex<usize>,
    browse_directory_result: Mutex<Option<String>>,
    confirm_requests: Mutex<Vec<ConfirmPromptRequest>>,
    cancel_ids: Mutex<Vec<String>>,
    swap_ids: Mutex<Vec<String>>,
}

impl RecordingPromptWindowCommandSink {
    fn set_browse_directory_result(&self, result: Option<String>) {
        *self.browse_directory_result.lock().unwrap() = result;
    }

    fn browse_directory_calls(&self) -> usize {
        *self.browse_directory_calls.lock().unwrap()
    }

    fn confirm_requests(&self) -> Vec<ConfirmPromptRequest> {
        self.confirm_requests.lock().unwrap().clone()
    }

    fn cancel_ids(&self) -> Vec<String> {
        self.cancel_ids.lock().unwrap().clone()
    }

    fn swap_ids(&self) -> Vec<String> {
        self.swap_ids.lock().unwrap().clone()
    }
}

impl PromptWindowCommandSink for RecordingPromptWindowCommandSink {
    fn browse_prompt_directory(&self) -> BackendFuture<'_, Option<String>> {
        Box::pin(async move {
            *self.browse_directory_calls.lock().unwrap() += 1;
            Ok(self.browse_directory_result.lock().unwrap().clone())
        })
    }

    fn confirm_prompt_from_slint(&self, request: ConfirmPromptRequest) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.confirm_requests.lock().unwrap().push(request);
            Ok(())
        })
    }

    fn cancel_prompt_from_slint(&self, id: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.cancel_ids.lock().unwrap().push(id);
            Ok(())
        })
    }

    fn swap_prompt_from_slint(&self, id: String) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.swap_ids.lock().unwrap().push(id);
            Ok(())
        })
    }
}

#[derive(Default)]
struct RecordingProgressPopupCommandSink {
    actions: Mutex<Vec<popups::ProgressPopupAction>>,
}

impl RecordingProgressPopupCommandSink {
    fn actions(&self) -> Vec<popups::ProgressPopupAction> {
        self.actions.lock().unwrap().clone()
    }
}

impl ProgressPopupCommandSink for RecordingProgressPopupCommandSink {
    fn run_progress_popup_action(
        &self,
        action: popups::ProgressPopupAction,
    ) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.actions.lock().unwrap().push(action);
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

#[derive(Default)]
struct RecordingMainWindowLifecycleSink {
    commands: Mutex<Vec<MainWindowLifecycleCommand>>,
}

impl RecordingMainWindowLifecycleSink {
    fn commands(&self) -> Vec<MainWindowLifecycleCommand> {
        self.commands.lock().unwrap().clone()
    }
}

impl MainWindowLifecycleSink for RecordingMainWindowLifecycleSink {
    fn run_main_window_lifecycle_command(&self, command: MainWindowLifecycleCommand) {
        self.commands.lock().unwrap().push(command);
    }
}

fn test_snapshot(jobs: Vec<DownloadJob>) -> DesktopSnapshot {
    DesktopSnapshot {
        connection_state: ConnectionState::Connected,
        jobs,
        settings: Settings::default(),
    }
}

fn diagnostics_snapshot(
    status: HostRegistrationStatus,
    browser: &str,
    event_message: &str,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot {
        connection_state: ConnectionState::Connected,
        queue_summary: QueueSummary {
            total: 1,
            active: 0,
            attention: usize::from(status != HostRegistrationStatus::Configured),
            queued: 0,
            downloading: 0,
            completed: 1,
            failed: 0,
        },
        last_host_contact_seconds_ago: Some(4),
        host_registration: HostRegistrationDiagnostics {
            status,
            entries: vec![HostRegistrationEntry {
                browser: browser.into(),
                registry_path: format!("HKCU/{browser}/NativeMessagingHosts"),
                manifest_path: if status == HostRegistrationStatus::Missing {
                    None
                } else {
                    Some(format!("C:/Temp/{browser}.json"))
                },
                manifest_exists: status == HostRegistrationStatus::Configured,
                host_binary_path: if status == HostRegistrationStatus::Configured {
                    Some("C:/Program Files/SimpleDownloadManager/native-host.exe".into())
                } else {
                    None
                },
                host_binary_exists: status == HostRegistrationStatus::Configured,
            }],
        },
        torrent_diagnostics: Vec::new(),
        recent_events: vec![DiagnosticEvent {
            timestamp: 0,
            level: DiagnosticLevel::Info,
            category: "native_host".into(),
            message: event_message.into(),
            job_id: None,
        }],
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

fn failed_browser_download_job(id: &str) -> DownloadJob {
    let mut job = download_job(id, JobState::Failed);
    job.source = Some(DownloadSource {
        entry_point: "browser_download".into(),
        browser: "chrome".into(),
        extension_version: "0.3.51".into(),
        page_url: None,
        page_title: None,
        referrer: None,
        incognito: None,
    });
    job
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

fn drain_slint_events() {
    slint::invoke_from_event_loop(|| {
        let _ = slint::quit_event_loop();
    })
    .expect("quit callback should be queued");
    slint::run_event_loop().expect("Slint event loop should drain queued callbacks");
}

fn wait_for_toast_present(toast_state: &ToastRuntimeState, title: &str, message_fragment: &str) {
    for _ in 0..20 {
        if toast_present(toast_state, title, message_fragment) {
            return;
        }
        drain_slint_events();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        toast_present(toast_state, title, message_fragment),
        "expected toast '{title}' containing '{message_fragment}', got {:?}",
        toast_state.toasts()
    );
}

fn toast_present(toast_state: &ToastRuntimeState, title: &str, message_fragment: &str) -> bool {
    toast_state
        .toasts()
        .iter()
        .any(|toast| toast.title == title && toast.message.contains(message_fragment))
}
