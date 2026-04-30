use simple_download_manager_desktop_core::state::{validate_settings, EnqueueOptions, SharedState};
use simple_download_manager_desktop_core::storage::Settings;

#[test]
fn core_exports_state_api_needed_by_desktop_shells() {
    fn assert_clone<T: Clone>() {}

    assert_clone::<SharedState>();

    let options = EnqueueOptions::default();
    assert!(options.source.is_none());
}

#[test]
fn core_exports_settings_validation_from_state_layer() {
    let mut settings = Settings {
        download_directory: "target/test-downloads/state-contracts".into(),
        ..Settings::default()
    };

    validate_settings(&mut settings).expect("settings validation should be available from core");
}
