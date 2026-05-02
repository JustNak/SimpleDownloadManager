import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const config = JSON.parse(
  readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
) as {
  app?: {
    windows?: unknown[];
  };
};
const lifecycleSource = readFileSync(new URL('../src-tauri/src/lifecycle.rs', import.meta.url), 'utf8');

assert.deepEqual(
  config.app?.windows ?? [],
  [],
  'main application window should be lazily created by lifecycle instead of tauri.conf.json',
);

assert.match(
  lifecycleSource,
  /pub const MAIN_WINDOW_MIN_WIDTH: f64 = 1360\.0;/,
  'main window minWidth should keep the Actions menu reachable at minimum scale',
);
assert.match(
  lifecycleSource,
  /pub const MAIN_WINDOW_WIDTH: f64 = 1360\.0;/,
  'main window default width should not be smaller than its configured minimum width',
);
assert.match(
  lifecycleSource,
  /pub const MAIN_WINDOW_MIN_HEIGHT: f64 = 720\.0;/,
  'main window minHeight should preserve enough vertical context at minimum scale',
);
