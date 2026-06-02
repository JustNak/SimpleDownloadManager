import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const script = await readFile('scripts/build-release-linux.sh', 'utf8').catch((error) => {
  assert.fail(`Linux release script should exist: ${error.message}`);
});
const rootPackage = JSON.parse(await readFile('package.json', 'utf8'));

assert.match(script, /set -euo pipefail/, 'Linux release script should use strict bash mode');
assert.match(script, /TAURI_SIGNING_PRIVATE_KEY/, 'Linux release script should require Tauri updater signing key');
assert.match(script, /x86_64-unknown-linux-gnu/, 'Linux release script should build x64 GNU target');
assert.match(script, /cargo build --release --manifest-path "\$host_root\/Cargo.toml" --target "\$rust_target"/, 'Linux release script should build native host sidecar');
assert.match(script, /--bundles deb,rpm,appimage/, 'Linux release script should build deb, rpm, and AppImage bundles');
assert.match(script, /bundle\/appimage/, 'Linux release script should copy AppImage artifacts');
assert.match(script, /bundle\/deb/, 'Linux release script should copy Debian artifacts');
assert.match(script, /bundle\/rpm/, 'Linux release script should copy RPM artifacts');
assert.match(script, /scripts\/updater-release\.mjs/, 'Linux release script should write updater metadata');

assert.equal(rootPackage.scripts['release:linux'], 'bash ./scripts/build-release-linux.sh');
assert.equal(rootPackage.scripts['release:linux:x64'], 'bash ./scripts/build-release-linux.sh --targets x64');
