import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const config = JSON.parse(
  readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
) as {
  app?: {
    windows?: unknown[];
  };
};
const lifecycleSource = readFileSync(
  new URL('../src-tauri/src/lifecycle.rs', import.meta.url),
  'utf8',
);

assert.ok(
  (config.app?.windows?.length ?? 0) === 0,
  'Tauri config should not eagerly create the main WebView at startup',
);
assert.ok(
  /MAIN_WINDOW_MIN_WIDTH:\s*f64\s*=\s*1360\.0/.test(lifecycleSource),
  'lazy main window min width should keep the Actions menu reachable at minimum scale',
);
assert.ok(
  /MAIN_WINDOW_WIDTH:\s*f64\s*=\s*1360\.0/.test(lifecycleSource),
  'lazy main window default width should not be smaller than its configured minimum width',
);
assert.ok(
  /MAIN_WINDOW_MIN_HEIGHT:\s*f64\s*=\s*720\.0/.test(lifecycleSource),
  'lazy main window minHeight should preserve enough vertical context at minimum scale',
);
