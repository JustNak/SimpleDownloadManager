import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/SettingsPage.svelte', import.meta.url), 'utf8');
const appSource = readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8');
const backendSource = readFileSync(new URL('../src/backend.ts', import.meta.url), 'utf8');
const sectionsSource = readFileSync(new URL('../src/settingsSections.ts', import.meta.url), 'utf8');

assert.match(source, /settings-surface mx-auto w-full max-w-6xl p-4/, 'settings content should keep the Svelte centered max-width form layout');
assert.match(sectionsSource, /export const SETTINGS_SECTIONS = \[/, 'settings section list should live in the lightweight app-shell metadata module');

for (const [sectionId, label] of [
  ['settings-general', 'General'],
  ['settings-updates', 'App Updates'],
  ['settings-torrenting', 'Torrenting'],
  ['settings-appearance', 'Appearance'],
  ['settings-extension', 'Web Extension'],
  ['settings-native-host', 'Native Host'],
]) {
  assert.match(sectionsSource, new RegExp(`id: '${sectionId}'[\\s\\S]*label: '${label}'`), `settings section metadata for ${sectionId} should exist`);
  assert.match(source, new RegExp(`<section id="${sectionId}" class="scroll-mt-4"`), `settings section ${sectionId} should render as a scroll anchor`);
}

assert.match(source, /sticky top-0 z-30[\s\S]*bg-surface\/95[\s\S]*backdrop-blur/, 'settings header should keep the Svelte sticky translucent header');
assert.match(source, /Configure downloads, appearance, notifications, and native host diagnostics\./, 'settings subtitle should match Svelte');
assert.match(source, /Cancel[\s\S]*Save Changes/, 'settings header should keep Svelte cancel and save actions');
assert.match(source, /CategorySettingsCard\('General'/, 'general settings should render through the category card helper');
assert.match(source, /Beta channel updates/, 'app update card should be present');
assert.doesNotMatch(source, /0\.5\.1-beta/, 'app update current version should not use the stale hardcoded 0.5.1-beta fallback');
assert.match(source, /installedVersion:\s*string/, 'settings page should receive the installed app version from the app shell');
assert.match(source, /updateVersionIndicator\(updateState,\s*installedVersion\)/, 'app update version rows should use the shared installed-version indicator helper');
assert.match(appSource, /getInstalledVersion/, 'app shell should read the installed app version through the backend wrapper');
assert.doesNotMatch(appSource, /Preview build/, 'app shell should not initialize update version rows with the non-version Preview build fallback');
assert.doesNotMatch(backendSource, /return 'Preview build'/, 'backend version wrapper should not return the non-version Preview build fallback');
assert.match(backendSource, /import desktopPackage from '\.\.\/package\.json'/, 'backend should read the desktop package version for browser preview and fallbacks');
assert.match(backendSource, /export const APP_VERSION = desktopPackage\.version/, 'backend should expose the desktop package version as an exact app-version fallback');
assert.match(appSource, /let installedVersion = \$state\(APP_VERSION\)/, 'app shell should initialize installed version state from the exact package version');
assert.match(backendSource, /plugin:app\|version/, 'backend wrapper should invoke the existing Tauri app version command directly so removeUnusedCommands preserves it');
assert.match(appSource, /let installedVersion = \$state/, 'app shell should keep installed version state for settings rendering');
assert.match(appSource, /installedVersion=\{installedVersion\}/, 'app shell should pass the installed version into settings');
assert.match(source, /CategorySettingsCard\('Torrenting'/, 'torrent settings should render the torrenting category card');
assert.match(source, /CategorySettingsCard\('Appearance'/, 'appearance settings should render the appearance category card');
assert.match(source, /CategorySettingsCard\('Web Extension'/, 'extension settings should render the web extension category card');
assert.match(source, /CategorySettingsCard\('Native Host'/, 'native host diagnostics should render the native host category card');
assert.match(source, /#snippet CategorySettingsCard[\s\S]*rounded-md border border-border\/60 bg-card/, 'settings categories should use one softened card wrapper per category');
assert.match(source, /#snippet CategorySettingsCard[\s\S]*border-b border-border\/45 bg-header px-4 py-2/, 'category cards should keep a softened Svelte-style section header treatment');
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
assert.match(source, /isExcludedSitesDialogOpen/, 'excluded sites should use the Svelte dialog workflow');
assert.match(source, /onRefreshDiagnostics/, 'native host diagnostics should keep the refresh callback');
assert.match(source, /onCheckForUpdates/, 'app updates section should keep the manual update callback');
assert.match(source, /Recent Events[\s\S]*max-h-56 overflow-auto rounded-md border border-border\/55 bg-zinc-950 font-mono shadow-inner/, 'recent events should render as a compact console-like box');
assert.match(source, /diagnosticLevelConsoleClass/, 'recent event levels should use console-specific colors');
assert.match(source, /const recentDiagnosticEvents = \$derived\(diagnostics\?\.recentEvents \? \[\.\.\.diagnostics\.recentEvents\]\.reverse\(\) : \[\]\)/, 'recent events should use a reversed display copy so newest entries render first');
assert.match(source, /\{#if recentDiagnosticEvents\.length\}[\s\S]*\{#each recentDiagnosticEvents as event\}/, 'recent events should render from the newest-first display list');
assert.doesNotMatch(source, /\{#each diagnostics\.recentEvents as event\}/, 'recent events should not render directly from the backend-ordered diagnostics array');
