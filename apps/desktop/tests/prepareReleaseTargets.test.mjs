import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile('scripts/prepare-release.mjs', 'utf8');

assert.match(source, /linuxReleaseTargetForRustTarget/, 'prepare-release should resolve Linux release targets');
assert.match(source, /simple-download-manager-native-host-\$\{hostTuple\}/, 'Linux sidecar names should omit .exe');
assert.match(source, /register-native-host-linux\.sh/, 'Linux release staging should include registration script');
assert.match(source, /unregister-native-host-linux\.sh/, 'Linux release staging should include unregistration script');
assert.match(source, /postInstallScript/, 'Linux Tauri config override should wire package post-install scripts');
assert.match(source, /postRemoveScript/, 'Linux Tauri config override should wire package post-remove scripts');
assert.match(source, /\/usr\/share\/simple-download-manager\/install\/register-native-host-linux\.sh/, 'Linux packages should include registration helper in a stable share path');
