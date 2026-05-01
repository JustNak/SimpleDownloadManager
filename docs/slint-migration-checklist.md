# Tauri-to-Slint Parity Phase Tracker

This is the source of truth for tracking parity between the current Tauri desktop app and the future native Slint desktop app on `migration/Slint-SimpleDownloadManager`.

Progress values are gate-based estimates, not a count of checkboxes. Update a phase percentage only when its acceptance criteria are demonstrably closer to passing.

| Phase | Status | Completion | Gate |
| --- | --- | ---: | --- |
| Phase 0: Baseline And Migration Spine | In Progress | 95% | Migration crates, scripts, tracker, and current tests are green. |
| Phase 1: Core Backend Extraction | In Progress | 78% | `desktop-core` owns state, settings, diagnostics orchestration, and command backend behavior. |
| Phase 2: Transfer Engines And IPC | In Progress | 90% | Native-host protocol, handoff, HTTP/torrent transfer, and scheduler/worker handling live in `desktop-core`; Tauri remains the app shell. |
| Phase 3: Slint Runtime Shell | Done | 100% | Slint app loads real state, handles backend events, invokes basic queue commands, accepts native-host wake/focus requests, persists main-window geometry, delegates Windows shell effects, supports tray open/exit plus close-to-tray, owns basic prompt/progress popup lifecycle, handles notifications plus native-host registration repair, has a basic cargo-packager updater UI, and matches Tauri main-window startup/close/minimize behavior. |
| Phase 4: Slint UI Feature Parity | In Progress | 55% | Every current React/Tauri workflow has a Slint equivalent. |
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
- [x] Keep this tracker updated after each phase-changing migration slice.

Acceptance:
- [x] `npm run test:rust` passes.
- [x] `npm run clippy` passes.
- [x] `npm run build:desktop:slint` passes.
- [x] Existing Tauri app remains the shippable desktop app until cutover.

## Phase 1: Core Backend Extraction

Status: **In Progress, 78%**

Tasks:
- [x] Move storage and prompt contracts behind `desktop-core`.
- [x] Define `DesktopBackend`, `DesktopEvent`, and `ShellServices`.
- [x] Move `SharedState` and all state submodules into `desktop-core`.
- [x] Move diagnostics orchestration into `desktop-core`, with host-registration probing behind `ShellServices`.
- [x] Convert Tauri commands into thin adapter calls against `DesktopBackend`.
- [x] Add adapter tests proving Tauri command inputs map to core requests.

Acceptance:
- [x] `apps/desktop-core` owns storage, prompts, state, settings, diagnostics, and queue mutation logic.
- [x] `apps/desktop/src-tauri/src/commands` mostly contains Tauri-specific argument/event glue.
- [x] `desktop-core` has no Tauri dependency.

## Phase 2: Transfer Engines And IPC

Status: **In Progress, 90%**

Tasks:
- [x] Move HTTP transfer engine into `desktop-core`; keep Tauri worker dispatch as the runtime adapter for now.
- [x] Remove dormant Tauri-local HTTP implementation and keep HTTP cleanup delegated to `desktop-core::transfer`.
- [x] Move torrent engine orchestration into `desktop-core`.
- [x] Move native pipe request validation and browser handoff handling into `desktop-core`.
- [x] Replace direct Tauri transfer notifications/events with `ShellServices` and `DesktopEvent`.
- [x] Move transfer scheduler, worker finalization, failure notification, and external reseed retry scheduling into `desktop-core`.
- [x] Preserve retry, pause, resume, integrity, torrent seeding, external reseed, and duplicate handling semantics through the extraction.

Acceptance:
- [x] Core tests cover HTTP jobs without starting Tauri.
- [x] `desktop-core::transfer` is the only HTTP transfer implementation; Tauri dispatch delegates HTTP jobs to core.
- [x] Core tests cover torrent jobs without starting Tauri.
- [x] Core tests cover prompt and IPC handoff without starting Tauri.
- [x] Tauri still works through adapters.
- [x] Slint can call the same backend surface Tauri uses.

