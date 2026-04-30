import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const progressSource = readFileSync(new URL('../src/DownloadProgressWindow.tsx', import.meta.url), 'utf8');
const backendMockUrl = new URL('../src/backendMock.ts', import.meta.url);
const windowsSource = readFileSync(new URL('../src-tauri/src/windows.rs', import.meta.url), 'utf8');

assert.ok(
  existsSync(backendMockUrl),
  'browser-preview mock behavior should live in backendMock.ts',
);

const backendMockSource = readFileSync(backendMockUrl, 'utf8');

assert.match(
  progressSource,
  /return <CompactDownloadProgressView \{\.\.\.sharedProps\} \/>/,
  'download progress controller should render only the normal compact progress view',
);

assert.match(
  progressSource,
  /function CompactDownloadProgressView/,
  'normal downloads should render through a named compact progress view',
);

assert.doesNotMatch(
  progressSource,
  /TorrentingProgressView|<ProgressShell title="Torrenting">/,
  'torrent-specific progress UI should move out of the shared download progress window',
);

assert.match(
  progressSource,
  /<ProgressShell title="Download progress">/,
  'normal progress popup should keep the Download progress title',
);

assert.match(
  progressSource,
  /<MetricRail>[\s\S]*label="Speed"[\s\S]*formatTime\(progressMetrics\.timeRemaining\)[\s\S]*label="Size"[\s\S]*<\/MetricRail>/,
  'compact download view should keep speed, ETA, and size metrics visible in a flat metric rail',
);

assert.doesNotMatch(
  progressSource,
  /label="Uploaded"|label="Ratio"/,
  'normal download progress popup should not include torrent upload or ratio metrics',
);

assert.doesNotMatch(
  progressSource,
  /function MetricGrid|rounded border border-border bg-background/,
  'progress popup metrics should be flat instead of boxed cards',
);

assert.doesNotMatch(
  progressSource,
  /border-y border-border\/70/,
  'progress popup metric rail should avoid strong hairline separators around the metrics',
);

assert.match(
  progressSource,
  /function MetricRail[\s\S]*bg-background\/30[\s\S]*border-t border-border\/35/,
  'progress popup metric rail should use softer boxed-in shading and a muted top separator',
);

assert.doesNotMatch(
  progressSource,
  /\$\{torrentPeerCount\(job\)\}\/--|\/--/,
  'normal download progress popup should not render torrent peer ratios',
);

assert.match(
  windowsSource,
  /width:\s*460\.0,[\s\S]*height:\s*280\.0,/,
  'download progress popup geometry should stay compact for normal downloads',
);

assert.match(
  backendMockSource,
  /window\.open\(\s*popupUrl\(`\?window=download-progress&jobId=\$\{encodeURIComponent\(id\)\}`\),\s*`download-progress-\$\{id\}`,\s*'width=460,height=280'\s*\)/,
  'browser fallback download progress popup should keep the compact 460x280 geometry',
);
