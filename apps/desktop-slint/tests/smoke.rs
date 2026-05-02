use simple_download_manager_desktop_slint::smoke::{
    parse_smoke_command_from_args, run_smoke_command_with, SmokeCommand,
};

#[test]
fn smoke_sync_autostart_command_accepts_enable_and_disable_only() {
    assert_eq!(
        parse_smoke_command_from_args(["simple-download-manager", "--smoke-sync-autostart=enable"])
            .unwrap(),
        Some(SmokeCommand::SyncAutostart { enabled: true })
    );
    assert_eq!(
        parse_smoke_command_from_args([
            "simple-download-manager",
            "--smoke-sync-autostart=disable"
        ])
        .unwrap(),
        Some(SmokeCommand::SyncAutostart { enabled: false })
    );
    assert_eq!(
        parse_smoke_command_from_args(["simple-download-manager", "--autostart"]).unwrap(),
        None
    );
    assert!(parse_smoke_command_from_args([
        "simple-download-manager",
        "--smoke-sync-autostart=maybe"
    ])
    .is_err());
}

#[test]
fn smoke_command_is_ignored_unless_environment_guard_is_enabled() {
    let mut calls = Vec::new();
    let handled = run_smoke_command_with(
        ["simple-download-manager", "--smoke-sync-autostart=enable"],
        false,
        |enabled| {
            calls.push(enabled);
            Ok(())
        },
    )
    .unwrap();

    assert!(!handled);
    assert!(calls.is_empty());
}

#[test]
fn smoke_command_delegates_to_startup_sync_when_environment_guard_is_enabled() {
    let mut calls = Vec::new();
    let handled = run_smoke_command_with(
        ["simple-download-manager", "--smoke-sync-autostart=disable"],
        true,
        |enabled| {
            calls.push(enabled);
            Ok(())
        },
    )
    .unwrap();

    assert!(handled);
    assert_eq!(calls, vec![false]);
}
