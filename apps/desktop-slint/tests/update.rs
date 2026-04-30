use simple_download_manager_desktop_slint::update::{
    SLINT_UPDATER_FEED_NAME, TAURI_TRANSITION_FEED_NAME,
};

#[test]
fn updater_feed_names_preserve_tauri_transition_and_add_slint_channel() {
    assert_eq!(TAURI_TRANSITION_FEED_NAME, "latest-alpha.json");
    assert_eq!(SLINT_UPDATER_FEED_NAME, "latest-alpha-slint.json");
}
