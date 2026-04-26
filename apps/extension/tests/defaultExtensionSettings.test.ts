import assert from 'node:assert/strict';
import {
  DEFAULT_EXCLUDED_HOSTS,
  createDefaultExtensionSettings,
  defaultExtensionSettings,
  normalizeExtensionSettings,
} from '../src/shared/defaultExtensionSettings.ts';

assert.deepEqual(DEFAULT_EXCLUDED_HOSTS, ['web.telegram.org']);
assert.deepEqual(defaultExtensionSettings.excludedHosts, ['web.telegram.org']);
assert.deepEqual(createDefaultExtensionSettings().excludedHosts, ['web.telegram.org']);

assert.deepEqual(
  normalizeExtensionSettings(undefined).excludedHosts,
  ['web.telegram.org'],
  'fresh settings should include Telegram Web in excluded hosts',
);

assert.deepEqual(
  normalizeExtensionSettings({
    excludedHosts: ['https://web.telegram.org/', 'WEB.TELEGRAM.ORG', 'https://example.com/path'],
  }).excludedHosts,
  ['web.telegram.org', 'example.com'],
  'excluded hosts should normalize URLs and deduplicate hosts',
);

assert.deepEqual(
  normalizeExtensionSettings({ excludedHosts: [] }).excludedHosts,
  [],
  'existing saved settings with an explicit empty list should not be migrated',
);
