pub mod clipboard;
pub mod lifecycle;
pub mod main_window;
pub mod native_host;
pub mod notifications;
pub mod popups;
pub mod tray;
pub mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRole {
    Main,
    DownloadPrompt,
    HttpProgress,
    TorrentProgress,
    BatchProgress,
}

impl WindowRole {
    pub fn default_size(self) -> WindowSize {
        match self {
            Self::Main => WindowSize {
                width: 1360,
                height: 860,
            },
            Self::DownloadPrompt | Self::HttpProgress => WindowSize {
                width: 460,
                height: 280,
            },
            Self::TorrentProgress => WindowSize {
                width: 720,
                height: 520,
            },
            Self::BatchProgress => WindowSize {
                width: 560,
                height: 430,
            },
        }
    }

    pub fn is_fixed_popup(self) -> bool {
        matches!(
            self,
            Self::DownloadPrompt | Self::HttpProgress | Self::TorrentProgress | Self::BatchProgress
        )
    }
}
