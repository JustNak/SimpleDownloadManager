import assert from 'node:assert/strict';
import {
  DEFAULT_AUTHENTICATED_HANDOFF_HOSTS,
  DEFAULT_EXCLUDED_HOSTS,
  createDefaultExtensionSettings,
  defaultExtensionSettings,
  normalizeExtensionSettings,
} from '../src/shared/defaultExtensionSettings.ts';

assert.deepEqual(DEFAULT_EXCLUDED_HOSTS, ['web.telegram.org']);
assert.deepEqual(DEFAULT_AUTHENTICATED_HANDOFF_HOSTS, []);
assert.deepEqual(defaultExtensionSettings.excludedHosts, ['web.telegram.org']);
assert.equal(defaultExtensionSettings.authenticatedHandoffEnabled, true);
assert.equal(defaultExtensionSettings.protectedDownloadAuthScope, 'legacy_global');
assert.deepEqual(defaultExtensionSettings.authenticatedHandoffHosts, []);
assert.deepEqual(defaultExtensionSettings.capturedFileExtensions, []);
assert.deepEqual(createDefaultExtensionSettings().excludedHosts, ['web.telegram.org']);
assert.equal(createDefaultExtensionSettings().protectedDownloadAuthScope, 'legacy_global');
assert.deepEqual(createDefaultExtensionSettings().authenticatedHandoffHosts, []);
assert.deepEqual(createDefaultExtensionSettings().capturedFileExtensions, []);

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
  normalizeExtensionSettings({
    capturedFileExtensions: [' zip rar exe 7zip ppt pptx docx ', '.ZIP', 'invalid/path'],
  }).capturedFileExtensions,
  ['zip', 'rar', 'exe', '7z', 'ppt', 'pptx', 'docx'],
  'captured file extensions should normalize whitespace input, aliases, dots, and duplicates',
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

assert.equal(
  normalizeExtensionSettings({
    authenticatedHandoffEnabled: true,
    protectedDownloadAuthScope: 'allowlist',
    authenticatedHandoffHosts: ['chatgpt.com'],
  }).protectedDownloadAuthScope,
  'legacy_global',
  'legacy allowlist settings should migrate to global browser-session forwarding',
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
