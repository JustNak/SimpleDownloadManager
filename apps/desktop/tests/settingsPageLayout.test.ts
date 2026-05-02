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
assert.match(source, /CategorySettingsCard\('General'/, 'general settings should render through the category card helper');
assert.match(source, /Alpha channel updates/, 'app update card should be present');
assert.match(source, /CategorySettingsCard\('Torrenting'/, 'torrent settings should render the torrenting category card');
assert.match(source, /CategorySettingsCard\('Appearance'/, 'appearance settings should render the appearance category card');
assert.match(source, /CategorySettingsCard\('Web Extension'/, 'extension settings should render the web extension category card');
assert.match(source, /CategorySettingsCard\('Native Host'/, 'native host diagnostics should render the native host category card');
assert.match(source, /#snippet CategorySettingsCard[\s\S]*rounded-md border border-border\/60 bg-card/, 'settings categories should use one softened card wrapper per category');
assert.match(source, /#snippet CategorySettingsCard[\s\S]*border-b border-border\/45 bg-header px-4 py-2/, 'category cards should keep a softened React-style section header treatment');
assert.match(source, /#snippet SwitchFieldRow/, 'switch settings should share the flat field-row treatment inside category cards');
assert.match(source, /#snippet FieldRow[\s\S]*border-t border-border\/35/, 'field rows should use toned-down separators');
assert.match(source, /#snippet SwitchFieldRow[\s\S]*border-t border-border\/35/, 'switch rows should use toned-down separators');
assert.doesNotMatch(source, /#snippet SettingsPanel[\s\S]*rounded-md border border-border bg-card/, 'settings sections should not use card panel wrappers');
assert.doesNotMatch(source, /#snippet CompactSetting[\s\S]*rounded-md border border-border bg-surface/, 'switch settings should not use compact card-like wrappers');
assert.doesNotMatch(source, /<div class="rounded-md border border-border bg-surface p-4">/, 'settings content should not use nested page cards');
assert.doesNotMatch(source, /border-border\/70/, 'settings page separators should avoid the heavier hairline treatment');
assert.match(source, /bind:value=\{formData\.torrent\.downloadDirectory\}/, 'torrent settings should expose the torrent download directory field');
assert.match(source, /Clear torrent session cache/, 'torrent settings should expose the torrent session cache cleanup action');
assert.match(source, /Show details on click/, 'settings should expose the click-to-show details pane toggle');
assert.match(source, /formData\.showDetailsOnClick[\s\S]*formData\.showDetailsOnClick = checked/, 'click-to-show details setting should be wired to the settings draft');
assert.match(source, /bind:value=\{formData\.queueRowSize\}/, 'queue row-size setting should be wired to the settings draft');
assert.match(source, /bind:value=\{accentColorInput\}/, 'accent color setting should be wired through the normalized color draft');
assert.match(source, /const accentGradientPresets = \[/, 'accent settings should offer gradient preset options above the palette');
assert.match(source, /Gradient options[\s\S]*Solid palette/, 'accent settings should separate gradient choices from solid color choices');
assert.match(source, /accentColorInput = preset\.value/, 'gradient accent presets should update the normalized accent draft');
assert.match(source, /#snippet accentControl\(\)[\s\S]*grid gap-3/, 'accent color control should use a larger multi-row palette layout');
assert.match(source, /Custom accent[\s\S]*bind:value=\{accentColorInput\}[\s\S]*Solid palette/, 'accent settings should keep a compact custom color and solid palette fallback');
assert.doesNotMatch(source, /Custom mix/, 'accent settings should not expose a full HSL editor');
assert.doesNotMatch(source, /Live preview[\s\S]*Primary[\s\S]*Soft[\s\S]*Selected/, 'accent settings should not include the oversized live token preview');
assert.doesNotMatch(source, /ColorSlider\('Hue'[\s\S]*ColorSlider\('Saturation'[\s\S]*ColorSlider\('Lightness'/, 'accent settings should not include HSL sliders');
assert.doesNotMatch(source, /function updateAccentFromHsl/, 'accent settings should not keep unused HSL update code');
assert.doesNotMatch(source, /function hexToHsl/, 'accent settings should not derive HSL state for the compact control');
assert.doesNotMatch(source, /function hslToHex/, 'accent settings should not convert HSL values for the compact control');
assert.match(source, /FieldRow\('Accent Color', 'Primary highlight color\.', accentControl, 'Primary highlight color\.', true\)/, 'accent color row should use the wider settings row treatment');
assert.match(source, /#snippet FieldRow[\s\S]*title=\{description\}[\s\S]*cursor-help/, 'field labels should expose descriptions as hover tooltips');
assert.match(source, /#snippet SwitchFieldRow[\s\S]*title=\{description\}[\s\S]*cursor-help/, 'switch labels should expose descriptions as hover tooltips');
assert.doesNotMatch(source, /<p class="mt-0\.5 text-xs leading-4 text-muted-foreground" title=\{tooltip\}>\{description\}<\/p>/, 'field descriptions should not render as wrapping helper text');
assert.doesNotMatch(source, /<div class="mt-0\.5 text-xs leading-4 text-muted-foreground" title=\{description\}>\{description\}<\/div>/, 'switch descriptions should not render as wrapping helper text');
assert.match(source, /isExcludedSitesDialogOpen/, 'excluded sites should use the React dialog workflow');
assert.match(source, /onRefreshDiagnostics/, 'native host diagnostics should keep the refresh callback');
assert.match(source, /onCheckForUpdates/, 'app updates section should keep the manual update callback');
assert.match(source, /Recent Events[\s\S]*max-h-56 overflow-auto rounded-md border border-border\/55 bg-zinc-950 font-mono shadow-inner/, 'recent events should render as a compact console-like box');
assert.match(source, /diagnosticLevelConsoleClass/, 'recent event levels should use console-specific colors');
