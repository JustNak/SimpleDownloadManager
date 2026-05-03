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

The full Windows release pipeline is:

```powershell
npm run release:windows
```

Signed updater artifacts require the Tauri updater private key. The release script reads it from the first available source:

- `TAURI_SIGNING_PRIVATE_KEY`
- `SDM_TAURI_SIGNING_PRIVATE_KEY_PATH`
- `%USERPROFILE%\.simple-download-manager\tauri-updater.key`

If the key has a password, set `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` or `SDM_TAURI_SIGNING_PRIVATE_KEY_PASSWORD_PATH`. Keep the private key outside the repository; only the public key belongs in app configuration.

That command will:

- build Chromium and Firefox extension bundles
- build the desktop frontend
- build the native host in release mode
- stage the native host as a Tauri sidecar
- bundle installer resources and native-host manifest templates
- build the Tauri NSIS installer
- zip the extension outputs into the top-level `release/` directory
- write `release/latest-beta.json` for the beta updater feed
- write `release/latest-alpha.json` as a one-time alpha-to-beta migration bridge

Publish the beta updater feed after a successful release build:

```powershell
npm run publish:updater-beta
```

For the first beta rollout, also publish the alpha bridge feed so installed alpha clients can discover the beta installer:

```powershell
npm run publish:updater-alpha-bridge
```

The bridge uploads only `latest-alpha.json` to the existing `updater-alpha` release. That metadata points at the beta installer asset under `updater-beta`, so alpha users upgrade in place and then follow `latest-beta.json` on future checks.
