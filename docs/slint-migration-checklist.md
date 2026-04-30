# Tauri-to-Slint Parity Phase Tracker

This is the source of truth for tracking parity between the current Tauri desktop app and the future native Slint desktop app on `migration/Slint-SimpleDownloadManager`.

Progress values are gate-based estimates, not a count of checkboxes. Update a phase percentage only when its acceptance criteria are demonstrably closer to passing.

| Phase | Status | Completion | Gate |
| --- | --- | ---: | --- |
| Phase 0: Baseline And Migration Spine | In Progress | 95% | Migration crates, scripts, tracker, and current tests are green. |
| Phase 1: Core Backend Extraction | In Progress | 50% | `desktop-core` owns state, settings, diagnostics, and queue mutation logic. |
| Phase 2: Transfer Engines And IPC | Not Started | 5% | HTTP, torrent, and native-host handoff run through `desktop-core`. |
| Phase 3: Slint Runtime Shell | In Progress | 5% | Slint app loads real state, receives events, and invokes backend commands. |
| Phase 4: Slint UI Feature Parity | Not Started | 5% | Every current React/Tauri workflow has a Slint equivalent. |
| Phase 5: Packaging And Updater Transition | Not Started | 5% | Signed Slint installer and updater transition are smoke-tested. |
| Phase 6: Cutover And Tauri Removal | Blocked | 0% | Slint is the only desktop product and Tauri is removed. |

## Phase 0: Baseline And Migration Spine

Status: **In Progress, 95%**

Tasks:
- [x] Create `migration/Slint-SimpleDownloadManager`.
- [x] Add `apps/desktop-core`.
- [x] Add `apps/desktop-slint`.
- [x] Keep Tauri app compiling while Slint work proceeds in parallel.
- [x] Add core/Slint test and clippy script coverage.
- [x] Replace the rough checklist with this phase tracker.
- [ ] Keep this tracker updated after each phase-changing migration slice.

Acceptance:
- [ ] `npm run test:rust` passes.
- [ ] `npm run clippy` passes.
- [ ] `npm run build:desktop:slint` passes.
- [ ] Existing Tauri app remains the shippable desktop app until cutover.

## Phase 1: Core Backend Extraction

Status: **In Progress, 50%**

Tasks:
- [x] Move storage and prompt contracts behind `desktop-core`.
- [x] Define `DesktopBackend`, `DesktopEvent`, and `ShellServices`.
- [x] Move `SharedState` and all state submodules into `desktop-core`.
- [ ] Move diagnostics and host-registration diagnostics into `desktop-core`.
- [ ] Convert Tauri commands into thin adapter calls against `DesktopBackend`.
- [ ] Add adapter tests proving Tauri command inputs map to core requests.

Acceptance:
- [x] `apps/desktop-core` owns storage, prompts, state, settings, diagnostics, and queue mutation logic.
- [ ] `apps/desktop/src-tauri/src/commands` mostly contains Tauri-specific argument/event glue.
- [x] `desktop-core` has no Tauri dependency.

## Phase 2: Transfer Engines And IPC

Status: **Not Started, 5%**

Tasks:
- [ ] Move HTTP scheduling and download engine into `desktop-core`.
- [ ] Move torrent engine orchestration into `desktop-core`.
- [ ] Move native pipe request validation and browser handoff handling into `desktop-core`.
- [ ] Replace direct Tauri notifications/events with `ShellServices` and `DesktopEvent`.
- [ ] Preserve retry, pause, resume, integrity, torrent seeding, external reseed, and duplicate handling semantics.

Acceptance:
- [ ] Core tests cover HTTP jobs without starting Tauri.
- [ ] Core tests cover torrent jobs without starting Tauri.
- [ ] Core tests cover prompt and IPC handoff without starting Tauri.
- [ ] Tauri still works through adapters.
- [ ] Slint can call the same backend surface Tauri uses.

## Phase 3: Slint Runtime Shell

Status: **In Progress, 5%**

