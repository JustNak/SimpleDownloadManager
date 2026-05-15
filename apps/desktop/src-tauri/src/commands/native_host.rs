use crate::ipc::gather_host_registration_diagnostics;
use crate::storage::HostRegistrationStatus;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[cfg(windows)]
use serde_json::json;
#[cfg(windows)]
use winreg::enums::HKEY_CURRENT_USER;
#[cfg(windows)]
use winreg::RegKey;

const INSTALL_RESOURCE_DIR: &str = "resources\\install";
const NATIVE_HOST_NAME: &str = "com.myapp.download_manager";
const DEFAULT_CHROMIUM_EXTENSION_ID: &str = "pkaojpfpjieklhinoibjibmjldohlmbb";
const DEFAULT_FIREFOX_EXTENSION_ID: &str = "simple-download-manager@example.com";

#[cfg(windows)]
const CHROME_REGISTRY_PATH: &str =
    r"Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager";
#[cfg(windows)]
const EDGE_REGISTRY_PATH: &str =
    r"Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager";
#[cfg(windows)]
const FIREFOX_REGISTRY_PATH: &str =
    r"Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseMetadata {
    sidecar_binary_name: Option<String>,
    chromium_extension_id: Option<String>,
    edge_extension_id: Option<String>,
    firefox_extension_id: Option<String>,
}

impl Default for ReleaseMetadata {
    fn default() -> Self {
        Self {
            sidecar_binary_name: None,
            chromium_extension_id: Some(DEFAULT_CHROMIUM_EXTENSION_ID.into()),
            edge_extension_id: Some(DEFAULT_CHROMIUM_EXTENSION_ID.into()),
            firefox_extension_id: Some(DEFAULT_FIREFOX_EXTENSION_ID.into()),
        }
    }
}

fn current_install_root() -> Result<PathBuf, String> {
    let current_exe =
        std::env::current_exe().map_err(|error| format!("Could not locate app binary: {error}"))?;
    current_exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Could not resolve app install directory.".to_string())
}

