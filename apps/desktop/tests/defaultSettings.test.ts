import assert from 'node:assert/strict';
import { createDefaultExtensionIntegrationSettings } from '../src/defaultSettings.ts';

assert.deepEqual(
  createDefaultExtensionIntegrationSettings().authenticatedHandoffHosts,
  ['gofile.io', '*.instructure.com'],
  'desktop default extension settings should enable protected handoff coverage for Instructure-hosted Canvas files',
);
