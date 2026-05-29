import assert from 'node:assert/strict';
import { DEFAULT_CAPTURED_FILE_EXTENSIONS } from '@myapp/protocol';
import {
  DEFAULT_EXCLUDED_HOSTS,
  createDefaultExtensionSettings,
  defaultExtensionSettings,
  normalizeExtensionSettings,
} from '../src/shared/defaultExtensionSettings.ts';

assert.deepEqual(DEFAULT_EXCLUDED_HOSTS, ['web.telegram.org']);
assert.deepEqual(defaultExtensionSettings.excludedHosts, ['web.telegram.org']);
assert.deepEqual(defaultExtensionSettings.capturedFileExtensions, [...DEFAULT_CAPTURED_FILE_EXTENSIONS]);
assert.equal(defaultExtensionSettings.downloadCaptureDebugLogging, false);
assert.deepEqual(createDefaultExtensionSettings().excludedHosts, ['web.telegram.org']);
assert.deepEqual(createDefaultExtensionSettings().capturedFileExtensions, [...DEFAULT_CAPTURED_FILE_EXTENSIONS]);
assert.equal(createDefaultExtensionSettings().downloadCaptureDebugLogging, false);

assert.deepEqual(
  normalizeExtensionSettings(undefined).excludedHosts,
  ['web.telegram.org'],
  'fresh settings should exclude Telegram Web by default because Telegram download handling is intentionally ignored',
);

assert.deepEqual(
  normalizeExtensionSettings({ excludedHosts: ['web.telegram.org'] }).excludedHosts,
  ['web.telegram.org'],
  'Telegram Web exclusions should be preserved during normalization',
);

assert.deepEqual(
  normalizeExtensionSettings({
    excludedHosts: ['https://web.telegram.org/', 'WEB.TELEGRAM.ORG', 'https://example.com/path', 'https://*.Example.com/downloads'],
  }).excludedHosts,
  ['web.telegram.org', 'example.com', '*.example.com'],
  'excluded hosts should preserve Telegram while normalizing custom exclusions',
);

assert.deepEqual(
  normalizeExtensionSettings({ excludedHosts: [] }).excludedHosts,
  [],
  'existing saved settings with an explicit empty list should not be migrated',
);

assert.deepEqual(
  normalizeExtensionSettings(undefined).capturedFileExtensions,
  [...DEFAULT_CAPTURED_FILE_EXTENSIONS],
  'fresh settings should list the default captured file extensions for user editing',
);

assert.deepEqual(
  normalizeExtensionSettings({ capturedFileExtensions: [] }).capturedFileExtensions,
  [],
  'existing saved settings with an explicit empty captured extension list should not be migrated',
);

assert.deepEqual(
  normalizeExtensionSettings({
    capturedFileExtensions: [' zip rar exe 7zip ppt pptx docx ', '.ZIP', 'invalid/path'],
  }).capturedFileExtensions,
  ['zip', 'rar', 'exe', '7z', 'ppt', 'pptx', 'docx'],
  'captured file extensions should normalize whitespace input, aliases, dots, and duplicates',
);
