import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const config = JSON.parse(
  readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
) as {
  app?: {
    windows?: Array<{
      width?: number;
      minWidth?: number;
      minHeight?: number;
    }>;
  };
};

const mainWindow = config.app?.windows?.[0];

assert.ok(mainWindow, 'Tauri config should define the main application window');
assert.ok(
  (mainWindow.minWidth ?? 0) >= 1360,
  'main window minWidth should keep the Actions menu reachable at minimum scale',
);
assert.ok(
  (mainWindow.width ?? 0) >= (mainWindow.minWidth ?? 0),
  'main window default width should not be smaller than its configured minimum width',
);
assert.ok(
  (mainWindow.minHeight ?? 0) >= 720,
  'main window minHeight should preserve enough vertical context at minimum scale',
);
