import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const tracker = await readFile('docs/slint-migration-checklist.md', 'utf8');

assert.match(
  tracker,
  /^# Tauri-to-Slint Parity Phase Tracker/m,
  'migration tracker should use the phase tracker title',
);

for (const phase of [
  'Phase 0: Baseline And Migration Spine',
  'Phase 1: Core Backend Extraction',
  'Phase 2: Transfer Engines And IPC',
  'Phase 3: Slint Runtime Shell',
  'Phase 4: Slint UI Feature Parity',
  'Phase 5: Packaging And Updater Transition',
  'Phase 6: Slint Primary Cutover With Tauri Retained',
]) {
  assert.match(tracker, new RegExp(`^## ${phase}$`, 'm'), `${phase} should be tracked`);
}

assert.match(
  tracker,
  /\| Phase \| Status \| Completion \| Gate \|/,
  'migration tracker should include a compact progress table',
);

assert.match(
  tracker,
  /`DesktopBackend`[\s\S]*`DesktopEvent`[\s\S]*`ShellServices`/,
  'migration tracker should name the public interfaces that determine parity',
);

assert.match(
  tracker,
  /## Recurring Verification Gates/,
  'migration tracker should include recurring verification gates',
);

assert.match(
  tracker,
  /\| Phase 2: Transfer Engines And IPC \| Done \| 100% \|/,
  'Phase 2 should be consistently marked complete after transfer extraction',
);

assert.match(
  tracker,
  /## Phase 4: Slint UI Feature Parity[\s\S]*Status: \*\*Done, 100%\*\*/,
  'Phase 4 section should match the complete table status',
);

assert.match(
  tracker,
  /\| Phase 6: Slint Primary Cutover With Tauri Retained \| Done \| 100% \|/,
  'Phase 6 should be complete after the passed Slint primary runtime smoke evidence',
);

assert.match(
  tracker,
  /## Phase 6: Slint Primary Cutover With Tauri Retained[\s\S]*Status: \*\*Done, 100%\*\*/,
  'Phase 6 section should match the complete table status',
);

assert.match(
  tracker,
  /`npm run build:desktop` and `npm run release:windows` to target Slint by default[\s\S]*`npm run build:desktop:tauri` and `npm run release:windows:tauri`/,
  'Phase 6 should document Slint defaults and explicit retained Tauri commands',
);

assert.match(
  tracker,
  /updater publishing remains on the legacy Tauri default/i,
  'Phase 6 should record that updater publishing default is intentionally not cut over yet',
);

assert.match(
  tracker,
  /Phase 6B added `scripts\/smoke-phase6-slint\.ps1` and `scripts\/slint-phase6-smoke-report\.mjs`/,
  'Phase 6 should record the Slint runtime acceptance harness',
);
assert.match(
  tracker,
  /Phase 6C ran strict full runtime smoke[\s\S]*Tray open\/exit behavior was not confirmed/,
  'Phase 6 should record the strict runtime completion gate evidence and remaining tray blocker',
);

assert.match(
  tracker,
  /Phase 6D recorded the manual tray confirmation[\s\S]*slint-phase6-smoke-2026-05-02T04-09-06\.6553272Z\.json/,
  'Phase 6 should record the passed strict runtime smoke report after tray confirmation',
);

for (const finalGate of [
  'Native-host end-to-end browser handoff',
  'Single-instance wake',
  'Tray open/exit',
  'Startup registration',
  'State migration from existing Tauri install',
]) {
  assert.match(
    tracker,
    new RegExp(`- \\[x\\] ${finalGate.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`),
    `final parity gate should be checked after Phase 6D: ${finalGate}`,
  );
}

assert.match(
  tracker,
  /legacy\/reference desktop app/i,
  'Phase 6 should explicitly keep Tauri as a legacy/reference desktop app',
);

assert.match(
  tracker,
  /legacy Tauri remains buildable/i,
  'Phase 6 acceptance should keep legacy Tauri buildable',
);

for (const forbiddenRemovalPlan of [
  /Cutover And Tauri Removal/,
  /Tauri is removed/i,
  /Remove React\/Vite\/Tailwind/i,
  /Remove Tauri config/i,
  /Tauri-specific tests/i,
]) {
  assert.doesNotMatch(
    tracker,
    forbiddenRemovalPlan,
    `tracker should not plan Tauri deletion: ${forbiddenRemovalPlan}`,
  );
}
