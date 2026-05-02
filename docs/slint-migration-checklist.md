# Tauri-to-Slint Parity Phase Tracker

This is the source of truth for tracking parity between the current Tauri desktop app and the future native Slint desktop app on `migration/Slint-SimpleDownloadManager`.

Progress values are gate-based estimates, not a count of checkboxes. Update a phase percentage only when its acceptance criteria are demonstrably closer to passing.

| Phase | Status | Completion | Gate |
| --- | --- | ---: | --- |
| Phase 0: Baseline And Migration Spine | In Progress | 95% | Migration crates, scripts, tracker, and current tests are green. |
| Phase 1: Core Backend Extraction | In Progress | 78% | `desktop-core` owns state, settings, diagnostics orchestration, and command backend behavior. |
| Phase 2: Transfer Engines And IPC | Done | 100% | Native-host protocol, handoff, HTTP/torrent transfer, and scheduler/worker handling live in `desktop-core`; Tauri remains the app shell. |
| Phase 3: Slint Runtime Shell | Done | 100% | Slint app loads real state, handles backend events, invokes basic queue commands, accepts native-host wake/focus requests, persists main-window geometry, delegates Windows shell effects, supports tray open/exit plus close-to-tray, owns basic prompt/progress popup lifecycle, handles notifications plus native-host registration repair, has a basic cargo-packager updater UI, and matches Tauri main-window startup/close/minimize behavior. |
| Phase 4: Slint UI Feature Parity | Done | 100% | Every current React/Tauri workflow has a Slint equivalent. |
| Phase 5: Packaging And Updater Transition | Done | 100% | Parallel Slint NSIS packaging path builds a signed Slint installer, verifies transition/native updater feeds, passes publish dry-run, installs/uninstalls into an isolated smoke root, and validates native-host registration cleanup. |
| Phase 6: Slint Primary Cutover With Tauri Retained | Done | 100% | Slint is now the primary local build/release target, full runtime acceptance evidence is recorded, and Tauri remains as a legacy/reference desktop app. |

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

Status: **Done, 100%**

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

Status: **Done, 100%**

Tasks:
- [x] Main queue with search, sorting, categories, torrent views, and selection.
- [x] Command bar actions: pause, resume, cancel, retry, restart, remove, delete, rename, clear completed.
- [x] Add-download and batch-add flows.
- [x] Settings with draft/discard/save, torrent settings, extension settings, startup, theme, updates.
- [x] Diagnostics report and host repair flow.
- [x] Download prompt duplicate handling.
- [x] HTTP, torrent, and batch progress windows.
- [x] Toasts and shell error presentation.

Note: Phase 4A added the Slint queue view-model, sidebar counts, search, sort toggles, category/torrent views, selection state, basic aggregate queue commands, and controller/runtime tests. Phase 4B added delete confirmation, rename, open/reveal, progress popup, browser swap, and React-matching row action enablement. Phase 4C added Slint add-download flows for single HTTP downloads, torrents, multi-download batches, and bulk archive batches. Phase 4D added the Slint settings view with clean/dirty draft adoption, discard confirmation, save wiring, directory browsing, torrent preferences, extension handoff preferences, startup/theme controls, excluded-site editing, torrent cache clear, and updater controls. Phase 4E added Slint native-host diagnostics UI, report copy/export, install docs, repair, test handoff, recent events, and Windows clipboard support. Phase 4F added Slint prompt duplicate actions, rename, browser swap, directory override, busy state, inline errors, and prompt action bridge tests. Phase 4G added enriched HTTP/torrent/batch progress details, React-matching progress metrics, torrent presentation helpers, batch archive phases, action buttons, cancel confirmation, and the progress popup action bridge. Phase 4H added Slint in-app toasts, 3000ms auto-close behavior, dismiss handling, shell-error routing, and workflow feedback for queue, add-download, settings, diagnostics, update, and external torrent use actions.

Acceptance:
- [x] Every current user workflow available in React/Tauri has a Slint equivalent.
- [x] Ported controller tests cover sorting, queue actions, settings draft sync, diagnostics, progress metrics, prompt behavior, and updates.

## Phase 5: Packaging And Updater Transition

Status: **Done, 100%**

