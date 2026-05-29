import assert from 'node:assert/strict';
import {
  DEFAULT_CAPTURED_FILE_EXTENSIONS,
  createDefaultExtensionIntegrationSettings,
} from '../src/defaultSettings.ts';

assert.equal(
  createDefaultExtensionIntegrationSettings().downloadCaptureDebugLogging,
  false,
  'desktop default extension settings should keep download capture debug logging off',
);
assert.deepEqual(
  createDefaultExtensionIntegrationSettings().capturedFileExtensions,
  [...DEFAULT_CAPTURED_FILE_EXTENSIONS],
  'desktop default extension settings should expose the editable captured extension list',
);
