use std::path::Path;

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows_sys::Win32::UI::Shell::ShellExecuteW;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[cfg(windows)]
pub(super) fn open_url(url: &str) -> Result<(), String> {
    shell_execute(OsStr::new("open"), OsStr::new(url), None)
}

#[cfg(not(windows))]
pub(super) fn open_url(_url: &str) -> Result<(), String> {
    Err("Opening downloads in the browser is only supported on Windows in this build.".into())
}

#[cfg(windows)]
pub(super) fn open_path(path: &Path) -> Result<(), String> {
    shell_execute(OsStr::new("open"), path.as_os_str(), None)
}

#[cfg(not(windows))]
pub(super) fn open_path(_path: &Path) -> Result<(), String> {
    Err("Opening files is only supported on Windows in this build.".into())
}

#[cfg(windows)]
pub(super) fn reveal_path(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        return open_path(path);
    }

    let arguments = format!("/select,\"{}\"", path.display());
    shell_execute(
        OsStr::new("open"),
        OsStr::new("explorer.exe"),
        Some(OsStr::new(&arguments)),
    )
}

#[cfg(not(windows))]
pub(super) fn reveal_path(_path: &Path) -> Result<(), String> {
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
