import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/SettingsPage.svelte', import.meta.url), 'utf8');

assert.match(source, /settings-surface mx-auto w-full max-w-6xl p-4/, 'settings content should keep the React centered max-width form layout');
assert.match(source, /export const SETTINGS_SECTIONS = \[/, 'settings should export its section list for the app shell');

for (const [sectionId, label] of [
  ['settings-general', 'General'],
  ['settings-updates', 'App Updates'],
  ['settings-torrenting', 'Torrenting'],
  ['settings-appearance', 'Appearance'],
  ['settings-extension', 'Web Extension'],
  ['settings-native-host', 'Native Host'],
]) {
  assert.match(source, new RegExp(`id: '${sectionId}'[\\s\\S]*label: '${label}'`), `settings section metadata for ${sectionId} should exist`);
  assert.match(source, new RegExp(`<section id="${sectionId}" class="scroll-mt-4"`), `settings section ${sectionId} should render as a scroll anchor`);
}

assert.match(source, /sticky top-0 z-30[\s\S]*bg-surface\/95[\s\S]*backdrop-blur/, 'settings header should keep the React sticky translucent header');
assert.match(source, /Configure downloads, appearance, notifications, and native host diagnostics\./, 'settings subtitle should match React');
assert.match(source, /Cancel[\s\S]*Save Changes/, 'settings header should keep React cancel and save actions');
assert.match(source, /SettingsPanel\('General'/, 'general settings should render through the React-style panel helper');
assert.match(source, /Alpha channel updates/, 'app update card should be present');
assert.match(source, /SettingsPanel\('Torrenting'/, 'torrent settings should render the torrenting panel');
assert.match(source, /SettingsPanel\('Appearance'/, 'appearance settings should render the appearance panel');
assert.match(source, /SettingsPanel\('Web Extension'/, 'extension settings should render the web extension panel');
assert.match(source, /SettingsPanel\('Native Host'/, 'native host diagnostics should render the native host panel');
assert.match(source, /bind:value=\{formData\.torrent\.downloadDirectory\}/, 'torrent settings should expose the torrent download directory field');
assert.match(source, /Clear torrent session cache/, 'torrent settings should expose the torrent session cache cleanup action');
assert.match(source, /Show details on click/, 'settings should expose the click-to-show details pane toggle');
assert.match(source, /formData\.showDetailsOnClick[\s\S]*formData\.showDetailsOnClick = checked/, 'click-to-show details setting should be wired to the settings draft');
assert.match(source, /bind:value=\{formData\.queueRowSize\}/, 'queue row-size setting should be wired to the settings draft');
assert.match(source, /bind:value=\{accentColorInput\}/, 'accent color setting should be wired through the normalized color draft');
assert.match(source, /isExcludedSitesDialogOpen/, 'excluded sites should use the React dialog workflow');
assert.match(source, /onRefreshDiagnostics/, 'native host diagnostics should keep the refresh callback');
assert.match(source, /onCheckForUpdates/, 'app updates section should keep the manual update callback');