Tasks:
- [x] Compile external `.slint` files with `slint-build`.
- [x] Add basic main window scaffold and controller conversion tests.
- [ ] Add background Tokio runtime and event bridge using `slint::invoke_from_event_loop`.
- [ ] Implement Slint `DesktopBackend` client/controller wiring.
- [ ] Implement native window lifecycle: main window, prompt window, progress windows, close-to-tray.
- [ ] Implement Windows shell services: single instance, tray, dialogs, notifications, open/reveal, startup registry.

Acceptance:
- [ ] Slint app loads persisted state.
- [ ] Slint app renders real jobs.
- [ ] Slint app reacts to backend events.
- [ ] Slint app invokes queue commands.
- [ ] Browser/native-host handoff wakes or focuses the Slint app.
- [ ] Window sizing and close/minimize behavior match current Tauri behavior.

## Phase 4: Slint UI Feature Parity

Status: **Not Started, 5%**

Tasks:
- [ ] Main queue with search, sorting, categories, torrent views, and selection.
- [ ] Command bar actions: pause, resume, cancel, retry, restart, remove, delete, rename, clear completed.
- [ ] Add-download and batch-add flows.
- [ ] Settings with draft/discard/save, torrent settings, extension settings, startup, theme, updates.
- [ ] Diagnostics report and host repair flow.
- [ ] Download prompt duplicate handling.
- [ ] HTTP, torrent, and batch progress windows.
- [ ] Toasts and shell error presentation.

Acceptance:
- [ ] Every current user workflow available in React/Tauri has a Slint equivalent.
- [ ] Ported controller tests cover sorting, queue actions, settings draft sync, diagnostics, progress metrics, prompt behavior, and updates.

## Phase 5: Packaging And Updater Transition

Status: **Not Started, 5%**

Tasks:
- [ ] Add `cargo-packager` NSIS configuration.
- [ ] Include native-host sidecar and install resources.
- [ ] Port installer postinstall/uninstall native-host registration hooks.
- [ ] Update release scripts to build extension, native host, Slint app, installer, signatures, and updater metadata.
- [ ] Preserve `latest-alpha.json` for the first Tauri-to-Slint update.
- [ ] Add `latest-alpha-slint.json` for Slint-native updates.
- [ ] Smoke-test install, uninstall, native-host registration, and updater transition.

Acceptance:
- [ ] A signed Slint installer can replace the Tauri installer.
- [ ] Existing installed Tauri alpha users can update into the Slint build.
- [ ] Native host remains registered for Chrome, Edge, and Firefox.

## Phase 6: Cutover And Tauri Removal

Status: **Blocked, 0%**

Tasks:
- [ ] Make root desktop build/release scripts point to Slint as the desktop product.
- [ ] Run full parity acceptance suite.
- [ ] Remove React/Vite/Tailwind desktop code after tests are ported.
- [ ] Remove Tauri config, capabilities, schemas, plugins, and Tauri-specific tests.
- [ ] Keep extension and native-host contracts unchanged.

Acceptance:
- [ ] Slint app is the only desktop product.
- [ ] Full test, clippy, build, installer, and updater smoke gates pass.
- [ ] No Tauri runtime dependency remains in the shipped desktop app.

## Public Interfaces To Track

- `DesktopBackend`: one method per former Tauri command.
- `DesktopEvent`: state changed, prompt changed, select job, update progress, shell errors.
- `ShellServices`: dialogs, notifications, open/reveal, tray, lifecycle, updater, windows.
- Stable external contracts remain unchanged: state path, named pipe, native host name, extension protocol v1, browser registration behavior.

## Recurring Verification Gates

- [ ] `npm run test:ts`
- [ ] `npm run typecheck`
- [ ] `npm run test:rust`
- [ ] `npm run clippy`
- [ ] `npm run build:desktop:slint`

Final parity gates:
- [ ] Slint release build.
- [ ] NSIS package build.
- [ ] Native-host end-to-end browser handoff.
- [ ] Single-instance wake.
- [ ] Tray open/exit.
- [ ] Startup registration.
- [ ] State migration from existing Tauri install.
- [ ] Tauri-to-Slint updater smoke test.

## Assumptions

- Windows-first remains the target.
- Behavioral parity matters more than pixel parity.
- Tauri stays compiling until Slint reaches functional parity.
- Existing dirty files are user-owned unless explicitly included in migration work.