Tasks:
- [x] Add parallel `cargo-packager` NSIS configuration for the Slint app.
- [x] Include native-host sidecar and install resources in the Slint packaging config.
- [x] Add a parallel Slint release script while preserving the existing Tauri release script.
- [x] Add isolated Slint release staging for install resources and the native-host sidecar.
- [x] Port installer postinstall/uninstall native-host registration hooks into the Slint NSIS template.
- [x] Add Slint release signing/preflight validation and cargo-packager signing env fallback.
- [x] Add Slint release artifact verification for installer, signature, staged resources, extension zips, and updater metadata.
- [x] Add Slint installer smoke harness for install/uninstall and native-host registration checks.
- [x] Generate a Tauri-compatible Slint transition `latest-alpha.json` alongside `latest-alpha-slint.json`.
- [x] Add explicit Slint updater publish dry-run validation for installer, signature, transition feed, and Slint-native feed.
- [x] Add Phase 5 Slint smoke orchestrator and report helper.
- [x] Smoke-test the Slint release script through installer/signature generation.
- [x] Preserve `latest-alpha.json` for the Tauri legacy release path.
- [x] Add `latest-alpha-slint.json` generation for Slint-native updates.
- [x] Smoke-test install, uninstall, native-host registration, and updater transition.

Note: Phase 5D added `scripts/smoke-release-slint.ps1` and `scripts/smoke-release-slint.mjs` so the Slint installer can be installed into an isolated temp directory, inspected for app/sidecar/resources/native-host manifests, checked against HKCU Chrome/Edge/Firefox native-host registry entries, uninstalled, and verified for registry cleanup.

Note: Phase 5E made the updater transition explicit: `scripts/updater-release.mjs --slint` now writes both `release/slint/latest-alpha.json` for existing Tauri alpha clients and `release/slint/latest-alpha-slint.json` for Slint-native clients. `scripts/publish-updater-alpha-slint.mjs` validates and uploads only Slint artifacts to the existing `updater-alpha` release, with a dry-run mode for local checks. Real updater transition smoke remains unchecked until a signed installer exists.

Note: Phase 5F added `scripts/smoke-phase5-slint.ps1` as the explicit orchestration entrypoint for check-only, build, publish dry-run, installer smoke, and full Slint smoke modes, plus `scripts/slint-phase5-smoke-report.mjs` for normalized passed/blocked/failed JSON reports under `release/slint/smoke/`.

Note: Phase 5G ran the signed Slint release smoke orchestrator in check-only mode on 2026-05-01. The latest report is `blocked`: `cargo-packager`, `makensis`, signing env, the Slint installer, installer signature, transition feed, and Slint-native feed are missing locally. Phase 5 remains at 89%, and the signed installer plus install/uninstall/updater smoke tasks remain unchecked until those prerequisites exist and `npm run smoke:phase5:slint:full` writes a `passed` report.

Note: Phase 5H installed `cargo-packager` and NSIS locally, then updated the Slint release/smoke scripts to detect standard NSIS install paths when `makensis.exe` is not on PATH yet. The latest check-only report is still `blocked` because signing env plus generated Slint installer/signature/feed artifacts are missing. Phase 5 remains at 89%, and full smoke must wait for `CARGO_PACKAGER_SIGN_PRIVATE_KEY` or `TAURI_SIGNING_PRIVATE_KEY`.

Note: Phase 5I reran the completion gate in check-only mode on 2026-05-01. The latest report remains `blocked`: signing env, the Slint installer, installer signature, transition feed, and Slint-native feed are missing. `cargo-packager`, NSIS, and GitHub CLI are no longer blockers, but full smoke was intentionally not run without signing material.

Note: Phase 5J verified the legacy Tauri updater key through the same local fallback path used by `scripts/build-release.ps1`. The Tauri signer accepts that key with the existing empty-password behavior, while cargo-packager's signer does not, so the Slint release path now packages with cargo-packager and creates updater signatures with the Tauri signer to preserve update-key continuity.

