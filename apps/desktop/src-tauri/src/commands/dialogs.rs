use std::path::PathBuf;

const DIAGNOSTICS_REPORT_FILE_NAME: &str = "simple-download-manager-diagnostics.json";
const TORRENT_FILE_FILTER_NAME: &str = "Torrent or magnet";
const TORRENT_FILE_EXTENSIONS: &[&str] = &["torrent", "magnet", "txt"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileDialogFilter<'a> {
    pub(super) name: &'a str,
    pub(super) extensions: &'a [&'a str],
}

pub(super) trait NativeDialogs {
    fn save_file(&self, default_file_name: &str) -> Option<PathBuf>;
    fn pick_directory(&self) -> Option<PathBuf>;
    fn pick_file(&self, filter: FileDialogFilter<'_>) -> Option<PathBuf>;
}

pub(super) struct SystemNativeDialogs;

impl NativeDialogs for SystemNativeDialogs {
    fn save_file(&self, default_file_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_file_name(default_file_name)
            .save_file()
    }

    fn pick_directory(&self) -> Option<PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }

    fn pick_file(&self, filter: FileDialogFilter<'_>) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter(filter.name, filter.extensions)
            .pick_file()
    }
}

pub(super) fn save_diagnostics_report_path() -> Option<PathBuf> {
    save_diagnostics_report_path_with(&SystemNativeDialogs)
}

pub(super) fn save_diagnostics_report_path_with(dialogs: &impl NativeDialogs) -> Option<PathBuf> {
    dialogs.save_file(DIAGNOSTICS_REPORT_FILE_NAME)
}

pub(super) fn pick_directory() -> Option<PathBuf> {
    pick_directory_with(&SystemNativeDialogs)
}

pub(super) fn pick_directory_with(dialogs: &impl NativeDialogs) -> Option<PathBuf> {
    dialogs.pick_directory()
}

pub(super) fn pick_torrent_file() -> Option<PathBuf> {
    pick_torrent_file_with(&SystemNativeDialogs)
}

pub(super) fn pick_torrent_file_with(dialogs: &impl NativeDialogs) -> Option<PathBuf> {
    dialogs.pick_file(FileDialogFilter {
        name: TORRENT_FILE_FILTER_NAME,
        extensions: TORRENT_FILE_EXTENSIONS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    #[derive(Default)]
    struct RecordingNativeDialogs {
        operations: RefCell<Vec<(String, String, Vec<String>)>>,
        save_file: Option<PathBuf>,
        directory: Option<PathBuf>,
        file: Option<PathBuf>,
    }

    impl NativeDialogs for RecordingNativeDialogs {
        fn save_file(&self, default_file_name: &str) -> Option<PathBuf> {
            self.operations.borrow_mut().push((
                "save_file".to_string(),
                default_file_name.to_string(),
                Vec::new(),
            ));
            self.save_file.clone()
        }

        fn pick_directory(&self) -> Option<PathBuf> {
            self.operations.borrow_mut().push((
                "pick_directory".to_string(),
                String::new(),
                Vec::new(),
            ));
            self.directory.clone()
        }

        fn pick_file(&self, filter: FileDialogFilter<'_>) -> Option<PathBuf> {
            self.operations.borrow_mut().push((
                "pick_file".to_string(),
                filter.name.to_string(),
                filter
                    .extensions
                    .iter()
                    .map(|extension| (*extension).to_string())
                    .collect(),
            ));
            self.file.clone()
        }
    }

    #[test]
    fn native_dialog_trait_records_dialog_requests() {
        let dialogs = RecordingNativeDialogs {
            save_file: Some(PathBuf::from(r"C:\Temp\diagnostics.json")),
            directory: Some(PathBuf::from(r"C:\Downloads")),
            file: Some(PathBuf::from(r"C:\Downloads\sample.torrent")),
            ..Default::default()
        };

        assert_eq!(
            save_diagnostics_report_path_with(&dialogs),
            Some(PathBuf::from(r"C:\Temp\diagnostics.json"))
        );
        assert_eq!(
            pick_directory_with(&dialogs),
            Some(PathBuf::from(r"C:\Downloads"))
        );
        assert_eq!(
            pick_torrent_file_with(&dialogs),
            Some(PathBuf::from(r"C:\Downloads\sample.torrent"))
        );

        assert_eq!(
            dialogs.operations.into_inner(),
            vec![
                (
                    "save_file".to_string(),
                    "simple-download-manager-diagnostics.json".to_string(),
                    Vec::new()
                ),
                ("pick_directory".to_string(), String::new(), Vec::new()),
                (
                    "pick_file".to_string(),
                    "Torrent or magnet".to_string(),
                    vec![
                        "torrent".to_string(),
                        "magnet".to_string(),
                        "txt".to_string()
                    ]
                ),
            ]
        );
    }
}
