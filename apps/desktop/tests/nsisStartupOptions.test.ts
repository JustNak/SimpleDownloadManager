import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const repoRoot = path.resolve(path.dirname(__filename), '..', '..', '..');
const hooksPath = path.join(repoRoot, 'apps', 'desktop', 'src-tauri', 'windows', 'hooks.nsh');
const templatePath = path.join(repoRoot, 'apps', 'desktop', 'src-tauri', 'windows', 'installer.nsi');
const configPath = path.join(repoRoot, 'apps', 'desktop', 'src-tauri', 'tauri.conf.json');
const mainPath = path.join(repoRoot, 'apps', 'desktop', 'src-tauri', 'src', 'main.rs');
const hooks = await readFile(hooksPath, 'utf8');
const template = await readFile(templatePath, 'utf8');
const tauriConfig = JSON.parse(await readFile(configPath, 'utf8'));
const mainSource = await readFile(mainPath, 'utf8');

assert.equal(
  tauriConfig.bundle.windows.nsis.template,
  './windows/installer.nsi',
  'NSIS should use the project-owned template that adds the startup options page',
);
assert.equal(
  tauriConfig.bundle.windows.nsis.headerImage,
  './windows/installer-header.bmp',
  'NSIS should use the compact branded header bitmap',
);

const startupPageIndex = template.indexOf('Page custom PageStartupOptions PageLeaveStartupOptions');
const installFilesPageIndex = template.indexOf('!insertmacro MUI_PAGE_INSTFILES');
const hooksIncludeIndex = template.indexOf('!include "{{installer_hooks}}"');
const mainBinaryDefineIndex = template.indexOf('!define MAINBINARYNAME');
const updateModeVarIndex = template.indexOf('Var UpdateMode');
assert.notEqual(startupPageIndex, -1, 'installer template should insert the startup options custom page');
assert.notEqual(installFilesPageIndex, -1, 'installer template should keep the standard install files page');
assert.ok(
  startupPageIndex < installFilesPageIndex,
  'startup options should be selected before files are installed',
);
assert.ok(
  hooksIncludeIndex > mainBinaryDefineIndex && hooksIncludeIndex > updateModeVarIndex,
  'installer hooks should be included after template defines and mode variables used by custom pages',
);

assert.doesNotMatch(
  hooks,
  /Start Simple Download Manager when Windows starts\?/,
  'installer should not use the old first startup confirmation popup',
);
assert.doesNotMatch(
  hooks,
  /Start minimized to tray when Windows starts\?/,
  'installer should not use the old second tray confirmation popup',
);
assert.doesNotMatch(
  hooks,
  /MB_YESNO\|MB_ICONQUESTION/,
  'startup options should not be implemented as modal yes/no popups',
);
assert.match(
  hooks,
  /Function\s+PageStartupOptions[\s\S]*nsDialogs::Create\s+1018[\s\S]*Start Simple Download Manager with Windows[\s\S]*Start minimized to tray/,
  'startup options should render as one nsDialogs page with both choices',
);
assert.match(
  hooks,
  /Function\s+PageStartupOptions[\s\S]*SendMessage\s+\$StartupCheckbox\s+\$\{BM_SETCHECK\}\s+\$\{BST_CHECKED\}[\s\S]*SendMessage\s+\$StartupTrayCheckbox\s+\$\{BM_SETCHECK\}\s+\$\{BST_CHECKED\}/,
  'startup and tray options should default to checked',
);
assert.match(
  hooks,
  /Function\s+PageStartupOptions[\s\S]*\$\{If\}\s+\$PassiveMode\s+=\s+1[\s\S]*Abort[\s\S]*\$\{If\}\s+\$UpdateMode\s+=\s+1[\s\S]*Abort[\s\S]*IfSilent\s+0\s+\+2[\s\S]*Abort/,
  'passive, update, and silent installs should skip the interactive options page',
);
assert.match(
  hooks,
  /Function\s+PageLeaveStartupOptions[\s\S]*StrCpy\s+\$StartupOptionsApplyState\s+1/,
  'startup options page should record that interactive choices should be applied after installation',
);
assert.match(
  hooks,
  /Function\s+ApplyStartupOptions[\s\S]*\$\{If\}\s+\$StartupCheckboxState\s+==\s+\$\{BST_CHECKED\}/,
  'postinstall startup option application should branch on the selected Windows startup state',
);
assert.match(
  hooks,
  /Function\s+ApplyStartupOptions[\s\S]*"\$INSTDIR\\\$\{MAINBINARYNAME\}\.exe" --installer-configure --installer-startup --installer-tray/,
  'postinstall startup option application should apply startup-on with tray through the installed app binary',
);
assert.match(
  hooks,
  /Function\s+ApplyStartupOptions[\s\S]*"\$INSTDIR\\\$\{MAINBINARYNAME\}\.exe" --installer-configure --installer-startup'/,
  'postinstall startup option application should apply startup-on without tray through the installed app binary',
);
assert.match(
  hooks,
  /Function\s+ApplyStartupOptions[\s\S]*"\$INSTDIR\\\$\{MAINBINARYNAME\}\.exe" --installer-configure'/,
  'postinstall startup option application should apply startup-off through the installed app binary',
);
assert.match(
  hooks,
  /Function\s+UpdateStartupTrayCheckbox[\s\S]*EnableWindow\s+\$StartupTrayCheckbox\s+0/,
  'tray startup option should be disabled when Windows startup is unchecked',
);
assert.match(
  hooks,
  /Function\s+UpdateStartupTrayCheckbox[\s\S]*EnableWindow\s+\$StartupTrayCheckbox\s+1/,
  'tray startup option should be enabled when Windows startup is checked',
);
assert.doesNotMatch(
  hooks,
  /relaunch_after_update:|Exec\s+'"\$INSTDIR\\\$\{MAINBINARYNAME\}\.exe"'/,
  'postinstall hooks should not add a second updater relaunch that can race the built-in /R launch',
);
assert.match(
  mainSource,
  /tauri_plugin_updater::Builder::new\(\)\s*\.installer_arg\(lifecycle::POST_UPDATE_ARG\)\s*\.build\(\)/s,
  'updater relaunches should include the post-update marker used to show the main window',
);
assert.match(
  hooks,
  /nsExec::ExecToLog\s+'"\$SYSDIR\\WindowsPowerShell\\v1\.0\\powershell\.exe" -ExecutionPolicy Bypass -File "\$INSTDIR\\resources\\install\\register-native-host\.ps1"/,
  'postinstall hook should still register the browser native host',
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
