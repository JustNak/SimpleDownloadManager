use simple_download_manager_desktop_slint::shell::{WindowRole, WindowSize};

#[test]
fn shell_window_roles_preserve_existing_fixed_popup_sizes() {
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
