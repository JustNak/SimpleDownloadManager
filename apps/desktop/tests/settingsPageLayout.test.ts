import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/SettingsPage.tsx', import.meta.url), 'utf8');

assert.match(
  source,
  /settings-surface[\s\S]*mx-auto[\s\S]*w-full[\s\S]*max-w-6xl[\s\S]*p-4/,
  'settings surface should keep a centered single-column content layout',
);

assert.match(
  source,
  /export const SETTINGS_SECTIONS = \[/,
  'settings should export its section anchors for the app sidebar',
);

for (const [sectionId, iconName] of [
  ['settings-general', 'general'],
  ['settings-updates', 'updates'],
  ['settings-torrenting', 'torrenting'],
  ['settings-appearance', 'appearance'],
  ['settings-extension', 'extension'],
  ['settings-native-host', 'native-host'],
]) {
  assert.match(
    source,
    new RegExp(`id: '${sectionId}'[\\s\\S]*href: '#${sectionId}'[\\s\\S]*iconName: '${iconName}'[\\s\\S]*description:`),
    `settings section metadata for ${sectionId} should include id, href, icon, and sidebar description`,
  );
}

assert.doesNotMatch(
  source,
  /settings-nav/,
  'settings page should not render an internal section navigator',
);

for (const sectionId of [
  'settings-general',
  'settings-updates',
  'settings-torrenting',
  'settings-appearance',
  'settings-extension',
  'settings-native-host',
]) {
  assert.match(
    source,
    new RegExp(`id="${sectionId}"`),
    `settings navigator target ${sectionId} should exist`,
  );
}

assert.match(
  source,
  /FieldRow label="Torrent Download Directory"[\s\S]*id="torrentDownloadDirectory"[\s\S]*Browse/,
  'torrent settings should expose a torrent download directory browse row',
);

assert.match(
  source,
  /FieldRow[\s\S]*label="Clear Cache Session"[\s\S]*handleClearTorrentSessionCache/,
  'torrent settings should expose the torrent session cache cleanup action',
);

assert.match(
  source,
  /FieldRow label="Peer Connection Watchdog"[\s\S]*Experimental auto tuning/,
  'torrent settings should expose the experimental peer connection watchdog mode',
);

assert.match(
  source,
  /<header className="sticky top-0 z-30 flex items-center justify-between border-b border-border bg-surface\/95 pb-3 pt-4 backdrop-blur/,
  'settings header actions should stay sticky while scrolling long settings pages',
);

assert.match(
  source,
  /function ExcludedSitesDialog/,
  'excluded-site editing should live in a dedicated dialog component',
);

assert.match(
  source,
  /Configure Sites/,
  'excluded-site row should open the dedicated configuration dialog',
);

assert.doesNotMatch(
  source,
  /No browser-only hosts configured/,
  'excluded-site row should not render a summary card around the configure button',
);

assert.doesNotMatch(
  source,
  /FieldRow label="Excluded Sites"[\s\S]*value=\{excludedHostInput\}/,
  'excluded-site row should not keep the full inline add/list editor',
);

assert.match(
  source,
  /FieldRow label="Click Opens Details" description="Show selected-download details on row click\."/,
  'appearance settings should expose the click-to-show details pane toggle with clear copy',
);

assert.match(
  source,
  /id="showDetailsOnClick"[\s\S]*checked=\{formData\.showDetailsOnClick\}/,
  'click-to-show details setting should be wired to the settings draft',
);

assert.match(
  source,
  /id="torrentSeedMode"[\s\S]*className="h-9 w-44/,
  'torrent seeding policy select should use the shorter field width',
);

assert.match(
  source,
  /import desktopPackage from '\.\.\/package\.json';/,
  'settings update panel should read the installed desktop version from the desktop package manifest',
);

assert.match(
  source,
  /const updateVersion = updateVersionIndicator\(updateState, DESKTOP_APP_VERSION\);/,
  'settings update panel should derive current and new version indicators from update state',
);

assert.match(
  source,
  /<VersionIndicator label="Current" value=\{updateVersion\.currentVersion\} \/>[\s\S]*<VersionIndicator label="New" value=\{updateVersion\.newVersion\} tone=\{updateVersion\.newVersionTone\} \/>/,
  'app updates section should show both current and new version indicators',
);
