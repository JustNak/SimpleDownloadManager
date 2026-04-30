# Slint Migration Checklist

This branch tracks the Windows-first migration from the current Tauri desktop app to a native Slint desktop application.

## Stable Contracts

- [ ] Keep persisted state at `%LOCALAPPDATA%\SimpleDownloadManager\state.json`.
- [ ] Keep the named pipe at `\\.\pipe\myapp.downloads.v1`.
- [ ] Keep native host name `com.myapp.download_manager`.
- [ ] Keep extension protocol version `1`.
- [ ] Keep Chrome, Edge, and Firefox native-host registration behavior.
- [ ] Keep queue, prompt, retry, integrity, torrent, and external-reseed semantics.
- [ ] Preserve the first Tauri-to-Slint signed updater transition through `latest-alpha.json`.
- [ ] Add the Slint updater feed as `latest-alpha-slint.json`.

## Current Branch Status

- [x] Created `apps/desktop-core` as the framework-neutral Rust crate.
- [x] Moved the storage and prompt contracts behind the new core crate boundary.
- [x] Re-exported extracted storage and prompt modules from the Tauri crate to keep existing paths compiling.
- [x] Added core contracts for backend calls, shell services, desktop events, updater progress, and shell errors.
- [x] Created `apps/desktop-slint` with external `.slint` UI compiled by `slint-build`.
- [x] Added Slint controller tests for job-row conversion and snapshot status text.
- [x] Added Slint shell tests for existing popup window sizing policy.
- [x] Added Slint update tests for transition and Slint feed names.
- [x] Added root scripts for Slint build/test/clippy coverage.

## Core Extraction

- [ ] Move state modules from `apps/desktop/src-tauri/src/state` into `apps/desktop-core`.
- [ ] Move download scheduling and transfer logic into `apps/desktop-core` behind shell notification/dialog traits.
- [ ] Move torrent engine logic into `apps/desktop-core` behind filesystem and shell adapters where needed.
- [ ] Move IPC request validation and native-host registration diagnostics into `apps/desktop-core`.
- [ ] Keep Tauri `commands`, `windows`, `lifecycle`, and `updates` as thin adapters until cutover.
- [ ] Add adapter tests that compare Tauri command payloads with core `DesktopBackend` requests.

## Slint UI Parity

- [ ] Main queue view with search, sorting, categories, and torrent views.
- [ ] Command bar actions for pause, resume, cancel, retry, restart, remove, delete, rename, and clear completed.
- [ ] Add-download and batch-add flows.
- [ ] Settings view with draft, discard, save, torrent settings, extension settings, appearance, startup, and update panels.
- [ ] Diagnostics report and host registration repair flow.
- [ ] Download prompt window with duplicate handling.
- [ ] HTTP progress, torrent progress, and batch progress windows.
- [ ] Toast lifecycle and shell error presentation.
- [ ] Selection/focus handling for browser handoff and progress windows.

## Native Windows Behavior

- [ ] Preserve single-instance mutex behavior.
- [ ] Preserve named-pipe wake/show-window behavior.
- [ ] Preserve tray open/exit and close-to-tray behavior.
- [ ] Preserve startup registry behavior and installer launch options.
- [ ] Preserve main window state persistence.
- [ ] Preserve passive update relaunch behavior.
- [ ] Preserve open/reveal behavior for files and folders.
- [ ] Preserve native notifications.
- [ ] Preserve folder and torrent-file dialogs.
- [ ] Preserve fixed prompt/progress window sizing.

## Packaging And Release

- [ ] Add `cargo-packager` NSIS configuration for `apps/desktop-slint`.
- [ ] Port `windows/hooks.nsh` behavior for native-host registration and uninstall cleanup.
- [ ] Copy native-host sidecar and install resources into Slint packaging resources.
- [ ] Update release scripts to build extension, native host, Slint app, installer, signatures, and updater metadata.
- [ ] Generate both transition and Slint updater feed metadata.
- [ ] Smoke-test a Tauri-to-Slint update from the current alpha feed.

## Test Gates

- [ ] `npm run test:ts`
- [ ] `npm run typecheck`
- [ ] `cargo test --manifest-path apps/desktop-core/Cargo.toml`
- [ ] `cargo test --manifest-path apps/desktop-slint/Cargo.toml`
- [ ] `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`
- [ ] `cargo test --manifest-path apps/native-host/Cargo.toml`
- [ ] `cargo clippy --all-targets --manifest-path apps/desktop-core/Cargo.toml -- -D warnings`
- [ ] `cargo clippy --all-targets --manifest-path apps/desktop-slint/Cargo.toml -- -D warnings`
- [ ] `cargo clippy --all-targets --manifest-path apps/desktop/src-tauri/Cargo.toml -- -D warnings`
- [ ] `cargo clippy --all-targets --manifest-path apps/native-host/Cargo.toml -- -D warnings`
- [ ] `npm run build:desktop:slint`
- [ ] NSIS package build
- [ ] Local updater feed/install smoke test
