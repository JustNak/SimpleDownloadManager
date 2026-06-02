import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const readme = await readFile('README.md', 'utf8');
const install = await readFile('docs/install.md', 'utf8');

for (const [name, source] of [['README', readme], ['install docs', install]]) {
  assert.match(source, /\.deb/, `${name} should document Debian package`);
  assert.match(source, /\.rpm/, `${name} should document RPM package`);
  assert.match(source, /AppImage/, `${name} should document AppImage fallback`);
  assert.match(source, /native messaging/i, `${name} should mention native messaging`);
  assert.match(source, /Flatpak/i, `${name} should explain why Flatpak is not the primary Linux package`);
}

assert.match(readme, /Linux x64/, 'README should list Linux x64 artifacts');
assert.match(install, /\/etc\/opt\/chrome\/native-messaging-hosts/, 'install docs should include Chrome Linux manifest location');
assert.match(install, /\/usr\/lib\/mozilla\/native-messaging-hosts/, 'install docs should include Firefox Linux manifest location');
