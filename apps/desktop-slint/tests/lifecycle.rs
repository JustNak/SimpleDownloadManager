use serde_json::Value;
use simple_download_manager_desktop_slint::shell::lifecycle::{
    single_instance_show_window_request, SINGLE_INSTANCE_MUTEX_NAME, SINGLE_INSTANCE_REQUEST_ID,
};

#[test]
fn duplicate_instance_request_preserves_tauri_wire_contract() {
    let request = single_instance_show_window_request();
    let value: Value =
        serde_json::from_str(&request).expect("duplicate-instance request should be valid JSON");

    assert_eq!(
        SINGLE_INSTANCE_MUTEX_NAME,
        "Local\\SimpleDownloadManager.SingleInstance"
    );
    assert_eq!(SINGLE_INSTANCE_REQUEST_ID, "desktop-single-instance");
    assert_eq!(value["protocolVersion"], 1);
    assert_eq!(value["requestId"], SINGLE_INSTANCE_REQUEST_ID);
    assert_eq!(value["type"], "show_window");
    assert_eq!(value["payload"]["reason"], "user_request");
}
