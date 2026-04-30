#![allow(unsafe_code)]

use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

#[cfg(windows)]
use windows_sys::Win32::UI::Shell::ShellExecuteW;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub const STARTUP_REGISTRY_VALUE_NAME: &str = "Simple Download Manager";
pub const AUTOSTART_ARG: &str = "--autostart";

const INSTALL_RESOURCE_DIR: &[&str] = &["resources", "install"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellLaunchRequest {
    OpenPath(PathBuf),
    RevealFile { explorer: String, arguments: String },
}

pub fn browse_directory() -> Result<Option<String>, String> {
    let selected = rfd::FileDialog::new().pick_folder();
    Ok(selected.map(|path| path.display().to_string()))
}

pub fn browse_torrent_file() -> Result<Option<String>, String> {
    let selected = rfd::FileDialog::new()
        .add_filter("Torrent or magnet", &["torrent", "magnet", "txt"])
        .pick_file();

    selected
        .as_deref()
        .map(torrent_import_value_from_path)
        .transpose()
}

pub fn save_diagnostics_report(report: String) -> Result<Option<String>, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_file_name("simple-download-manager-diagnostics.json")
        .save_file()
    else {
        return Ok(None);
    };

    std::fs::write(&path, report)
        .map_err(|error| format!("Could not write diagnostics report: {error}"))?;
    Ok(Some(path.display().to_string()))
}

#[cfg(windows)]
pub fn open_url(url: &str) -> Result<(), String> {
    shell_execute(OsStr::new("open"), OsStr::new(url), None)
}

#[cfg(not(windows))]
pub fn open_url(_url: &str) -> Result<(), String> {
    Err("Opening downloads in the browser is only supported on Windows in this build.".into())
}

#[cfg(windows)]
pub fn open_path(path: &Path) -> Result<(), String> {
    shell_execute(OsStr::new("open"), path.as_os_str(), None)
}

#[cfg(not(windows))]
pub fn open_path(_path: &Path) -> Result<(), String> {
    Err("Opening files is only supported on Windows in this build.".into())
}

#[cfg(windows)]
pub fn reveal_path(path: &Path) -> Result<(), String> {
    match reveal_launch_request_for_path(path) {
        ShellLaunchRequest::OpenPath(path) => open_path(&path),
        ShellLaunchRequest::RevealFile {
            explorer,
            arguments,
        } => shell_execute(
            OsStr::new("open"),
            OsStr::new(&explorer),
            Some(OsStr::new(&arguments)),
        ),
    }
}

#[cfg(not(windows))]
pub fn reveal_path(_path: &Path) -> Result<(), String> {
    Err("Revealing files is only supported on Windows in this build.".into())
}

pub fn reveal_launch_request_for_path(path: &Path) -> ShellLaunchRequest {
    if path.is_dir() {
        return ShellLaunchRequest::OpenPath(path.to_path_buf());
    }

    ShellLaunchRequest::RevealFile {
        explorer: "explorer.exe".into(),
        arguments: explorer_select_arguments(path),
    }
}

pub fn explorer_select_arguments(path: &Path) -> String {
    format!("/select,\"{}\"", path.display())
}

#[cfg(windows)]
pub fn open_install_docs() -> Result<(), String> {
    let docs_path = resolve_install_resource_path("install.md")?;
    open_path(&docs_path)
}

#[cfg(not(windows))]
pub fn open_install_docs() -> Result<(), String> {
    Err("Opening install docs is only supported on Windows in this build.".into())
}

#[cfg(windows)]
pub fn sync_autostart_setting(enabled: bool) -> Result<(), String> {
    let registry_key = startup_run_registry_key()?;
    if enabled {
        let command = autostart_command(AUTOSTART_ARG)?;
        registry_key
            .set_value(STARTUP_REGISTRY_VALUE_NAME, &command)
            .map_err(|error| format!("Could not enable startup registration: {error}"))?;
    } else {
        match registry_key.delete_value(STARTUP_REGISTRY_VALUE_NAME) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!("Could not disable startup registration: {error}"));
            }
        }
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn sync_autostart_setting(_enabled: bool) -> Result<(), String> {
    Err("Startup registration is only supported on Windows in this build.".into())
}

pub fn autostart_command_for_executable(executable: &Path, autostart_arg: &str) -> String {
    format!("\"{}\" {autostart_arg}", executable.display())
}

#[cfg(windows)]
fn autostart_command(autostart_arg: &str) -> Result<String, String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("Could not locate app binary: {error}"))?;
    Ok(autostart_command_for_executable(&executable, autostart_arg))
}

#[cfg(windows)]
fn startup_run_registry_key() -> Result<RegKey, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .map_err(|error| format!("Could not open startup registry key: {error}"))?;
    Ok(key)
}

fn torrent_import_value_from_path(path: &Path) -> Result<String, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "torrent" => Ok(path.display().to_string()),
        "magnet" | "txt" => {
            let content = std::fs::read_to_string(path)
                .map_err(|error| format!("Could not read torrent import file: {error}"))?;
            torrent_import_value_from_text(&content)
        }
        _ => Err("Choose a .torrent file or a text file containing a magnet link.".into()),
    }
}

fn torrent_import_value_from_text(content: &str) -> Result<String, String> {
    let value = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| "The selected import file is empty.".to_string())?;

    if value.starts_with("magnet:?")
        || (value.starts_with("https://") || value.starts_with("http://"))
            && value.to_ascii_lowercase().contains(".torrent")
    {
        Ok(value.to_string())
    } else {
        Err("The selected import file must contain a magnet link or HTTP(S) .torrent URL.".into())
    }
}

#[cfg(windows)]
fn current_install_root() -> Result<PathBuf, String> {
    let current_exe =
        std::env::current_exe().map_err(|error| format!("Could not locate app binary: {error}"))?;
    current_exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Could not resolve app install directory.".to_string())
}

#[cfg(windows)]
fn resolve_install_resource_path(file_name: &str) -> Result<PathBuf, String> {
    let install_root = current_install_root()?;
    let bundled_candidate = INSTALL_RESOURCE_DIR
        .iter()
        .fold(install_root.clone(), |path, segment| path.join(segment))
        .join(file_name);
    if bundled_candidate.exists() {
        return Ok(bundled_candidate);
    }

    for ancestor in install_root.ancestors() {
        for relative_root in [
            ["src-tauri", "resources", "install"].as_slice(),
            ["apps", "desktop", "src-tauri", "resources", "install"].as_slice(),
            ["docs"].as_slice(),
        ] {
            let candidate = relative_root
                .iter()
                .fold(ancestor.to_path_buf(), |path, segment| path.join(segment))
                .join(file_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Err(format!(
        "Could not find bundled install resource: {file_name}."
    ))
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
    use std::os::windows::ffi::OsStrExt;

    value.encode_wide().chain(std::iter::once(0)).collect()
}