Note: Phase 5K ran `npm run smoke:phase5:slint:full` on 2026-05-02. The run built the Slint release, generated the NSIS installer and `.sig`, wrote `release/slint/latest-alpha.json` plus `release/slint/latest-alpha-slint.json`, passed Slint publish dry-run validation, installed into an isolated temp root, validated app/sidecar/install resources/native-host manifests, checked HKCU Chrome/Edge/Firefox native-host registration, uninstalled, verified registry cleanup, and wrote a `passed` smoke report at `release/slint/smoke/slint-phase5-smoke-2026-05-01T18-31-31-5867929Z.json`.

Acceptance:
- [x] A signed Slint installer can replace the Tauri installer.
- [x] Existing installed Tauri alpha users can update into the Slint build.
- [x] Native host remains registered for Chrome, Edge, and Firefox.

## Phase 6: Slint Primary Cutover With Tauri Retained

Status: **Done, 100%**

Tasks:
- [x] Make root desktop build/release defaults point to Slint as the primary desktop product after Phase 5 smoke tests pass.
- [x] Run full parity acceptance suite for the Slint primary product.
- [x] Keep `apps/desktop` and `apps/desktop/src-tauri` intact as the retained legacy/reference desktop app.
- [x] Keep legacy Tauri build and test commands available.
- [x] Document how to build and run Slint primary versus Tauri legacy.
- [x] Keep extension and native-host contracts unchanged.
- [x] Add Phase 6 Slint runtime acceptance smoke orchestrator and report helper.

Note: Phase 6A changed `npm run build:desktop` and `npm run release:windows` to target Slint by default after the passed Phase 5 smoke report. Tauri remains available through `npm run build:desktop:tauri` and `npm run release:windows:tauri`, and updater publishing remains on the legacy Tauri default until a separate publish cutover is planned.

Note: Phase 6B added `scripts/smoke-phase6-slint.ps1` and `scripts/slint-phase6-smoke-report.mjs` to record Slint primary runtime acceptance evidence for native-host handoff, duplicate-instance wake, startup command shape, state migration, cleanup, and tray/manual status. `npm run smoke:phase6:slint` runs check-only validation; `npm run smoke:phase6:slint:full` is the explicit full runtime smoke. Final Phase 6 acceptance remains open until a passed full report and tray evidence are recorded.

Note: Phase 6C ran strict full runtime smoke with `-StartupRegistrySmoke -RequireCompletionEvidence` on 2026-05-02. The run rebuilt the signed Slint installer, installed into an isolated temp root, verified native-host ping/enqueue handoff, duplicate-instance wake, state migration, startup registry enable/disable through the installed Slint smoke command, uninstall cleanup, and wrote a `blocked` report at `release/slint/smoke/slint-phase6-smoke-2026-05-02T03-46-33.9162855Z.json`. Tray open/exit behavior was not confirmed, so Phase 6 remains at 88% and final acceptance stays open.

Note: Phase 6D recorded the manual tray confirmation and reran strict full runtime smoke with `-StartupRegistrySmoke -TrayConfirmed -RequireCompletionEvidence` on 2026-05-02. The run rebuilt and signed the Slint installer, verified installer artifacts and both updater feeds, installed into an isolated temp root, verified native-host ping/enqueue handoff, duplicate-instance wake, state migration, startup registry enable/disable through the installed Slint smoke command, tray open/exit evidence, uninstall cleanup, and wrote a `passed` report at `release/slint/smoke/slint-phase6-smoke-2026-05-02T04-09-06.6553272Z.json`.

Acceptance:
- [x] Slint app is the primary shipped desktop product.
- [x] Full test, clippy, build, installer, and updater smoke gates pass for Slint while retained Tauri legacy checks remain available.
- [x] No Tauri runtime dependency remains in the shipped Slint desktop app; legacy Tauri remains buildable.

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
- [x] Slint release build.
- [x] NSIS package build.
- [x] Native-host end-to-end browser handoff.
- [x] Single-instance wake.
- [x] Tray open/exit.
- [x] Startup registration.
- [x] State migration from existing Tauri install.
- [x] Tauri-to-Slint updater smoke test.

## Assumptions

- Windows-first remains the target.
- Behavioral parity matters more than pixel parity.
- Tauri remains preserved as a retained legacy/reference desktop app after Slint becomes primary.
- Existing dirty files are user-owned unless explicitly included in migration work.