pub(super) fn resolve_install_resource_path(file_name: &str) -> Result<PathBuf, String> {
    let install_root = current_install_root()?;
    let bundled_candidate = install_root.join(INSTALL_RESOURCE_DIR).join(file_name);
    if bundled_candidate.exists() {
        return Ok(bundled_candidate);
    }

    for ancestor in install_root.ancestors() {
        for relative_root in [
            "src-tauri\\resources\\install",
            "apps\\desktop\\src-tauri\\resources\\install",
        ] {
            let candidate = ancestor.join(relative_root).join(file_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Err(format!(
        "Could not find bundled install resource: {file_name}."
    ))
}

fn resolve_host_binary_path() -> Result<PathBuf, String> {
    let install_root = current_install_root()?;
    let mut candidate_names = Vec::new();

    let metadata = resolve_release_metadata();
    if let Some(sidecar_binary_name) = metadata.sidecar_binary_name {
        candidate_names.push(sidecar_binary_name);
    }

    candidate_names.push("simple-download-manager-native-host.exe".into());
    candidate_names.push("simple-download-manager-native-host-x86_64-pc-windows-msvc.exe".into());

    for candidate_name in candidate_names {
        let candidate_path = install_root.join(&candidate_name);
        if candidate_path.exists() {
            return Ok(candidate_path);
        }
    }

    Err("The bundled native host executable was not found beside the desktop app.".into())
}

#[cfg(windows)]
pub(super) fn ensure_native_host_registration() -> Result<(), String> {
    let diagnostics = gather_host_registration_diagnostics()?;
    if should_register_native_host(diagnostics.status) {
        register_native_host()?;
    }

    Ok(())
}

#[cfg(windows)]
pub(super) fn register_native_host() -> Result<(), String> {
    let install_root = current_install_root()?;
    let host_binary_path = resolve_host_binary_path()?;
    let manifest_root = install_root.join("native-messaging");
    let metadata = resolve_release_metadata();
    let chromium_extension_id = metadata
        .chromium_extension_id
        .as_deref()
        .unwrap_or(DEFAULT_CHROMIUM_EXTENSION_ID);
    let edge_extension_id = metadata
        .edge_extension_id
        .as_deref()
        .unwrap_or(chromium_extension_id);
    let firefox_extension_id = metadata
        .firefox_extension_id
        .as_deref()
        .unwrap_or(DEFAULT_FIREFOX_EXTENSION_ID);

    std::fs::create_dir_all(&manifest_root).map_err(|error| {
        format!("Could not create native messaging manifest directory: {error}")
    })?;

    let chrome_manifest_path = manifest_root.join(format!("{NATIVE_HOST_NAME}.chrome.json"));
    let edge_manifest_path = manifest_root.join(format!("{NATIVE_HOST_NAME}.edge.json"));
    let firefox_manifest_path = manifest_root.join(format!("{NATIVE_HOST_NAME}.firefox.json"));

    write_native_host_manifest(
        &chrome_manifest_path,
        native_host_manifest_json(
            &host_binary_path,
            "allowed_origins",
            json!([format!("chrome-extension://{chromium_extension_id}/")]),
        ),
    )?;
    write_native_host_manifest(
        &edge_manifest_path,
        native_host_manifest_json(
            &host_binary_path,
            "allowed_origins",
            json!([format!("chrome-extension://{edge_extension_id}/")]),
        ),
    )?;
    write_native_host_manifest(
        &firefox_manifest_path,
        native_host_manifest_json(
            &host_binary_path,
            "allowed_extensions",
            json!([firefox_extension_id]),
        ),
    )?;

    set_registry_default_value(CHROME_REGISTRY_PATH, &chrome_manifest_path)?;
    set_registry_default_value(EDGE_REGISTRY_PATH, &edge_manifest_path)?;
    set_registry_default_value(FIREFOX_REGISTRY_PATH, &firefox_manifest_path)?;

    Ok(())
}

#[cfg(not(windows))]
pub(super) fn register_native_host() -> Result<(), String> {
    Err("Native host registration is only supported on Windows in this build.".into())
}

fn resolve_release_metadata() -> ReleaseMetadata {
    resolve_install_resource_path("release.json")
        .ok()
        .and_then(|release_path| std::fs::read_to_string(release_path).ok())
        .and_then(|content| serde_json::from_str::<ReleaseMetadata>(&content).ok())
        .unwrap_or_default()
}

#[cfg(windows)]
pub(super) fn native_host_manifest_json(
    host_binary_path: &Path,
    browser_key: &str,
    browser_value: serde_json::Value,
) -> serde_json::Value {
    let mut manifest = json!({
        "name": NATIVE_HOST_NAME,
        "description": "Simple Download Manager native messaging host",
        "path": host_binary_path.display().to_string(),
        "type": "stdio",
    });

    if let Some(object) = manifest.as_object_mut() {
        object.insert(browser_key.into(), browser_value);
    }

    manifest
}

#[cfg(windows)]
fn write_native_host_manifest(path: &Path, manifest: serde_json::Value) -> Result<(), String> {
    let content = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("Could not serialize native host manifest: {error}"))?;

    std::fs::write(path, content)
        .map_err(|error| format!("Could not write native host manifest: {error}"))
}

#[cfg(windows)]
fn set_registry_default_value(registry_path: &str, manifest_path: &Path) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(registry_path)
        .map_err(|error| format!("Could not create HKCU\\{registry_path}: {error}"))?;

    key.set_value("", &manifest_path.display().to_string())
        .map_err(|error| format!("Could not write HKCU\\{registry_path}: {error}"))
}

pub(super) fn should_register_native_host(status: HostRegistrationStatus) -> bool {
    !matches!(status, HostRegistrationStatus::Configured)
}
