import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const mainSource = await readFile(new URL('../src-tauri/src/main.rs', import.meta.url), 'utf8');

assert.match(
  mainSource,
  /let startup_args = std::env::args\(\)\.collect::<Vec<_>>\(\);/,
  'desktop startup should collect args once before installer and single-instance routing',
);

const installerProbeIndex = mainSource.indexOf('lifecycle::installer_launch_options_from_args(&startup_args).is_some()');
const installerApplyIndex = mainSource.indexOf('lifecycle::apply_installer_launch_options_from_args(&shared_state, &startup_args)');
const singleInstanceIndex = mainSource.indexOf('lifecycle::acquire_single_instance_or_notify()');

assert.notEqual(installerProbeIndex, -1, 'installer configuration should be detected before normal app startup');
assert.notEqual(installerApplyIndex, -1, 'installer configuration should still apply persisted startup settings');
assert.notEqual(singleInstanceIndex, -1, 'normal startup should still install the Windows single-instance guard');
assert(
  installerProbeIndex < installerApplyIndex && installerApplyIndex < singleInstanceIndex,
  'installer configuration should apply and return before normal single-instance startup',
);

const normalSharedStateIndex = mainSource.indexOf(
  'let shared_state = match state::SharedState::new()',
  singleInstanceIndex,
);
const appBuildIndex = mainSource.indexOf('let app = tauri::Builder::default()', singleInstanceIndex);

assert.notEqual(normalSharedStateIndex, -1, 'normal startup should load SharedState after single-instance arbitration');
assert.notEqual(appBuildIndex, -1, 'desktop app should still build after normal SharedState initialization');
assert(
  singleInstanceIndex < normalSharedStateIndex && normalSharedStateIndex < appBuildIndex,
  'normal launch should acquire the single-instance guard before loading persisted state or building Tauri',
);
