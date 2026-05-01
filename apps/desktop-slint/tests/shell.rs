use simple_download_manager_desktop_core::storage::{
    HostRegistrationEntry, HostRegistrationStatus, MainWindowState, StartupLaunchMode,
};
use simple_download_manager_desktop_slint::shell::{
    main_window, native_host, notifications, popups, tray, windows, WindowRole, WindowSize,
};
use slint::{PhysicalPosition, PhysicalSize};
use std::path::Path;

#[test]
fn shell_window_roles_preserve_existing_fixed_popup_sizes() {
    assert_eq!(
        WindowRole::Main.default_size(),
        WindowSize {
            width: 1360,
            height: 860
        }
    );
    assert_eq!(
        WindowRole::DownloadPrompt.default_size(),
        WindowSize {
            width: 460,
            height: 280
        }
    );
    assert_eq!(
        WindowRole::HttpProgress.default_size(),
        WindowSize {
            width: 460,
            height: 280
        }
    );
    assert_eq!(
        WindowRole::TorrentProgress.default_size(),
        WindowSize {
            width: 720,
            height: 520
        }
    );
    assert_eq!(
        WindowRole::BatchProgress.default_size(),
        WindowSize {
            width: 560,
            height: 430
        }
    );
}

#[test]
fn main_window_config_preserves_tauri_dimensions_and_title() {
    let config = main_window::main_window_config();

    assert_eq!(config.title, "Simple Download Manager");
    assert!(config.frameless);
    assert_eq!(
        config.preferred_size,
        WindowSize {
            width: 1360,
            height: 860
        }
    );
    assert_eq!(
        config.minimum_size,
        WindowSize {
            width: 1360,
            height: 720
        }
    );
}

#[test]
fn main_window_titlebar_contract_matches_tauri_shell() {
    assert_eq!(main_window::TITLEBAR_HEIGHT, 44);
    assert_eq!(main_window::TITLEBAR_TITLE, "Download Manager");
    assert_eq!(
        main_window::titlebar_action_for_drag_band(false),
        main_window::TitlebarAction::StartDrag
    );
    assert_eq!(
        main_window::titlebar_action_for_drag_band(true),
        main_window::TitlebarAction::ToggleMaximize
    );
    assert_eq!(
        main_window::titlebar_action_for_control(main_window::TitlebarControl::Minimize),
        main_window::TitlebarAction::Minimize
    );
    assert_eq!(
        main_window::titlebar_action_for_control(main_window::TitlebarControl::Maximize),
        main_window::TitlebarAction::ToggleMaximize
    );
    assert_eq!(
        main_window::titlebar_action_for_control(main_window::TitlebarControl::Close),
        main_window::TitlebarAction::CloseToTray
    );
}

#[test]
fn main_window_startup_visibility_matches_tauri_policy() {
    assert_eq!(main_window::AUTOSTART_ARG, "--autostart");
    assert_eq!(main_window::POST_UPDATE_ARG, "--post-update");

    assert!(main_window::is_autostart_launch_from_args([
        "simple-download-manager",
        "--autostart",
    ]));
    assert!(!main_window::is_autostart_launch_from_args([
        "simple-download-manager",
        "--flag=--autostart",
    ]));
    assert!(main_window::is_post_update_launch_from_args([
        "simple-download-manager",
        "--post-update",
    ]));
    assert!(!main_window::is_post_update_launch_from_args([
        "simple-download-manager",
        "--not-post-update",
    ]));

    assert_eq!(
        main_window::startup_window_action(false, false, StartupLaunchMode::Tray),
        main_window::StartupWindowAction::Show
    );
    assert_eq!(
        main_window::startup_window_action(true, false, StartupLaunchMode::Tray),
        main_window::StartupWindowAction::KeepHidden
    );
    assert_eq!(
        main_window::startup_window_action(true, false, StartupLaunchMode::Open),
        main_window::StartupWindowAction::Show
    );
    assert_eq!(
        main_window::startup_window_action(true, true, StartupLaunchMode::Tray),
        main_window::StartupWindowAction::Show
    );
}

