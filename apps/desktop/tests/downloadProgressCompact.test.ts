import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const progressSource = readFileSync(new URL('../src/DownloadProgressWindow.tsx', import.meta.url), 'utf8');
const backendSource = readFileSync(new URL('../src/backend.ts', import.meta.url), 'utf8');
const windowsSource = readFileSync(new URL('../src-tauri/src/windows.rs', import.meta.url), 'utf8');

assert.match(
  progressSource,
  /job\.transferKind === 'torrent'\s*\?\s*<TorrentingProgressView/,
  'download progress controller should dispatch torrent jobs to TorrentingProgressView',
);

assert.match(
  progressSource,
  /function CompactDownloadProgressView/,
  'normal downloads should render through a named compact progress view',
);

assert.match(
  progressSource,
  /function TorrentingProgressView/,
  'torrent jobs should render through a dedicated TorrentingProgressView',
);

assert.match(
  progressSource,
  /<ProgressShell title="Torrenting">[\s\S]*function ProgressShell[\s\S]*<PopupTitlebar title=\{title\} \/>/,
  'torrent progress popup should be titled Torrenting',
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

assert.match(
  progressSource,
  /<MetricRail>[\s\S]*label="Speed"[\s\S]*formatTime\(progressMetrics\.timeRemaining\)[\s\S]*label="Peers"[\s\S]*<\/MetricRail>/,
  'torrenting view should show download speed, ETA, and peers in a flat metric rail',
);

assert.match(
  progressSource,
  /const metadataPending = isTorrentMetadataPending\(job\)/,
  'torrenting view should branch on metadata-pending state',
);

assert.match(
  progressSource,
  /\{!metadataPending \? \([\s\S]*<ProgressStrip[\s\S]*<MetricRail>[\s\S]*label="Peers"[\s\S]*<\/MetricRail>[\s\S]*\) : null\}/,
  'torrenting view should hide progress and metrics while metadata is pending',
);

assert.match(
  progressSource,
  /progressLabel=\{`Verified \$\{progress\.toFixed\(0\)\}%`\}[\s\S]*bytesText=\{verifiedTorrentText\(job\)\}/,
  'torrenting progress strip should label checked torrent bytes as verified content',
);

assert.match(
  progressSource,
  /<TorrentDownloadedRow job=\{job\} \/>/,
  'torrenting popup should show a separate peer-fetched downloaded byte row',
);

assert.match(
  progressSource,
  /function TorrentDownloadedRow[\s\S]*torrentFetchedText\(job\)[\s\S]*Downloaded/,
  'torrent downloaded row should use the cumulative peer-fetched byte counter',
);

assert.match(
  progressSource,
  /function verifiedTorrentText[\s\S]*formatTorrentVerifiedSize\(job, formatBytes\)/,
  'torrent verified text should make progress-byte semantics explicit',
);

assert.match(
  progressSource,
  /function torrentFetchedText[\s\S]*formatTorrentFetchedSize\(job, formatBytes\)/,
  'torrent fetched text should show peer-downloaded bytes instead of checked progress bytes',
);

assert.doesNotMatch(
  progressSource,
  /label="Uploaded"|label="Ratio"/,
  'torrenting popup should omit upload and ratio metrics while downloading',
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

assert.match(
  progressSource,
  /torrentDisplayName\(job\)/,
  'torrenting view should use torrent metadata display names',
);

assert.match(
  progressSource,
  /Finding metadata/,
  'torrenting view should preserve the metadata-pending state label',
);

assert.doesNotMatch(
  progressSource,
  /\$\{torrentPeerCount\(job\)\}\/--|\/--/,
  'torrenting popup should display peer count without an unknown denominator',
);

assert.match(
  windowsSource,
  /width:\s*460\.0,[\s\S]*height:\s*250\.0,/,
  'single progress popup geometry should be compact at 460x250',
);

assert.match(
  backendSource,
  /window\.open\(\s*popupUrl\(`\?window=download-progress&jobId=\$\{encodeURIComponent\(id\)\}`\),\s*`download-progress-\$\{id\}`,\s*'width=460,height=250'\s*\)/,
  'browser fallback progress popup should use the same compact 460x250 geometry',
);
