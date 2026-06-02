import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import {
  extensionDisplayVersion,
  extensionManifestVersion,
  extensionVersionsFromPackage,
} from '../scripts/version.mjs';

assert.equal(extensionManifestVersion('0.2.9-alpha'), '0.2.9');
assert.equal(extensionManifestVersion('1.4.7-beta.2'), '1.4.7');
assert.equal(extensionManifestVersion('2.0.0'), '2.0.0');
assert.equal(extensionDisplayVersion('0.2.9-alpha'), '0.2.9-alpha');
assert.deepEqual(
  extensionVersionsFromPackage({ version: '0.3.48-beta' }),
  {
    browserExtensionVersion: '0.3.48',
    displayVersion: '0.3.48-beta',
  },
  'extension manifest versions should come from the extension package version, not the app package version',
);

const firefoxManifestPath = path.resolve('apps/extension/dist/firefox/manifest.json');
const firefoxManifest = JSON.parse(await readFile(firefoxManifestPath, 'utf8'));
const chromiumManifestPath = path.resolve('apps/extension/dist/chromium/manifest.json');
const chromiumManifest = JSON.parse(await readFile(chromiumManifestPath, 'utf8'));
const extensionPackage = JSON.parse(await readFile(path.resolve('apps/extension/package.json'), 'utf8'));
const buildScript = await readFile(
  new URL('../scripts/build.mjs', import.meta.url),
  'utf8',
);

assert.equal(firefoxManifest.manifest_version, 2);
assert.equal(
  extensionPackage.version,
  '1.1.3',
  'extension package version should remain on its own release version even when the desktop app is bumped',
);
assert.equal(firefoxManifest.version_name, extensionPackage.version);
assert.equal(chromiumManifest.version_name, extensionPackage.version);
assert.deepEqual(firefoxManifest.background, { scripts: ['background.js'] });
assert.equal(firefoxManifest.browser_action.default_title, 'Simple Download Manager');
assert.deepEqual(firefoxManifest.icons, {
  16: 'icons/icon-16.png',
  32: 'icons/icon-32.png',
  48: 'icons/icon-48.png',
  128: 'icons/icon-128.png',
});
assert.deepEqual(firefoxManifest.browser_action.default_icon, firefoxManifest.icons);
assert.equal(
  firefoxManifest.browser_specific_settings.gecko.id,
  'simple-download-manager@example.com',
);
assert.equal(
  firefoxManifest.browser_specific_settings.gecko.strict_min_version,
  '142.0',
  'Firefox upload should rely on the built-in data consent prompt without web-ext Android compatibility warnings',
);
assert.deepEqual(
  firefoxManifest.browser_specific_settings.gecko.data_collection_permissions,
  {
    required: ['browsingActivity', 'websiteActivity', 'websiteContent'],
  },
  'Firefox manifest should disclose required data transmission for AMO upload compliance',
);
assert.deepEqual(
  firefoxManifest.permissions,
  ['alarms', 'contextMenus', 'downloads', 'nativeMessaging', 'storage', 'webRequest', 'webRequestBlocking', '<all_urls>'],
);
assert.deepEqual(
  chromiumManifest.permissions,
  ['alarms', 'contextMenus', 'downloads', 'nativeMessaging', 'storage', 'webRequest'],
  'Chromium build should request webRequest for download response classification',
);
assert.deepEqual(
  chromiumManifest.host_permissions,
  ['<all_urls>'],
  'Chromium webRequest observation requires host permissions for download origins',
);
assert.equal('content_scripts' in firefoxManifest, false, 'Firefox build should not inject page-managed download content scripts');
assert.equal('content_scripts' in chromiumManifest, false, 'Chromium build should not inject page-managed download content scripts');
assert.equal('web_accessible_resources' in firefoxManifest, false, 'Firefox build should not expose page-managed download hooks');
assert.equal('web_accessible_resources' in chromiumManifest, false, 'Chromium build should not expose page-managed download hooks');

assert.match(
  buildScript,
  /rm\(outdir,\s*\{\s*recursive:\s*true,\s*force:\s*true\s*\}\)/,
  'extension build should clear each target output directory before writing release ZIP contents',
);
assert.doesNotMatch(buildScript, /blobDownloadInterceptor/, 'extension build should not bundle the removed page-managed download interceptor');
assert.doesNotMatch(buildScript, /blobDownloadPageHook/, 'extension build should not bundle the removed page hook');
