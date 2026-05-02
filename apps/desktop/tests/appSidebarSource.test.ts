import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');

assert.match(source, /isDownloadSectionExpanded/, 'the regular download sidebar section should track expanded/collapsed state');
assert.match(source, /isTorrentSectionExpanded/, 'the torrent sidebar section should track expanded/collapsed state');
assert.match(source, /Collapse downloads section/, 'the all-downloads section should expose an accessible collapse action');
assert.match(source, /Collapse torrents section/, 'the torrent section should expose an accessible collapse action');
assert.match(source, /isTorrentStatusView \? 'torrents' : 'downloads'/, 'the footer status bar should switch to a torrent-specific mode in torrent views');
assert.match(source, /Download size=\{16\} class="text-primary"[\s\S]*formatBytes\(torrentStats\.downloadedBytes\)[\s\S]*Total Ratio/, 'the torrent footer should show total torrent downloaded bytes before total ratio');
assert.match(source, /download-sidebar flex w-\[220px\] shrink-0 flex-col overflow-hidden/, 'the sidebar shell should constrain overflow so only the navigation section scrolls');
assert.match(source, /<nav class="min-h-0 flex-1 overflow-y-auto overscroll-contain/, 'the sidebar navigation should scroll independently on short windows');
assert.match(source, /Gauge, 'Active'[\s\S]*CheckCircle2, 'Completed'[\s\S]*Torrents/, 'Completed downloads should remain visible with Active above the torrent section');
assert.match(source, /class="shrink-0 space-y-2"/, 'the Settings footer should stay fixed below the scrollable sidebar navigation');
assert.match(source, /import SettingsPage, \{ SETTINGS_SECTIONS, type SettingsSectionId \}/, 'the app shell should consume the shared settings section list');
assert.match(source, /let activeSettingsSectionId = \$state<SettingsSectionId>\(SETTINGS_SECTIONS\[0\]\.id\)/, 'settings view should track the active settings section');
assert.match(source, /onActiveSectionChange=\{\(id\) => activeSettingsSectionId = id\}/, 'settings page should update the active section from the shell');
assert.doesNotMatch(source, /Needs Attention|label="Queued"|return '(?:attention|queued|torrent-attention|torrent-queued)'/, 'the sidebar should not render removed attention or queued filters');
assert.doesNotMatch(source, /setView\(outcome\.mode === 'torrent' \? 'torrent-queued' : 'queued'\)|setView\('queued'\)/, 'new downloads should not navigate to removed queued-only views');
