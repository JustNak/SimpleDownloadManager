import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import {
  extensionDisplayVersion,
  extensionManifestVersion,
} from '../scripts/version.mjs';

assert.equal(extensionManifestVersion('0.2.9-alpha'), '0.2.9');
assert.equal(extensionManifestVersion('1.4.7-beta.2'), '1.4.7');
assert.equal(extensionManifestVersion('2.0.0'), '2.0.0');
assert.equal(extensionDisplayVersion('0.2.9-alpha'), '0.2.9-alpha');

const firefoxManifestPath = path.resolve('apps/extension/dist/firefox/manifest.json');
const firefoxManifest = JSON.parse(await readFile(firefoxManifestPath, 'utf8'));

assert.equal(firefoxManifest.manifest_version, 2);
assert.deepEqual(firefoxManifest.background, { scripts: ['background.js'] });
assert.equal(firefoxManifest.browser_action.default_title, 'Simple Download Manager');
assert.equal(
  firefoxManifest.browser_specific_settings.gecko.id,
  'simple-download-manager@example.com',
);
assert.deepEqual(
  firefoxManifest.permissions,
  ['contextMenus', 'downloads', 'nativeMessaging', 'storage'],
);
