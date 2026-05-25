import assert from 'node:assert/strict';
import { createDefaultExtensionIntegrationSettings } from '../src/defaultSettings.ts';

assert.deepEqual(
  createDefaultExtensionIntegrationSettings().authenticatedHandoffHosts,
  [],
  'desktop default extension settings should not depend on a protected-download host allowlist',
);
assert.equal(
  createDefaultExtensionIntegrationSettings().protectedDownloadAuthScope,
  'legacy_global',
  'desktop default extension settings should use global Protected Downloads when enabled',
);
