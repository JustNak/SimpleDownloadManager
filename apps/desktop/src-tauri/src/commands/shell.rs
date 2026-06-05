use std::path::Path;

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows_sys::Win32::UI::Shell::ShellExecuteW;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[derive(Debug, Clone, Copy)]
pub(super) enum ExternalOpenOperation<'a> {
    Url(&'a str),
    OpenPath(&'a Path),
    RevealPath(&'a Path),
}

impl ExternalOpenOperation<'_> {
    #[cfg(test)]
    pub(super) const fn as_str(&self) -> &'static str {
        match self {
            Self::Url(_) => "url",
            Self::OpenPath(_) => "open_path",
            Self::RevealPath(_) => "reveal_path",
        }
    }

    #[cfg(test)]
    pub(super) fn target_display(&self) -> String {
        match self {
            Self::Url(url) => (*url).to_string(),
            Self::OpenPath(path) | Self::RevealPath(path) => path.display().to_string(),
        }
    }
}

pub(super) trait ExternalOpener {
    fn open_external(&self, operation: ExternalOpenOperation<'_>) -> Result<(), String>;
}

pub(super) struct SystemExternalOpener;

impl ExternalOpener for SystemExternalOpener {
    fn open_external(&self, operation: ExternalOpenOperation<'_>) -> Result<(), String> {
        match operation {
            ExternalOpenOperation::Url(url) => system_open_url(url),
            ExternalOpenOperation::OpenPath(path) => system_open_path(path),
            ExternalOpenOperation::RevealPath(path) => system_reveal_path(path),
        }
    }
}

pub(super) fn open_url_with(opener: &impl ExternalOpener, url: &str) -> Result<(), String> {
    opener.open_external(ExternalOpenOperation::Url(url))
}

pub(super) fn open_path_with(opener: &impl ExternalOpener, path: &Path) -> Result<(), String> {
    opener.open_external(ExternalOpenOperation::OpenPath(path))
}

pub(super) fn reveal_path_with(opener: &impl ExternalOpener, path: &Path) -> Result<(), String> {
    opener.open_external(ExternalOpenOperation::RevealPath(path))
}

pub(super) fn open_url(url: &str) -> Result<(), String> {
    open_url_with(&SystemExternalOpener, url)
}

pub(super) fn open_path(path: &Path) -> Result<(), String> {
    open_path_with(&SystemExternalOpener, path)
}

pub(super) fn reveal_path(path: &Path) -> Result<(), String> {
    reveal_path_with(&SystemExternalOpener, path)
}

#[cfg(windows)]
fn system_open_url(url: &str) -> Result<(), String> {
    shell_execute(OsStr::new("open"), OsStr::new(url), None)
}

#[cfg(not(windows))]
fn system_open_url(_url: &str) -> Result<(), String> {
    Err("Opening downloads in the browser is only supported on Windows in this build.".into())
}

#[cfg(windows)]
fn system_open_path(path: &Path) -> Result<(), String> {
    shell_execute(OsStr::new("open"), path.as_os_str(), None)
}

#[cfg(not(windows))]
fn system_open_path(_path: &Path) -> Result<(), String> {
    Err("Opening files is only supported on Windows in this build.".into())
}

#[cfg(windows)]
fn system_reveal_path(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        return system_open_path(path);
    }

    let arguments = format!("/select,\"{}\"", path.display());
    shell_execute(
        OsStr::new("open"),
        OsStr::new("explorer.exe"),
        Some(OsStr::new(&arguments)),
    )
}

#[cfg(not(windows))]
fn system_reveal_path(_path: &Path) -> Result<(), String> {
    Err("Revealing files is only supported on Windows in this build.".into())
}

#[cfg(windows)]
fn shell_execute(
    operation: &OsStr,
    file: &OsStr,
    parameters: Option<&OsStr>,
) -> Result<(), String> {
    let operation = wide_null(operation);
    let file = wide_null(file);
    let parameters = parameters.map(wide_null);
    let parameters_ptr = parameters
        .as_ref()
        .map(|value| value.as_ptr())
        .unwrap_or(std::ptr::null());

    // ShellExecuteW opens files and folders without showing a console window.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            parameters_ptr,
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;

    if result <= 32 {
        return Err(format!("ShellExecuteW failed with code {result}."));
    }

    Ok(())
}

#[cfg(windows)]
fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct RecordingExternalOpener {
        operations: RefCell<Vec<(String, String)>>,
    }

    impl ExternalOpener for RecordingExternalOpener {
        fn open_external(&self, operation: ExternalOpenOperation<'_>) -> Result<(), String> {
            self.operations
                .borrow_mut()
                .push((operation.as_str().to_string(), operation.target_display()));
            Ok(())
        }
    }

    #[test]
    fn external_opener_trait_records_typed_operations() {
        let opener = RecordingExternalOpener::default();
        let path = Path::new(r"C:\Downloads\file.zip");

        open_url_with(&opener, "https://example.test/file.zip").unwrap();
        open_path_with(&opener, path).unwrap();
        reveal_path_with(&opener, path).unwrap();

        assert_eq!(
            opener.operations.into_inner(),
            vec![
                (
                    "url".to_string(),
                    "https://example.test/file.zip".to_string()
                ),
                ("open_path".to_string(), path.display().to_string()),
                ("reveal_path".to_string(), path.display().to_string()),
            ]
        );
    }
}
