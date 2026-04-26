import assert from 'node:assert/strict';
import {
  extensionDisplayVersion,
  extensionManifestVersion,
} from '../scripts/version.mjs';

assert.equal(extensionManifestVersion('0.2.8-a'), '0.2.8');
assert.equal(extensionManifestVersion('1.4.7-beta.2'), '1.4.7');
assert.equal(extensionManifestVersion('2.0.0'), '2.0.0');
assert.equal(extensionDisplayVersion('0.2.8-a'), '0.2.8-a');
