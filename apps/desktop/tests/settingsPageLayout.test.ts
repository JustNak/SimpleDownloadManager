import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/SettingsPage.svelte', import.meta.url), 'utf8');

assert.match(source, /settings-surface flex min-h-0 flex-1 overflow-hidden bg-background/, 'settings surface should keep an overflow-protected application layout');
assert.match(source, /export const SETTINGS_SECTIONS = \[/, 'settings should export its section list for the app shell');

for (const [sectionId, label] of [
  ['downloads', 'Downloads'],
  ['torrents', 'Torrents'],
  ['extension', 'Extension'],
  ['appearance', 'Appearance'],
  ['startup', 'Startup'],
]) {
  assert.match(source, new RegExp(`id: '${sectionId}'[\\s\\S]*label: '${label}'`), `settings section metadata for ${sectionId} should exist`);
}

assert.match(source, /onActiveSectionChange\(section\.id\)/, 'settings sidebar should update the active section through the app shell');
assert.match(source, /Download behavior/, 'download settings should render the download behavior panel');
assert.match(source, /Torrent engine/, 'torrent settings should render the torrent engine panel');
assert.match(source, /Browser integration/, 'extension settings should render the browser integration panel');
assert.match(source, /Appearance/, 'appearance settings should render the appearance panel');
assert.match(source, /Startup/, 'startup settings should render the startup panel');
assert.match(source, /bind:value=\{formData\.torrent\.downloadDirectory\}/, 'torrent settings should expose the torrent download directory field');
assert.match(source, /Clear torrent session cache/, 'torrent settings should expose the torrent session cache cleanup action');
assert.match(source, /Show details on click/, 'settings should expose the click-to-show details pane toggle');
assert.match(source, /bind:checked=\{formData\.showDetailsOnClick\}/, 'click-to-show details setting should be wired to the settings draft');
assert.match(source, /bind:value=\{formData\.queueRowSize\}/, 'queue row-size setting should be wired to the settings draft');
assert.match(source, /bind:value=\{accentColorInput\}/, 'accent color setting should be wired through the normalized color draft');
