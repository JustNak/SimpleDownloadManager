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
  /\| Phase 6: Slint Primary Cutover With Tauri Retained \| In Progress \| 55% \|/,
  'Phase 6 should show the Slint primary cutover is underway after default build/release changes',
);

assert.match(
  tracker,
  /## Phase 6: Slint Primary Cutover With Tauri Retained[\s\S]*Status: \*\*In Progress, 55%\*\*/,
  'Phase 6 section should match the in-progress table status',
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
