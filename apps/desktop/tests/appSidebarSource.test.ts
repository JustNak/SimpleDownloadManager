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
  /<div className="shrink-0 space-y-2">/,
  'the Settings footer should stay fixed below the scrollable sidebar navigation',
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
