use std::fs;
use std::path::Path;

#[test]
fn slint_runtime_stays_tauri_free_and_uses_event_loop_bridge() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest =
        fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("Slint manifest should load");
    let runtime_source = fs::read_to_string(manifest_dir.join("src/runtime.rs"))
        .expect("runtime source should load");
    let lifecycle_source = fs::read_to_string(manifest_dir.join("src/shell/lifecycle.rs"))
        .expect("lifecycle source should load");
    let windows_shell_source = fs::read_to_string(manifest_dir.join("src/shell/windows.rs"))
        .expect("Windows shell source should load");
    let notifications_source = fs::read_to_string(manifest_dir.join("src/shell/notifications.rs"))
        .expect("notification shell source should load");
    let clipboard_source = fs::read_to_string(manifest_dir.join("src/shell/clipboard.rs"))
        .expect("clipboard shell source should load");
    let native_host_source = fs::read_to_string(manifest_dir.join("src/shell/native_host.rs"))
        .expect("native-host shell source should load");
    let popup_source = fs::read_to_string(manifest_dir.join("src/shell/popups.rs"))
        .expect("popup source should load");
    let tray_source = fs::read_to_string(manifest_dir.join("src/shell/tray.rs"))
        .expect("tray source should load");
    let update_source = fs::read_to_string(manifest_dir.join("src/update/mod.rs"))
        .expect("update source should load");
    let ipc_source =
        fs::read_to_string(manifest_dir.join("src/ipc/mod.rs")).expect("IPC source should load");

    for forbidden_dependency in ["tauri", "tauri-plugin"] {
        assert!(
            !manifest
                .lines()
                .any(|line| line.starts_with(&format!("{forbidden_dependency} "))),
            "apps/desktop-slint must not depend on {forbidden_dependency}"
        );
    }
    for native_shell_dependency in ["rfd ", "winreg "] {
        assert!(
            manifest
                .lines()
                .any(|line| line.starts_with(native_shell_dependency)),
            "Phase 3D shell effects should declare {native_shell_dependency}"
        );
    }
    assert!(
        manifest.contains("[target.'cfg(windows)'.dependencies]")
            && manifest.contains("tray-icon = \"0.21.3\"")
            && manifest.contains("notify-rust = \"4.14.0\"")
            && manifest.contains("clipboard-win = \"5.4.1\""),
        "native shell support should keep tray-icon, notify-rust, and clipboard-win scoped to Windows builds"
    );
    assert!(
        manifest.contains("cargo-packager-updater = \"0.2.3\""),
        "Phase 3H Slint updater support should use cargo-packager-updater"
    );
    assert!(
        !manifest.contains("tauri-plugin-updater"),
        "Slint updater must not reintroduce the Tauri updater plugin"
    );

    assert!(
        manifest
            .lines()
            .any(|line| line.starts_with("windows-sys ")),
        "apps/desktop-slint should depend on windows-sys only for Windows lifecycle support"
    );
    assert!(
        lifecycle_source.contains("windows_sys::"),
        "Windows API usage should stay in the lifecycle module"
    );
    assert!(
        windows_shell_source.contains("windows_sys::")
            && windows_shell_source.contains("winreg")
            && windows_shell_source.contains("rfd::FileDialog"),
        "Windows shell APIs should stay isolated in the Slint shell module"
    );
    assert!(
        notifications_source.contains("notify_rust::Notification"),
        "native notifications should stay isolated in shell::notifications"
    );
    assert!(
        clipboard_source.contains("clipboard_win::set_clipboard_string"),
        "Windows clipboard writes should stay isolated in shell::clipboard"
    );
    assert!(
        native_host_source.contains("winreg")
            && native_host_source.contains("NativeMessagingHosts")
            && native_host_source.contains("native_host_manifest_json"),
        "native-host registry and manifest repair should stay isolated in shell::native_host"
    );
    assert!(
        !runtime_source.contains("windows_sys::")
            && !runtime_source.contains("winreg::")
            && !runtime_source.contains("notify_rust::")
            && !runtime_source.contains("clipboard_win::")
            && !runtime_source.contains("tray_icon::")
            && !runtime_source.contains("cargo_packager_updater::")
            && !ipc_source.contains("windows_sys::")
            && !ipc_source.contains("winreg::"),
        "runtime and IPC transport should not call Windows, notification, tray, updater, or registry APIs directly"
    );
    assert!(
        runtime_source.contains("shell::notifications::show_notification")
            && runtime_source.contains("shell::native_host::gather_host_registration_diagnostics")
            && runtime_source.contains("shell::native_host::register_native_host")
            && runtime_source.contains("shell::native_host::ensure_native_host_registration")
            && runtime_source.contains("update::check_for_update(&pending_update)")
            && runtime_source.contains("update::install_update_with_progress"),
        "SlintShellServices should delegate notifications, native-host repair, and updater work through shell/update modules"
    );
    assert!(
        update_source.contains("cargo_packager_updater")
            && update_source.contains("download_and_install_extended")
            && update_source.contains("WindowsUpdateInstallMode::Passive"),
        "cargo-packager updater calls should stay isolated in the Slint update module"
    );
    assert!(
        runtime_source.contains("DesktopEvent::UpdateInstallProgress(event)")
            && runtime_source.contains("UiAction::ApplyUpdateState"),
        "Slint runtime should handle updater install progress through the UI event bridge"
    );
    assert!(
        tray_source.contains("tray_icon::")
            && tray_source.contains("slint::invoke_from_event_loop"),
        "tray integration should stay isolated in shell::tray and bridge events through Slint"
    );
    assert!(
        popup_source.contains("DownloadPromptWindow")
            && popup_source.contains("HttpProgressWindow")
            && popup_source.contains("TorrentProgressWindow")
            && popup_source.contains("BatchProgressWindow"),
        "prompt/progress popup lifecycle should stay isolated in shell::popups"
    );
    assert!(
        !popup_source.contains("tauri") && !popup_source.contains("tauri_plugin"),
        "Slint popup lifecycle must remain Tauri-free"
    );
    for source in [
        &notifications_source,
        &clipboard_source,
        &native_host_source,
    ] {
        assert!(
            !source.contains("tauri::") && !source.contains("tauri_plugin"),
            "Slint native shell modules must remain Tauri-free"
        );
    }
    assert!(
        !runtime_source.contains("progress window requested for")
            && !runtime_source.contains("batch progress window requested for"),
        "runtime should not keep placeholder progress popup logging once popup lifecycle exists"
    );
    assert!(
        runtime_source.contains("slint::invoke_from_event_loop"),
        "Slint runtime must bridge background backend events through slint::invoke_from_event_loop"
    );
    assert!(
        ipc_source.contains("host_protocol::{HostRequest, HostResponse}")
            && ipc_source.contains("handle_host_request"),
        "Slint IPC should delegate core host request/response handling instead of owning protocol semantics"
    );
    for forbidden_fragment in [
        "validate_host_request",
        "PROTOCOL_VERSION",
        "MAX_URL_LENGTH",
        "SIDE_EFFECT_REQUEST_LIMIT",
    ] {
        assert!(
            !ipc_source.contains(forbidden_fragment),
            "Slint IPC must not duplicate core host protocol validation: {forbidden_fragment}"
        );
    }
}
