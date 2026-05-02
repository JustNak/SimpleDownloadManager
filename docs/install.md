# Install Notes

## Browser Scope

- Chrome MV3
- Edge MV3
- Firefox WebExtension

## Native Host Registration

Register the same host name for each browser:

- `com.myapp.download_manager`

Locked extension IDs:

- Chrome/Edge: `pkaojpfpjieklhinoibjibmjldohlmbb`
- Firefox: `simple-download-manager@example.com`

Windows registry keys:

- `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.myapp.download_manager`
- `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.myapp.download_manager`
- `HKCU\Software\Mozilla\NativeMessagingHosts\com.myapp.download_manager`

Installer responsibilities:

- install `simple-download-manager.exe`
- install `simple-download-manager-native-host.exe`
- write browser-specific native host manifests
- register uninstall entry
- preserve app ID, host name, and install path conventions across upgrades

Recommended host manifest location strategy:

- install manifests beside the host binary
- point each registry key at the matching manifest file

## Registration Helpers

This repo now includes helper scripts:

- `scripts/register-native-host.ps1`
- `scripts/unregister-native-host.ps1`
- `scripts/prepare-release.mjs`
- `scripts/build-release.ps1`

Example registration command:

```powershell
.\scripts\register-native-host.ps1 `
  -HostBinaryPath "C:\Program Files\Simple Download Manager\simple-download-manager-native-host.exe"
```

Notes:

- Firefox defaults to `simple-download-manager@example.com`.
- Chrome and Edge default to `pkaojpfpjieklhinoibjibmjldohlmbb`.
- The Tauri NSIS installer now runs registration on install and unregisters on uninstall.

## Release Build

The primary Windows release pipeline now builds the Slint desktop app:

```powershell
npm run release:windows
```

Equivalent explicit commands:

```powershell
npm run release:windows:slint
npm run release:windows:tauri
```

Use `release:windows:slint` for the primary native desktop product. Use `release:windows:tauri` only for the retained legacy/reference Tauri app.

Signed updater artifacts require the existing Tauri updater private key so installed alpha users keep update continuity. The release scripts read it from the first available source:

- `TAURI_SIGNING_PRIVATE_KEY`
- `SDM_TAURI_SIGNING_PRIVATE_KEY_PATH`
- `%USERPROFILE%\.simple-download-manager\tauri-updater.key`

If the key has a password, set `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` or `SDM_TAURI_SIGNING_PRIVATE_KEY_PASSWORD_PATH`. Keep the private key outside the repository; only the public key belongs in app configuration.

The primary Slint release command will:

- build Chromium and Firefox extension bundles
- build the Slint desktop binary in release mode
- build the native host in release mode
- stage isolated Slint installer resources and native-host manifest templates
- build the Slint NSIS installer with `cargo-packager`
- sign the Slint installer for updater feeds with the existing Tauri signer
- write `release/slint/latest-alpha.json` and `release/slint/latest-alpha-slint.json`
- zip the extension outputs into `release/slint/`

Legacy Tauri remains buildable for reference:

```powershell
npm run build:desktop:tauri
npm run release:windows:tauri
```