## Phase 3: Slint Runtime Shell

Status: **Done, 100%**

Tasks:
- [x] Compile external `.slint` files with `slint-build`.
- [x] Add basic main window scaffold and controller conversion tests.
- [x] Add background Tokio runtime and event bridge using `slint::invoke_from_event_loop`.
- [x] Implement initial Slint `DesktopBackend` client/controller wiring for snapshots and basic queue commands.
- [x] Implement Slint single-instance guard, duplicate-launch wake request, and native-host named-pipe transport.
- [x] Implement Slint main-window sizing, restore, focus/show, and close-time geometry persistence.
- [x] Implement Slint dialog, diagnostics export, open/reveal/install-doc, and startup registry shell services.
- [x] Implement Slint tray open/exit and close-to-tray lifecycle.
- [x] Implement prompt and progress window lifecycle.
- [x] Implement Slint notifications and native-host registration diagnostics/repair.
- [x] Implement updater UI.
- [x] Implement Slint frameless main-window titlebar controls, startup visibility policy, focus/restore, and close/minimize lifecycle.

Acceptance:
- [x] Slint app loads persisted state.
- [x] Slint app renders real jobs.
- [x] Slint app reacts to backend events.
- [x] Slint app invokes basic queue commands.
- [x] Browser/native-host handoff wakes or focuses the Slint app.
- [x] Slint shell services cover folder/torrent dialogs, diagnostics export, open/reveal/install docs, and startup registry sync.
- [x] Tray open/exit and close-to-tray lifecycle are wired in Slint.
- [x] Prompt and progress popup lifecycle is wired in Slint with fixed Tauri-compatible sizing.
- [x] Slint shell services cover native notifications and native-host registration diagnostics/repair.
- [x] Slint update checks, install delegation, and install-progress UI updates are wired through `cargo-packager-updater`.
- [x] Window sizing and close/minimize behavior match current Tauri behavior.

Note: Slint keeps a hidden `MainWindow` instance for tray mode instead of destroying and recreating a Tauri webview. This is accepted as functional parity for Phase 3 because the user-visible startup, restore, minimize, and close-to-tray behavior matches the current Tauri shell.

## Phase 4: Slint UI Feature Parity

Status: **In Progress, 55%**

Tasks:
- [x] Main queue with search, sorting, categories, torrent views, and selection.
- [x] Command bar actions: pause, resume, cancel, retry, restart, remove, delete, rename, clear completed.
- [x] Add-download and batch-add flows.
- [ ] Settings with draft/discard/save, torrent settings, extension settings, startup, theme, updates.
- [ ] Diagnostics report and host repair flow.
- [ ] Download prompt duplicate handling.
- [ ] HTTP, torrent, and batch progress windows.
- [ ] Toasts and shell error presentation.

Note: Phase 4A added the Slint queue view-model, sidebar counts, search, sort toggles, category/torrent views, selection state, basic aggregate queue commands, and controller/runtime tests. Phase 4B added delete confirmation, rename, open/reveal, progress popup, browser swap, and React-matching row action enablement. Phase 4C added Slint add-download flows for single HTTP downloads, torrents, multi-download batches, and bulk archive batches. Settings, diagnostics, prompt duplicate UX, progress-window polish, and toasts remain for later Phase 4 slices.

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
- `HostRequest`/`HostResponse`: native-host protocol v1 request parsing, validation, and response mapping.
- `DesktopEvent`: state changed, prompt changed, select job, update progress, shell errors.
- `ShellServices`: dialogs, notifications, open/reveal, tray, lifecycle, updater, windows.
- Stable external contracts remain unchanged: state path, named pipe, native host name, extension protocol v1, browser registration behavior.

## Recurring Verification Gates

- [x] `npm run test:ts`
- [x] `npm run typecheck`
- [x] `npm run test:rust`
- [x] `npm run clippy`
- [x] `npm run build:desktop:slint`

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
