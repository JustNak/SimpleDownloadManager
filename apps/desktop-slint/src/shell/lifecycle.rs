#![allow(unsafe_code)]

#[cfg(windows)]
use crate::ipc::PIPE_NAME;

pub const SINGLE_INSTANCE_MUTEX_NAME: &str = "Local\\SimpleDownloadManager.SingleInstance";
pub const SINGLE_INSTANCE_REQUEST_ID: &str = "desktop-single-instance";

#[cfg(windows)]
pub struct SingleInstanceGuard {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(not(windows))]
pub struct SingleInstanceGuard;

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(windows)]
pub fn acquire_single_instance_or_notify() -> Result<Option<SingleInstanceGuard>, String> {
    let mutex_name = wide_null(SINGLE_INSTANCE_MUTEX_NAME);
    let handle = unsafe {
        windows_sys::Win32::System::Threading::CreateMutexW(
            std::ptr::null_mut(),
            1,
            mutex_name.as_ptr(),
        )
    };
    if handle.is_null() {
        return Err("Could not create application single-instance mutex.".into());
    }

    let already_running = unsafe { windows_sys::Win32::Foundation::GetLastError() }
        == windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS;
    if already_running {
        if let Err(error) = notify_existing_instance_show_window() {
            eprintln!("failed to notify existing app instance: {error}");
        }
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(handle);
        }
        return Ok(None);
    }

    Ok(Some(SingleInstanceGuard { handle }))
}

#[cfg(not(windows))]
pub fn acquire_single_instance_or_notify() -> Result<Option<SingleInstanceGuard>, String> {
    Ok(Some(SingleInstanceGuard))
}

pub fn single_instance_show_window_request() -> String {
    format!(
        r#"{{"protocolVersion":1,"requestId":"{SINGLE_INSTANCE_REQUEST_ID}","type":"show_window","payload":{{"reason":"user_request"}}}}"#
    )
}

#[cfg(windows)]
fn notify_existing_instance_show_window() -> Result<(), String> {
    use std::io::Write;

    let mut pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(PIPE_NAME)
        .map_err(|error| format!("Could not connect to existing app instance: {error}"))?;

    pipe.write_all(single_instance_show_window_request().as_bytes())
        .map_err(|error| format!("Could not write existing-instance wake request: {error}"))?;
    pipe.write_all(b"\n")
        .map_err(|error| format!("Could not terminate existing-instance wake request: {error}"))?;
    pipe.flush()
        .map_err(|error| format!("Could not flush existing-instance wake request: {error}"))
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