#[test]
fn main_window_state_conversions_preserve_persisted_geometry() {
    let persisted = MainWindowState {
        width: 1440,
        height: 900,
        x: 42,
        y: 84,
        maximized: true,
    };

    assert_eq!(
        main_window::persisted_state_size(&persisted),
        Some(PhysicalSize::new(1440, 900))
    );
    assert_eq!(
        main_window::persisted_state_position(&persisted),
        PhysicalPosition::new(42, 84)
    );
    assert_eq!(
        main_window::main_window_state_from_parts(
            PhysicalSize::new(1280, 720),
            PhysicalPosition::new(10, 20),
            false,
        ),
        MainWindowState {
            width: 1280,
            height: 720,
            x: 10,
            y: 20,
            maximized: false,
        }
    );
    assert_eq!(
        main_window::persisted_state_size(&MainWindowState {
            width: 0,
            height: 900,
            x: 0,
            y: 0,
            maximized: false,
        }),
        None
    );
}

#[test]
fn windows_shell_autostart_contract_matches_tauri() {
    let executable =
        Path::new(r"C:\Program Files\Simple Download Manager\simple-download-manager.exe");

    assert_eq!(
        windows::STARTUP_REGISTRY_VALUE_NAME,
        "Simple Download Manager"
    );
    assert_eq!(windows::AUTOSTART_ARG, "--autostart");
    assert_eq!(
        windows::autostart_command_for_executable(executable, windows::AUTOSTART_ARG),
        r#""C:\Program Files\Simple Download Manager\simple-download-manager.exe" --autostart"#
    );
}

#[test]
fn windows_shell_reveal_contract_matches_tauri() {
    let file_path = Path::new(r"C:\Downloads\archive.zip");

    assert_eq!(
        windows::explorer_select_arguments(file_path),
        r#"/select,"C:\Downloads\archive.zip""#
    );
    assert_eq!(
        windows::reveal_launch_request_for_path(file_path),
        windows::ShellLaunchRequest::RevealFile {
            explorer: "explorer.exe".into(),
            arguments: r#"/select,"C:\Downloads\archive.zip""#.into(),
        }
    );
}

