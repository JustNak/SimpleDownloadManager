use std::fs;
use std::path::Path;

#[test]
fn slint_runtime_stays_tauri_free_and_uses_event_loop_bridge() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(manifest_dir.join("Cargo.toml"))
        .expect("Slint manifest should load");
    let runtime_source = fs::read_to_string(manifest_dir.join("src/runtime.rs"))
        .expect("runtime source should load");

    for forbidden_dependency in [
        "tauri",
        "tauri-plugin",
        "rfd",
        "winreg",
        "windows-sys",
    ] {
        assert!(
            !manifest
                .lines()
                .any(|line| line.starts_with(&format!("{forbidden_dependency} "))),
            "apps/desktop-slint must not depend on {forbidden_dependency}"
        );
    }

    assert!(
        runtime_source.contains("slint::invoke_from_event_loop"),
        "Slint runtime must bridge background backend events through slint::invoke_from_event_loop"
    );
}
