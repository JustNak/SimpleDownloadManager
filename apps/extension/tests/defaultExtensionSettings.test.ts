import assert from 'node:assert/strict';
import {
  DEFAULT_EXCLUDED_HOSTS,
  createDefaultExtensionSettings,
  defaultExtensionSettings,
  normalizeExtensionSettings,
} from '../src/shared/defaultExtensionSettings.ts';

assert.deepEqual(DEFAULT_EXCLUDED_HOSTS, ['web.telegram.org']);
assert.deepEqual(defaultExtensionSettings.excludedHosts, ['web.telegram.org']);
assert.equal(defaultExtensionSettings.authenticatedHandoffEnabled, false);
assert.deepEqual(defaultExtensionSettings.authenticatedHandoffHosts, []);
assert.deepEqual(createDefaultExtensionSettings().excludedHosts, ['web.telegram.org']);

assert.deepEqual(
  normalizeExtensionSettings(undefined).excludedHosts,
  ['web.telegram.org'],
  'fresh settings should include Telegram Web in excluded hosts',
);

assert.deepEqual(
  normalizeExtensionSettings({
    excludedHosts: ['https://web.telegram.org/', 'WEB.TELEGRAM.ORG', 'https://example.com/path', 'https://*.Example.com/downloads'],
  }).excludedHosts,
  ['web.telegram.org', 'example.com', '*.example.com'],
  'excluded hosts should normalize URLs, wildcard host patterns, and deduplicate hosts',
);

assert.deepEqual(
  normalizeExtensionSettings({ excludedHosts: [] }).excludedHosts,
  [],
  'existing saved settings with an explicit empty list should not be migrated',
);

assert.deepEqual(
  normalizeExtensionSettings({
    authenticatedHandoffEnabled: true,
    authenticatedHandoffHosts: [' https://ChatGPT.com/backend-api ', 'CHATGPT.COM', 'https://*.Example.com/downloads'],
  }).authenticatedHandoffHosts,
  ['chatgpt.com', '*.example.com'],
  'authenticated handoff hosts should normalize URL input, wildcard patterns, and duplicates',
);
