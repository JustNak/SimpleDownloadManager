import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const registerSource = await readFile('scripts/register-native-host-linux.sh', 'utf8').catch((error) => {
  assert.fail(`register-native-host-linux.sh should exist: ${error.message}`);
});
const unregisterSource = await readFile('scripts/unregister-native-host-linux.sh', 'utf8').catch((error) => {
  assert.fail(`unregister-native-host-linux.sh should exist: ${error.message}`);
});

assert.match(registerSource, /set -eu/, 'registration script should use strict shell mode');
assert.match(registerSource, /HOST_NAME="com\.myapp\.download_manager"/, 'registration script should preserve native host name');
assert.match(registerSource, /CHROMIUM_EXTENSION_ID="\$\{SDM_CHROMIUM_EXTENSION_ID:-pkaojpfpjieklhinoibjibmjldohlmbb\}"/, 'registration script should default Chromium extension ID');
assert.match(registerSource, /FIREFOX_EXTENSION_ID="\$\{SDM_FIREFOX_EXTENSION_ID:-simple-download-manager@example\.com\}"/, 'registration script should default Firefox extension ID');
assert.match(registerSource, /\/etc\/opt\/chrome\/native-messaging-hosts/, 'registration script should install Google Chrome manifest');
assert.match(registerSource, /\/etc\/chromium\/native-messaging-hosts/, 'registration script should install Chromium manifest');
assert.match(registerSource, /\/etc\/opt\/edge\/native-messaging-hosts/, 'registration script should install Microsoft Edge manifest');
assert.match(registerSource, /\/usr\/lib\/mozilla\/native-messaging-hosts/, 'registration script should install Firefox manifest');
assert.match(registerSource, /"path": "\$HOST_BINARY_PATH"/, 'registration script should write absolute native-host binary path');
assert.doesNotMatch(registerSource, /~\//, 'system package registration should not write user-profile manifests');

assert.match(unregisterSource, /set -eu/, 'unregistration script should use strict shell mode');
assert.match(unregisterSource, /HOST_NAME="com\.myapp\.download_manager"/, 'unregistration script should preserve native host name');
assert.match(unregisterSource, /\/etc\/opt\/chrome\/native-messaging-hosts/, 'unregistration script should remove Google Chrome manifest');
assert.match(unregisterSource, /\/etc\/chromium\/native-messaging-hosts/, 'unregistration script should remove Chromium manifest');
assert.match(unregisterSource, /\/etc\/opt\/edge\/native-messaging-hosts/, 'unregistration script should remove Microsoft Edge manifest');
assert.match(unregisterSource, /\/usr\/lib\/mozilla\/native-messaging-hosts/, 'unregistration script should remove Firefox manifest');

const debPostinst = await readFile('apps/desktop/src-tauri/linux/deb/postinst', 'utf8').catch((error) => {
  assert.fail(`Debian postinst should exist: ${error.message}`);
});
const debPostrm = await readFile('apps/desktop/src-tauri/linux/deb/postrm', 'utf8').catch((error) => {
  assert.fail(`Debian postrm should exist: ${error.message}`);
});
const rpmPostinstall = await readFile('apps/desktop/src-tauri/linux/rpm/postinstall', 'utf8').catch((error) => {
  assert.fail(`RPM postinstall should exist: ${error.message}`);
});
const rpmPostremove = await readFile('apps/desktop/src-tauri/linux/rpm/postremove', 'utf8').catch((error) => {
  assert.fail(`RPM postremove should exist: ${error.message}`);
});

for (const [name, source] of [
  ['deb postinst', debPostinst],
  ['deb postrm', debPostrm],
  ['rpm postinstall', rpmPostinstall],
  ['rpm postremove', rpmPostremove],
]) {
  assert.match(source, /set -eu/, `${name} should use strict shell mode`);
}

assert.match(debPostinst, /\/usr\/bin\/simple-download-manager-native-host/, 'Debian postinst should register packaged host path');
assert.match(rpmPostinstall, /\/usr\/bin\/simple-download-manager-native-host/, 'RPM postinstall should register packaged host path');
assert.match(debPostrm, /unregister-native-host-linux\.sh/, 'Debian postrm should unregister manifests');
assert.match(rpmPostremove, /unregister-native-host-linux\.sh/, 'RPM postremove should unregister manifests');
