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
