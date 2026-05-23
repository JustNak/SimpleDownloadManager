import assert from 'node:assert/strict';
import {
  DEFAULT_EXCLUDED_HOSTS,
  createDefaultExtensionSettings,
  defaultExtensionSettings,
  normalizeExtensionSettings,
} from '../src/shared/defaultExtensionSettings.ts';

assert.deepEqual(DEFAULT_EXCLUDED_HOSTS, []);
assert.deepEqual(defaultExtensionSettings.excludedHosts, []);
assert.equal(defaultExtensionSettings.authenticatedHandoffEnabled, true);
assert.equal(defaultExtensionSettings.protectedDownloadAuthScope, 'allowlist');
assert.deepEqual(defaultExtensionSettings.authenticatedHandoffHosts, ['gofile.io']);
assert.deepEqual(createDefaultExtensionSettings().excludedHosts, []);
assert.equal(createDefaultExtensionSettings().protectedDownloadAuthScope, 'allowlist');
assert.deepEqual(createDefaultExtensionSettings().authenticatedHandoffHosts, ['gofile.io']);

assert.deepEqual(
  normalizeExtensionSettings(undefined).excludedHosts,
  [],
  'fresh settings should not exclude Telegram Web because blob downloads are bridged by the extension',
);

assert.deepEqual(
  normalizeExtensionSettings({ excludedHosts: ['web.telegram.org'] }).excludedHosts,
  [],
  'old default-only Telegram exclusions should migrate away so Telegram blob downloads can be captured',
);

assert.deepEqual(
  normalizeExtensionSettings({
    excludedHosts: ['https://web.telegram.org/', 'WEB.TELEGRAM.ORG', 'https://example.com/path', 'https://*.Example.com/downloads'],
  }).excludedHosts,
  ['example.com', '*.example.com'],
  'excluded hosts should migrate the old Telegram default while preserving custom exclusions',
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
  'legacy authenticated handoff hosts should normalize URL input, wildcard patterns, and duplicates',
);

assert.deepEqual(
  normalizeExtensionSettings({
    authenticatedHandoffEnabled: true,
    authenticatedHandoffHosts: [],
  }),
  {
    ...defaultExtensionSettings,
    authenticatedHandoffEnabled: true,
    protectedDownloadAuthScope: 'legacy_global',
    authenticatedHandoffHosts: [],
  },
  'existing users with the legacy enabled flag should retain global protected-download behavior explicitly',
);

assert.deepEqual(
  normalizeExtensionSettings({
    authenticatedHandoffEnabled: true,
    protectedDownloadAuthScope: 'allowlist',
    authenticatedHandoffHosts: [' https://ChatGPT.com/backend-api ', 'CHATGPT.COM', 'https://*.Example.com/downloads'],
  }).authenticatedHandoffHosts,
  ['chatgpt.com', '*.example.com'],
  'allowlisted protected-download hosts should normalize URL input, wildcard patterns, and duplicates',
);

assert.deepEqual(
  normalizeExtensionSettings({
    authenticatedHandoffEnabled: true,
    protectedDownloadAuthScope: 'off',
    authenticatedHandoffHosts: ['chatgpt.com'],
  }),
  {
    ...defaultExtensionSettings,
    authenticatedHandoffEnabled: false,
    protectedDownloadAuthScope: 'off',
    authenticatedHandoffHosts: ['chatgpt.com'],
  },
  'explicit off scope should clear the compatibility enabled flag while preserving the configured host list',
);
