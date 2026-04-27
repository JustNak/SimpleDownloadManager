import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const repoRoot = path.resolve(path.dirname(__filename), '..', '..', '..');
const hooksPath = path.join(repoRoot, 'apps', 'desktop', 'src-tauri', 'windows', 'hooks.nsh');
const hooks = await readFile(hooksPath, 'utf8');

assert.match(
  hooks,
  /Start Simple Download Manager when Windows starts\?/,
  'installer should ask whether to enable Windows startup',
);
assert.match(
  hooks,
  /Start minimized to tray when Windows starts\?/,
  'installer should ask whether startup launches minimized to tray',
);
assert.match(
  hooks,
  /MB_YESNO\|MB_ICONQUESTION/,
  'startup options should be explicit yes/no installer prompts',
);
assert.match(
  hooks,
  /StrCmp\s+\$PassiveMode\s+1\s+done_startup_options/,
  'passive installs should not show interactive startup prompts',
);
assert.match(
  hooks,
  /StrCmp\s+\$UpdateMode\s+1\s+relaunch_after_update/,
  'updates should preserve existing startup settings and relaunch the app without prompting',
);
assert.match(
  hooks,
  /relaunch_after_update:\s+Exec\s+'"\$INSTDIR\\\$\{MAINBINARYNAME\}\.exe"'/,
  'updates should relaunch the installed app so the main window returns after completion',
);
assert.match(
  hooks,
  /"\$INSTDIR\\\$\{MAINBINARYNAME\}\.exe" --installer-configure --installer-startup/,
  'installer should apply startup settings through the installed app binary',
);
assert.match(
  hooks,
  /--installer-configure --installer-startup --installer-tray/,
  'installer should pass a tray-only startup flag when minimized startup is selected',
);
