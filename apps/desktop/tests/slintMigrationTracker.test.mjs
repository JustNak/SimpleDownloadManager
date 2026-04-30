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
  'Phase 6: Cutover And Tauri Removal',
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
