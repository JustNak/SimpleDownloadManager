use crate::storage::{HostRegistrationDiagnostics, HostRegistrationEntry, HostRegistrationStatus};

#[cfg(windows)]
use serde_json::Value;

#[cfg(windows)]
use std::path::{Path, PathBuf};

#[cfg(windows)]
use winreg::enums::HKEY_CURRENT_USER;

#[cfg(windows)]
use winreg::RegKey;

#[cfg(windows)]
const NATIVE_HOST_REGISTRY_KEYS: [&str; 3] = [
    r"Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager",
    r"Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager",
    r"Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager",
];

#[cfg(not(windows))]
pub fn gather_host_registration_diagnostics() -> Result<HostRegistrationDiagnostics, String> {
    Ok(HostRegistrationDiagnostics {
        status: HostRegistrationStatus::Missing,
        entries: Vec::new(),
    })
}

#[cfg(windows)]
pub fn gather_host_registration_diagnostics() -> Result<HostRegistrationDiagnostics, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut entries = Vec::new();

    for (browser, registry_path) in [
        ("Chrome", NATIVE_HOST_REGISTRY_KEYS[0]),
        ("Edge", NATIVE_HOST_REGISTRY_KEYS[1]),
        ("Firefox", NATIVE_HOST_REGISTRY_KEYS[2]),
    ] {
        let key = match hkcu.open_subkey(registry_path) {
            Ok(key) => key,
            Err(_) => {
                entries.push(HostRegistrationEntry {
                    browser: browser.into(),
                    registry_path: registry_path.into(),
                    manifest_path: None,
                    manifest_exists: false,
                    host_binary_path: None,
                    host_binary_exists: false,
                });
                continue;
            }
        };

        let manifest_path: String = match key.get_value("") {
            Ok(value) => value,
            Err(_) => {
                entries.push(HostRegistrationEntry {
                    browser: browser.into(),
                    registry_path: registry_path.into(),
                    manifest_path: None,
                    manifest_exists: false,
                    host_binary_path: None,
                    host_binary_exists: false,
                });
                continue;
            }
        };

        entries.push(read_host_registration_entry(
            browser,
            registry_path,
            Path::new(&manifest_path),
        )?);
    }

    let status = if entries.iter().any(|entry| entry.host_binary_exists) {
        HostRegistrationStatus::Configured
    } else if entries.iter().any(|entry| entry.manifest_path.is_some()) {
        HostRegistrationStatus::Broken
    } else {
        HostRegistrationStatus::Missing
    };

    Ok(HostRegistrationDiagnostics { status, entries })
}

#[cfg(windows)]
fn read_host_registration_entry(
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

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    #[test]
    fn invalid_native_host_manifest_is_reported_as_broken_entry() {
        let manifest_path = std::env::temp_dir().join(format!(
            "simple-download-manager-invalid-manifest-{}.json",
            std::process::id()
        ));
        std::fs::write(&manifest_path, "{not valid json").expect("write invalid manifest");

        let entry =
            read_host_registration_entry("Chrome", "Software\\Test", &manifest_path).unwrap();

        assert!(entry.manifest_exists);
        assert!(!entry.host_binary_exists);
        assert!(entry.host_binary_path.is_none());

        let _ = std::fs::remove_file(manifest_path);
    }
}
