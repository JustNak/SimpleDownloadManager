import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');

assert.match(
  source,
  /isDownloadSectionExpanded/,
  'the regular download sidebar section should track expanded/collapsed state',
);

assert.match(
  source,
  /isTorrentSectionExpanded/,
  'the torrent sidebar section should track expanded/collapsed state',
);

assert.match(
  source,
  /Collapse downloads section/,
  'the all-downloads section should expose an accessible collapse action',
);

assert.match(
  source,
  /Collapse torrents section/,
  'the torrent section should expose an accessible collapse action',
);

assert.match(
  source,
  /mode=\{isTorrentStatusView \? 'torrents' : 'downloads'\}/,
  'the footer status bar should switch to a torrent-specific mode in torrent views',
);

assert.match(
  source,
  /<Download size=\{16\} className="text-primary" \/>\s*\{formatBytes\(torrentStats\.downloadedBytes\)\}[\s\S]*Total Ratio/,
  'the torrent footer should show icon-only total torrent downloaded bytes before total ratio',
);

assert.match(
  source,
  /download-sidebar flex w-\[220px\] shrink-0 flex-col overflow-hidden/,
  'the sidebar shell should constrain overflow so only the navigation section scrolls',
);

assert.match(
  source,
  /<nav className="min-h-0 flex-1 overflow-y-auto overscroll-contain/,
  'the sidebar navigation should scroll independently on short windows',
);

assert.match(
  source,
  /\{isDownloadSectionExpanded \? \([\s\S]*DOWNLOAD_CATEGORIES[\s\S]*\) : null\}\s*<NavItem icon=\{<Gauge size=\{18\} \/>\} label="Active"/,
  'Active downloads should remain visible when the All Downloads category group is collapsed',
);

assert.match(
  source,
  /label="Active"[\s\S]*label="Completed"[\s\S]*<div className="mt-2 border-t border-border\/70 pt-2">/,
  'Completed downloads should remain visible with Active above the torrent section',
);

assert.match(
  source,
  /<div className="shrink-0 space-y-2">/,
  'the Settings footer should stay fixed below the scrollable sidebar navigation',
);

assert.match(
  source,
  /import \{ SettingsPage, SETTINGS_SECTIONS \} from '\.\/SettingsPage';/,
  'the app shell should consume the shared settings section list',
);

assert.match(
  source,
  /view === 'settings' \? \([\s\S]*<SettingsSidebar[\s\S]*onBack=\{\(\) => requestViewChange\('all'\)\}/,
  'settings mode should swap the download sidebar for a settings sidebar with guarded back navigation',
);

assert.match(
  source,
  /event\.key !== 'Escape'[\s\S]*view !== 'settings'[\s\S]*requestViewChange\('all'\)/,
  'Escape should leave settings through the guarded view-change path',
);

assert.match(
  source,
  /<SettingsPage[\s\S]*onCancel=\{\(\) => requestViewChange\('all'\)\}/,
  'Cancel should leave settings through the guarded view-change path',
);

assert.match(
  source,
  /const \[activeSettingsSectionId, setActiveSettingsSectionId\] = useState<SettingsSection\['id'\]>\(SETTINGS_SECTIONS\[0\]\.id\)/,
  'settings view should track the currently visible settings section',
);

assert.match(
  source,
  /setActiveSettingsSectionId[\s\S]*new IntersectionObserver\([\s\S]*updateActiveSectionFromScroll[\s\S]*SETTINGS_SECTIONS\.forEach[\s\S]*observer\.observe/,
  'settings view should update the active section from visible settings panels',
);

assert.match(
  source,
  /scrollRoot\.scrollTop \+ scrollRoot\.clientHeight >= scrollRoot\.scrollHeight[\s\S]*SETTINGS_SECTIONS\[SETTINGS_SECTIONS\.length - 1\]\.id/,
  'settings scroll-spy should highlight the final section when the settings content is scrolled to the bottom',
);

assert.match(
  source,
  /function settingsSectionIcon\([\s\S]*case 'general'[\s\S]*<Settings2[\s\S]*case 'updates'[\s\S]*<Download[\s\S]*case 'torrenting'[\s\S]*<Gauge[\s\S]*case 'appearance'[\s\S]*<Palette[\s\S]*case 'extension'[\s\S]*<PlugZap[\s\S]*case 'native-host'[\s\S]*<Wrench/,
  'settings sidebar should map shared section icon names to lucide icons',
);

assert.match(
  source,
  /function SettingsSidebar\(\{[\s\S]*activeSectionId[\s\S]*onSectionClick[\s\S]*SETTINGS_SECTIONS\.map[\s\S]*const active = section\.id === activeSectionId[\s\S]*href=\{section\.href\}[\s\S]*onClick=\{\(\) => onSectionClick\(section\.id\)\}/,
  'settings sidebar should render active icon-backed links from shared settings sections',
);

assert.match(
  source,
  /active \? 'bg-primary-soft text-primary shadow-\[inset_3px_0_0_var\(--color-primary\)\]'[\s\S]*settingsSectionIcon\(section\.iconName/,
  'active settings sidebar links should use the selected nav treatment and brighter icons',
);

assert.match(
  source,
  /section\.description/,
  'settings sidebar should include short supporting text for each section',
);

assert.match(
  source,
  /<button[\s\S]*aria-label="Back to downloads"[\s\S]*<ChevronLeft[\s\S]*Back/,
  'settings sidebar should expose a back button at the top',
);

assert.doesNotMatch(
  source,
  /label="Needs Attention"/,
  'the sidebar should not render separate Needs Attention filters',
);

assert.doesNotMatch(
  source,
  /label="Queued"/,
  'the sidebar should not render separate Queued filters',
);

assert.doesNotMatch(
  source,
  /return '(?:attention|queued|torrent-attention|torrent-queued)'/,
  'the toolbar filter cycle should skip filters that are no longer visible in the sidebar',
);

assert.doesNotMatch(
  source,
  /setView\(outcome\.mode === 'torrent' \? 'torrent-queued' : 'queued'\)|setView\('queued'\)/,
  'new downloads should not navigate to removed queued-only views',
);
