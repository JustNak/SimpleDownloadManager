use serde_json::{json, Value};
use simple_download_manager_desktop_core::storage::{
    HostRegistrationDiagnostics, HostRegistrationEntry, HostRegistrationStatus,
};
use std::path::{Path, PathBuf};

#[cfg(windows)]
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

pub const INSTALL_RESOURCE_DIR: &str = "resources\\install";
pub const NATIVE_HOST_NAME: &str = "com.myapp.download_manager";
pub const DEFAULT_CHROMIUM_EXTENSION_ID: &str = "pkaojpfpjieklhinoibjibmjldohlmbb";
pub const DEFAULT_FIREFOX_EXTENSION_ID: &str = "simple-download-manager@example.com";
pub const CHROME_REGISTRY_PATH: &str =
    r"Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager";
pub const EDGE_REGISTRY_PATH: &str =
    r"Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager";
pub const FIREFOX_REGISTRY_PATH: &str =
    r"Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager";
pub const CHROME_MANIFEST_FILE: &str = "com.myapp.download_manager.chrome.json";
pub const EDGE_MANIFEST_FILE: &str = "com.myapp.download_manager.edge.json";
pub const FIREFOX_MANIFEST_FILE: &str = "com.myapp.download_manager.firefox.json";
pub const DEFAULT_HOST_BINARY_NAMES: [&str; 2] = [
    "simple-download-manager-native-host.exe",
    "simple-download-manager-native-host-x86_64-pc-windows-msvc.exe",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseMetadata {
    pub sidecar_binary_name: Option<String>,
    pub chromium_extension_id: Option<String>,
    pub edge_extension_id: Option<String>,
    pub firefox_extension_id: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeHostRegistryTarget {
    pub browser: &'static str,
    pub registry_path: &'static str,
    pub manifest_file: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserExtensionIds {
    pub chromium: String,
    pub edge: String,
    pub firefox: String,
}

pub fn registry_targets() -> [NativeHostRegistryTarget; 3] {
    [
        NativeHostRegistryTarget {
            browser: "Chrome",
            registry_path: CHROME_REGISTRY_PATH,
            manifest_file: CHROME_MANIFEST_FILE,
        },
        NativeHostRegistryTarget {
            browser: "Edge",
            registry_path: EDGE_REGISTRY_PATH,
            manifest_file: EDGE_MANIFEST_FILE,
        },
        NativeHostRegistryTarget {
            browser: "Firefox",
            registry_path: FIREFOX_REGISTRY_PATH,
            manifest_file: FIREFOX_MANIFEST_FILE,
        },
    ]
}

pub fn manifest_filenames() -> [&'static str; 3] {
    [
        CHROME_MANIFEST_FILE,
        EDGE_MANIFEST_FILE,
        FIREFOX_MANIFEST_FILE,
    ]
}

pub fn host_binary_candidate_names(metadata: &ReleaseMetadata) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(sidecar_binary_name) = metadata.sidecar_binary_name.as_ref() {
        names.push(sidecar_binary_name.clone());
    }
    names.extend(DEFAULT_HOST_BINARY_NAMES.iter().map(|name| (*name).into()));
    names
}

pub fn browser_extension_ids(metadata: &ReleaseMetadata) -> BrowserExtensionIds {
    let chromium = metadata
        .chromium_extension_id
        .as_deref()
        .unwrap_or(DEFAULT_CHROMIUM_EXTENSION_ID)
        .to_string();
    let edge = metadata
        .edge_extension_id
        .as_deref()
        .unwrap_or(&chromium)
        .to_string();
    let firefox = metadata
        .firefox_extension_id
        .as_deref()
        .unwrap_or(DEFAULT_FIREFOX_EXTENSION_ID)
        .to_string();

    BrowserExtensionIds {
        chromium,
        edge,
        firefox,
    }
}

pub fn should_register_native_host(status: HostRegistrationStatus) -> bool {
    !matches!(status, HostRegistrationStatus::Configured)
}

pub fn classify_host_registration_entries(
    entries: &[HostRegistrationEntry],
) -> HostRegistrationStatus {
    if entries.iter().any(|entry| entry.host_binary_exists) {
        HostRegistrationStatus::Configured
    } else if entries.iter().any(|entry| entry.manifest_path.is_some()) {
        HostRegistrationStatus::Broken
    } else {
        HostRegistrationStatus::Missing
    }
}

pub fn native_host_manifest_json(
    host_binary_path: &Path,
    browser_key: &str,
    browser_value: Value,
) -> Value {
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

pub fn read_host_registration_entry(
    browser: &str,
    registry_path: &str,
    manifest_path: &Path,
) -> Result<HostRegistrationEntry, String> {
    if !manifest_path.exists() {
        return Ok(HostRegistrationEntry {
            browser: browser.into(),
            registry_path: registry_path.into(),
            manifest_path: Some(manifest_path.display().to_string()),
            manifest_exists: false,
            host_binary_path: None,
            host_binary_exists: false,
        });
    }

    let content = match std::fs::read_to_string(manifest_path) {
        Ok(content) => content,
        Err(error) => {
            eprintln!("could not read native host manifest for diagnostics: {error}");
            return Ok(broken_host_registration_entry(
                browser,
                registry_path,
                manifest_path,
            ));
        }
    };
    let manifest: Value = match serde_json::from_str(&content) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("could not parse native host manifest for diagnostics: {error}");
            return Ok(broken_host_registration_entry(
                browser,
                registry_path,
                manifest_path,
            ));
        }
    };
    let host_path = manifest
        .get("path")
        .and_then(|value| value.as_str())
        .map(PathBuf::from);
    let host_binary_exists = host_path
        .as_ref()
        .map(|value| value.exists())
        .unwrap_or(false);

    Ok(HostRegistrationEntry {
        browser: browser.into(),
        registry_path: registry_path.into(),
        manifest_path: Some(manifest_path.display().to_string()),
        manifest_exists: true,
        host_binary_path: host_path.as_ref().map(|value| value.display().to_string()),
        host_binary_exists,
    })
}

#[cfg(windows)]
pub fn gather_host_registration_diagnostics() -> Result<HostRegistrationDiagnostics, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut entries = Vec::new();

    for target in registry_targets() {
        let key = match hkcu.open_subkey(target.registry_path) {
            Ok(key) => key,
            Err(_) => {
                entries.push(missing_host_registration_entry(target));
                continue;
            }
        };

        let manifest_path: String = match key.get_value("") {
            Ok(value) => value,
            Err(_) => {
                entries.push(missing_host_registration_entry(target));
                continue;
            }
        };

        entries.push(read_host_registration_entry(
            target.browser,
            target.registry_path,
            Path::new(&manifest_path),
        )?);
    }

    Ok(HostRegistrationDiagnostics {
        status: classify_host_registration_entries(&entries),
        entries,
    })
}

