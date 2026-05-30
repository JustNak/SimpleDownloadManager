<p align="center">
  <img src="apps/desktop/src-tauri/icons/icon.svg" width="96" height="96" alt="Simple Download Manager logo">
</p>

<h1 align="center">Simple Download Manager</h1>

Simple Download Manager is a local-first Windows download manager with browser handoff, torrent support, bulk downloads, and a native desktop queue.

**Latest beta:** [download from the beta release page](https://github.com/JustNak/SimpleDownloadManager/releases/tag/updater-beta)

It is built around a Tauri desktop app, a Rust download backend, a native messaging host, and WebExtension packages for browser integration. For the complete browser handoff experience, install both the desktop app and the matching browser WebExtension.

The project is currently beta software and is Windows-first; the installer, native-host registration, release pipeline, and updater artifacts target Windows x64 and Windows ARM64.

## Installation

Download the latest beta installer from the beta release page:

- [Latest beta release page](https://github.com/JustNak/SimpleDownloadManager/releases/tag/updater-beta)
- Windows x64 / Intel / AMD: choose the newest `Simple.Download.Manager_*_x64-setup.exe`
- Windows ARM64: choose the newest `Simple.Download.Manager_*_arm64-setup.exe`

Current direct installer links from the beta updater feed:

- [Windows x64 installer](https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-beta/Simple.Download.Manager_0.8.5-beta_x64-setup.exe)
- [Windows ARM64 installer](https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-beta/Simple.Download.Manager_0.8.5-beta_arm64-setup.exe)

Installer notes:

- The Windows installer includes the desktop app and native messaging host.
- Native-host registration is handled for Chrome, Edge, and Firefox during install.
- The browser WebExtension still needs to be installed or loaded for browser download capture.
- The app can check the beta updater feed for newer desktop builds after installation.

GitHub release asset filenames include the app version, so the beta release page is the most reliable always-current link. The direct links above are convenience links for the current beta build.

### Browser WebExtension

Browser integration needs the companion WebExtension in addition to the desktop installer.

- Firefox: install from [Firefox Add-ons](https://addons.mozilla.org/en-US/firefox/addon/sdm-simple-download-manager/).
- Chromium / Edge: download `simple-download-manager-chromium-extension.zip` from the [beta release assets](https://github.com/JustNak/SimpleDownloadManager/releases/tag/updater-beta) when published, then load it as an unpacked extension for local testing.

Chrome Web Store release is planned, but there is no public timeline yet.

## What It Does

- Queue normal HTTP(S) downloads from the desktop app.
- Capture eligible browser downloads through a companion extension.
- Prompt before handoff or send downloads directly, depending on extension settings.
- Preserve browser session request context for protected downloads without persisting secrets.
- Download magnet links and HTTP(S) `.torrent` files when torrenting is enabled.
- Manage bulk link batches, grouped bulk output, retries, and archive-oriented flows.
- Pause, resume, retry, cancel, and remove queue items.
- Track progress, speed, size, status, seeding activity, and diagnostics.
- Configure download folders, speed limits, concurrency, retries, notifications, themes, extension capture rules, and native-host diagnostics.

## Project Status

This is an active open project. Expect some rough edges, especially around installer polish, browser-store packaging, torrent edge cases, and cross-platform support.

Current practical support:

- Desktop app: Windows via Tauri.
- Browser extension: Chromium, Microsoft Edge, and Firefox WebExtension builds.
- Native messaging host: Windows registration helpers and installer integration.
- Releases: Windows NSIS installers and Tauri updater metadata.

## Repository Layout

```text
apps/desktop       Svelte 5 + Tauri desktop app and Rust backend
apps/extension     Browser extension for Chromium, Edge, and Firefox
apps/native-host   Rust native messaging bridge between browser and desktop
packages/protocol  Shared TypeScript protocol types
docs/              Installation, protocol, privacy, testing, and threat-model notes
scripts/           Test, release, native-host, packaging, and updater scripts
```

## Requirements

For development on Windows:

- Node.js and npm
- Rust stable toolchain
- Tauri v2 prerequisites for Windows
- Visual Studio C++ build tools
- PowerShell 7 for the release scripts

ARM64 release builds also require:

```powershell
rustup target add aarch64-pc-windows-msvc
```

## Getting Started

Install dependencies from the repository root:

```powershell
npm install
```

Run the desktop app in Tauri development mode:

```powershell
npm run tauri:dev --workspace @myapp/desktop
```

Build the desktop frontend bundle:

```powershell
npm run build:desktop
```

Build the browser extension:

```powershell
npm run build:extension
```

After building the extension, unpacked browser builds are created under `apps/extension/dist`.

## Common Commands

```powershell
npm run typecheck
npm run test:ts
npm run test:rust
npm test
npm run clippy
npm run check
```

Useful targeted test commands:

```powershell
npm run test:extension
npm run test:scenarios
npm run test:live:http
npm run test:live:torrent
```

The default test gates are designed to stay offline and deterministic. Live HTTP and torrent checks only run when the required environment variables are provided. See `docs/testing-scenario-matrix.md`.

## Browser Extension And Native Host

The browser extension talks to the local app through native messaging. It can:

- hand off links from the popup or context menu;
- intercept eligible browser downloads in `ask` or `auto` mode;
- preserve redirect-source URLs for protected browser-session downloads;
- forward bounded request headers to the desktop app for same-origin protected downloads;
- sync selected appearance and capture settings with the desktop app.

The native host name is:

```text
com.myapp.download_manager
```

Registration and installer details are documented in `docs/install.md`. The current installer flow registers the native host for Chrome, Edge, and Firefox on Windows.

## Privacy

Simple Download Manager is designed as a local app. The browser extension sends download metadata only to the local native desktop app installed on the same device. The project does not include analytics, advertising, remote configuration, or user tracking.

The extension may send download URLs, suggested filenames, MIME types, content lengths, redirect-source URLs, page metadata, handoff actions, and extension settings to the local app when a download is handed off. More detail is available in `docs/privacy-policy.md`.

## Security Notes

The app treats browser input, native-host messages, download URLs, and torrent metadata as untrusted. The desktop backend validates requests before queueing, limits native messaging and pipe payloads, bounds accepted browser headers, and keeps browser handoff authentication in memory only.

See `docs/threat-model.md` for the current threat model.

## Release Builds

The full Windows release pipeline is:

```powershell
npm run release:windows
```

Targeted release builds:

```powershell
npm run release:windows:x64
npm run release:windows:arm64
```

Release artifacts include Windows installers, native-host sidecars, extension packages, and updater feed metadata. Signed updater artifacts require a Tauri updater private key; see `docs/install.md` for the expected environment variables and key path conventions.

## Contributing

Contributions are welcome. Before changing browser handoff, queue state, torrent behavior, or shared protocol fields, read:

- `docs/protocol.md`
- `docs/browser-extension-download-capture.md`
- `docs/threat-model.md`
- `docs/testing-scenario-matrix.md`

Keep changes aligned across `packages/protocol`, `apps/extension`, `apps/native-host`, and `apps/desktop/src-tauri` when the browser-to-desktop protocol changes.

Recommended pre-submit check:

```powershell
npm run check
```

## Third-Party Components

The Windows bundle includes 7-Zip helper binaries. Their license is included at `apps/desktop/src-tauri/resources/bin/7zip-LICENSE.txt`.

Dependencies keep their own upstream licenses.

## License

Simple Download Manager is licensed under the GNU General Public License v3.0 or later. See `LICENSE`.

GPL licensing means people may use, study, modify, share, and even sell copies of the software, but distributed copies and derivatives must preserve the GPL terms and provide corresponding source code.