#[test]
fn windows_shell_reveal_routes_directories_to_direct_open() {
    let dir = std::env::current_dir()
        .unwrap()
        .join("test-shell")
        .join(format!("direct-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    assert_eq!(
        windows::reveal_launch_request_for_path(&dir),
        windows::ShellLaunchRequest::OpenPath(dir.clone())
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(not(windows))]
#[test]
fn non_windows_shell_effects_return_explicit_unsupported_errors() {
    assert_eq!(
        windows::open_url("https://example.test").unwrap_err(),
        "Opening downloads in the browser is only supported on Windows in this build."
    );
    assert_eq!(
        windows::sync_autostart_setting(false).unwrap_err(),
        "Startup registration is only supported on Windows in this build."
    );
}

#[test]
fn tray_contract_preserves_tauri_menu_and_tooltip() {
    assert_eq!(tray::TRAY_TOOLTIP, "Simple Download Manager");
    assert_eq!(tray::TRAY_MENU_OPEN_ID, "open");
    assert_eq!(tray::TRAY_MENU_EXIT_ID, "exit");
    assert_eq!(tray::TRAY_MENU_OPEN_LABEL, "Open");
    assert_eq!(tray::TRAY_MENU_EXIT_LABEL, "Exit");
}

#[test]
fn tray_menu_and_click_events_route_to_window_actions() {
    assert_eq!(
        tray::action_for_menu_id(tray::TRAY_MENU_OPEN_ID),
        Some(tray::TrayAction::OpenMainWindow)
    );
    assert_eq!(
        tray::action_for_menu_id(tray::TRAY_MENU_EXIT_ID),
        Some(tray::TrayAction::ExitApplication)
    );
    assert_eq!(tray::action_for_menu_id("unknown"), None);
    assert_eq!(
        tray::action_for_mouse_click(tray::MouseButton::Left, tray::MouseButtonState::Up),
        Some(tray::TrayAction::OpenMainWindow)
    );
    assert_eq!(
        tray::action_for_mouse_click(tray::MouseButton::Right, tray::MouseButtonState::Up),
        None
    );
    assert_eq!(
        tray::action_for_mouse_click(tray::MouseButton::Left, tray::MouseButtonState::Down),
        None
    );
}

#[test]
fn close_to_tray_action_hides_without_quitting() {
    assert_eq!(
        main_window::close_to_tray_action(),
        main_window::CloseToTrayAction::HideWindow
    );
}

#[test]
fn popup_window_contracts_match_tauri_sizes_and_titles() {
    assert_eq!(
        popups::popup_window_config(WindowRole::DownloadPrompt),
        popups::PopupWindowConfig {
            title: "New download detected",
            size: WindowSize {
                width: 460,
                height: 280,
            },
        }
    );
    assert_eq!(
        popups::popup_window_config(WindowRole::HttpProgress),
        popups::PopupWindowConfig {
            title: "Download progress",
            size: WindowSize {
                width: 460,
                height: 280,
            },
        }
    );
    assert_eq!(
        popups::popup_window_config(WindowRole::TorrentProgress),
        popups::PopupWindowConfig {
            title: "Torrent session",
            size: WindowSize {
                width: 720,
                height: 520,
            },
        }
    );
    assert_eq!(
        popups::popup_window_config(WindowRole::BatchProgress),
        popups::PopupWindowConfig {
            title: "Batch progress",
            size: WindowSize {
                width: 560,
                height: 430,
            },
        }
    );
}

#[test]
fn popup_labels_are_sanitized_like_tauri_windows() {
    assert_eq!(
        popups::progress_window_label("job:one/../two?bad"),
        "download-progress-job:one//twobad"
    );
    assert_eq!(
        popups::torrent_progress_window_label("job:one/../two?bad"),
        "torrent-progress-job:one//twobad"
    );
    assert_eq!(
        popups::batch_progress_window_label("batch:one/../two?bad"),
        "batch-progress-batchonetwobad"
    );
}

#[test]
fn progress_window_position_stacks_from_prompt_position() {
    let prompt_position = popups::PopupWindowPosition { x: 280, y: 221 };

    assert_eq!(
        popups::progress_window_position(Some(prompt_position), 0),
        Some(prompt_position)
    );
    assert_eq!(
        popups::progress_window_position(Some(prompt_position), 2),
        Some(popups::PopupWindowPosition { x: 336, y: 277 })
    );
    assert_eq!(
        popups::progress_window_position(Some(prompt_position), 20),
        Some(popups::PopupWindowPosition { x: 504, y: 445 })
    );
    assert_eq!(popups::progress_window_position(None, 2), None);
}

#[test]
fn prompt_close_remembers_position_only_when_requested() {
    let mut state = popups::PopupLifecycleState::default();
    let position = popups::PopupWindowPosition { x: 42, y: 84 };

    state.close_prompt(false, Some(position));
    assert_eq!(state.last_prompt_position(), None);

    state.close_prompt(true, Some(position));
    assert_eq!(state.last_prompt_position(), Some(position));
}

#[test]
fn pending_selected_job_is_taken_once() {
    let pending = popups::PendingSelectedJob::default();

    pending.queue("job_42".into());

    assert_eq!(pending.take(), Some("job_42".into()));
    assert_eq!(pending.take(), None);
}

#[test]
fn notification_request_preserves_app_identity_and_payload() {
    let request = notifications::NotificationRequest::new("Download complete", "archive.zip");

    assert_eq!(notifications::APP_NAME, "Simple Download Manager");
    assert_eq!(request.app_name, "Simple Download Manager");
    assert_eq!(request.title, "Download complete");
    assert_eq!(request.body, "archive.zip");
}

#[test]
fn native_host_contract_matches_tauri_registration_values() {
    assert_eq!(native_host::NATIVE_HOST_NAME, "com.myapp.download_manager");
    assert_eq!(
        native_host::DEFAULT_CHROMIUM_EXTENSION_ID,
        "pkaojpfpjieklhinoibjibmjldohlmbb"
    );
    assert_eq!(
        native_host::DEFAULT_FIREFOX_EXTENSION_ID,
        "simple-download-manager@example.com"
    );
    assert_eq!(
        native_host::CHROME_REGISTRY_PATH,
        r"Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager"
    );
    assert_eq!(
        native_host::EDGE_REGISTRY_PATH,
        r"Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager"
    );
    assert_eq!(
        native_host::FIREFOX_REGISTRY_PATH,
        r"Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager"
    );
    assert_eq!(
        native_host::manifest_filenames(),
        [
            "com.myapp.download_manager.chrome.json",
            "com.myapp.download_manager.edge.json",
            "com.myapp.download_manager.firefox.json",
        ]
    );
    assert_eq!(
        native_host::host_binary_candidate_names(&native_host::ReleaseMetadata::default()),
        [
            "simple-download-manager-native-host.exe",
            "simple-download-manager-native-host-x86_64-pc-windows-msvc.exe",
        ]
    );
}

#[test]
fn native_host_manifest_json_uses_browser_allowlists() {
    let host_path = Path::new(
        r"C:\Program Files\Simple Download Manager\simple-download-manager-native-host.exe",
    );
    let chrome = native_host::native_host_manifest_json(
        host_path,
        "allowed_origins",
        serde_json::json!(["chrome-extension://extension-id/"]),
    );
    let firefox = native_host::native_host_manifest_json(
        host_path,
        "allowed_extensions",
        serde_json::json!(["simple-download-manager@example.com"]),
    );

    assert_eq!(chrome["name"], native_host::NATIVE_HOST_NAME);
    assert_eq!(
        chrome["description"],
        "Simple Download Manager native messaging host"
    );
    assert_eq!(chrome["path"], host_path.display().to_string());
    assert_eq!(chrome["type"], "stdio");
    assert_eq!(
        chrome["allowed_origins"][0],
        "chrome-extension://extension-id/"
    );
    assert_eq!(
        firefox["allowed_extensions"][0],
        "simple-download-manager@example.com"
    );
}

#[test]
fn native_host_edge_extension_id_falls_back_to_custom_chromium_id() {
    let ids = native_host::browser_extension_ids(&native_host::ReleaseMetadata {
        sidecar_binary_name: None,
        chromium_extension_id: Some("custom-chromium".into()),
        edge_extension_id: None,
        firefox_extension_id: None,
    });

    assert_eq!(ids.chromium, "custom-chromium");
    assert_eq!(ids.edge, "custom-chromium");
    assert_eq!(ids.firefox, native_host::DEFAULT_FIREFOX_EXTENSION_ID);
}

#[test]
fn native_host_diagnostic_classification_matches_tauri() {
    assert_eq!(
        native_host::classify_host_registration_entries(&[]),
        HostRegistrationStatus::Missing
    );
    assert_eq!(
        native_host::classify_host_registration_entries(&[HostRegistrationEntry {
            browser: "Chrome".into(),
            registry_path: native_host::CHROME_REGISTRY_PATH.into(),
            manifest_path: Some(r"C:\missing.json".into()),
            manifest_exists: false,
            host_binary_path: None,
            host_binary_exists: false,
        }]),
        HostRegistrationStatus::Broken
    );
    assert_eq!(
        native_host::classify_host_registration_entries(&[HostRegistrationEntry {
            browser: "Chrome".into(),
            registry_path: native_host::CHROME_REGISTRY_PATH.into(),
            manifest_path: Some(r"C:\host.json".into()),
            manifest_exists: true,
            host_binary_path: Some(r"C:\host.exe".into()),
            host_binary_exists: true,
        }]),
        HostRegistrationStatus::Configured
    );
}

#[test]
fn native_host_repair_policy_matches_tauri() {
    assert!(!native_host::should_register_native_host(
        HostRegistrationStatus::Configured
    ));
    assert!(native_host::should_register_native_host(
        HostRegistrationStatus::Missing
    ));
    assert!(native_host::should_register_native_host(
        HostRegistrationStatus::Broken
    ));
}

#[test]
fn invalid_native_host_manifest_is_reported_as_broken_entry() {
    let dir = std::env::current_dir()
        .unwrap()
        .join("test-shell")
        .join(format!(
            "invalid-native-host-manifest-{}",
            std::process::id()
        ));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest_path = dir.join("native-host.json");
    std::fs::write(&manifest_path, "{not valid json").unwrap();

    let entry = native_host::read_host_registration_entry(
        "Chrome",
        native_host::CHROME_REGISTRY_PATH,
        &manifest_path,
    )
    .expect("invalid JSON should be reported as a broken entry");

    assert!(entry.manifest_exists);
    assert_eq!(
        entry.manifest_path,
        Some(manifest_path.display().to_string())
    );
    assert_eq!(entry.host_binary_path, None);
    assert!(!entry.host_binary_exists);

    let _ = std::fs::remove_dir_all(dir);
}