#[cfg(not(windows))]
pub fn gather_host_registration_diagnostics() -> Result<HostRegistrationDiagnostics, String> {
    Ok(HostRegistrationDiagnostics {
        status: HostRegistrationStatus::Missing,
        entries: Vec::new(),
    })
}

#[cfg(windows)]
pub fn ensure_native_host_registration() -> Result<(), String> {
    let diagnostics = gather_host_registration_diagnostics()?;
    if should_register_native_host(diagnostics.status) {
        register_native_host()?;
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn ensure_native_host_registration() -> Result<(), String> {
    Err("Native host registration is only supported on Windows in this build.".into())
}

#[cfg(windows)]
pub fn register_native_host() -> Result<(), String> {
    let install_root = current_install_root()?;
    let host_binary_path = resolve_host_binary_path()?;
    let manifest_root = install_root.join("native-messaging");
    let metadata = resolve_release_metadata();
    let extension_ids = browser_extension_ids(&metadata);

    std::fs::create_dir_all(&manifest_root).map_err(|error| {
        format!("Could not create native messaging manifest directory: {error}")
    })?;

    let chrome_manifest_path = manifest_root.join(CHROME_MANIFEST_FILE);
    let edge_manifest_path = manifest_root.join(EDGE_MANIFEST_FILE);
    let firefox_manifest_path = manifest_root.join(FIREFOX_MANIFEST_FILE);

    write_native_host_manifest(
        &chrome_manifest_path,
        native_host_manifest_json(
            &host_binary_path,
            "allowed_origins",
            json!([format!("chrome-extension://{}/", extension_ids.chromium)]),
        ),
    )?;
    write_native_host_manifest(
        &edge_manifest_path,
        native_host_manifest_json(
            &host_binary_path,
            "allowed_origins",
            json!([format!("chrome-extension://{}/", extension_ids.edge)]),
        ),
    )?;
    write_native_host_manifest(
        &firefox_manifest_path,
        native_host_manifest_json(
            &host_binary_path,
            "allowed_extensions",
            json!([extension_ids.firefox]),
        ),
    )?;

    set_registry_default_value(CHROME_REGISTRY_PATH, &chrome_manifest_path)?;
    set_registry_default_value(EDGE_REGISTRY_PATH, &edge_manifest_path)?;
    set_registry_default_value(FIREFOX_REGISTRY_PATH, &firefox_manifest_path)?;

    Ok(())
}

#[cfg(not(windows))]
pub fn register_native_host() -> Result<(), String> {
    Err("Native host registration is only supported on Windows in this build.".into())
}

fn missing_host_registration_entry(target: NativeHostRegistryTarget) -> HostRegistrationEntry {
    HostRegistrationEntry {
        browser: target.browser.into(),
        registry_path: target.registry_path.into(),
        manifest_path: None,
        manifest_exists: false,
        host_binary_path: None,
        host_binary_exists: false,
    }
}

fn broken_host_registration_entry(
    browser: &str,
    registry_path: &str,
    manifest_path: &Path,
) -> HostRegistrationEntry {
    HostRegistrationEntry {
        browser: browser.into(),
        registry_path: registry_path.into(),
        manifest_path: Some(manifest_path.display().to_string()),
        manifest_exists: true,
        host_binary_path: None,
        host_binary_exists: false,
    }
}

fn release_metadata_from_value(value: Value) -> ReleaseMetadata {
    ReleaseMetadata {
        sidecar_binary_name: value
            .get("sidecarBinaryName")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        chromium_extension_id: value
            .get("chromiumExtensionId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        edge_extension_id: value
            .get("edgeExtensionId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        firefox_extension_id: value
            .get("firefoxExtensionId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    }
}

fn resolve_release_metadata() -> ReleaseMetadata {
    resolve_install_resource_path("release.json")
        .ok()
        .and_then(|release_path| std::fs::read_to_string(release_path).ok())
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .map(release_metadata_from_value)
        .unwrap_or_default()
}

fn current_install_root() -> Result<PathBuf, String> {
    let current_exe =
        std::env::current_exe().map_err(|error| format!("Could not locate app binary: {error}"))?;
    current_exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Could not resolve app install directory.".to_string())
}

fn resolve_install_resource_path(file_name: &str) -> Result<PathBuf, String> {
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

#[cfg(windows)]
fn resolve_host_binary_path() -> Result<PathBuf, String> {
    let install_root = current_install_root()?;
    let metadata = resolve_release_metadata();

    for candidate_name in host_binary_candidate_names(&metadata) {
        let candidate_path = install_root.join(&candidate_name);
        if candidate_path.exists() {
            return Ok(candidate_path);
        }
    }

    Err("The bundled native host executable was not found beside the desktop app.".into())
}

#[cfg(windows)]
fn write_native_host_manifest(path: &Path, manifest: Value) -> Result<(), String> {
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
