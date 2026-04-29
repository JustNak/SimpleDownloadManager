import assert from 'node:assert/strict';
import {
  clampQueueProgress,
  fileBadgeActivityState,
  formatQueueSize,
  formatQueueSizeTitle,
  formatTorrentFetchedSize,
  formatTorrentVerifiedSize,
  isTorrentSeedingRestore,
  QUEUE_TABLE_COLUMNS,
  queueTableColumnsForView,
  queueStatusPresentation,
  shouldShowNameProgress,
  torrentActivitySummary,
  torrentDetailMetrics,
  torrentDisplayName,
} from '../src/queueRowPresentation.ts';
import type { DownloadJob } from '../src/types.ts';

const baseJob: DownloadJob = {
  id: 'job_1',
  url: 'https://example.com/file.zip',
  filename: 'file.zip',
  transferKind: 'http',
  state: 'downloading',
  progress: 40,
  totalBytes: 100,
  downloadedBytes: 40,
  speed: 1024,
  eta: 60,
};

assert.deepEqual(
  QUEUE_TABLE_COLUMNS,
  ['Name', 'Date', 'Speed', 'Time', 'Size', 'Actions'],
  'queue table should remove the dedicated Progress column',
);

assert.deepEqual(
  queueTableColumnsForView('all'),
  ['Name', 'Date', 'Speed', 'Time', 'Size', 'Actions'],
  'regular download views should keep the normal speed and time columns',
);

assert.deepEqual(
  queueTableColumnsForView('torrents'),
  ['Name', 'Date', 'Seed', 'Ratio', 'Size', 'Actions'],
  'torrent views should expose upload seeding and ratio columns instead of the normal speed/time pair',
);

assert.deepEqual(
  queueTableColumnsForView('torrent-seeding'),
  ['Name', 'Date', 'Seed', 'Ratio', 'Size', 'Actions'],
  'torrent status filters should keep the torrent-specific table header',
);

assert.equal(
  shouldShowNameProgress({ ...baseJob, state: 'downloading', progress: 40 }),
  true,
  'active downloads should show progress over the filename area',
);

assert.equal(
  shouldShowNameProgress({ ...baseJob, state: 'completed', progress: 100 }),
  false,
  'completed downloads should remove the inline progress bar entirely',
);

assert.equal(
  shouldShowNameProgress({ ...baseJob, state: 'failed', progress: 22 }),
  false,
  'failed downloads should rely on the Error status badge instead of showing progress',
);

assert.equal(
  shouldShowNameProgress({ ...baseJob, state: 'queued', progress: 10 }),
  false,
  'queued downloads should not show a progress overlay',
);

assert.equal(
  fileBadgeActivityState({ ...baseJob, state: 'downloading' }, false),
  'buffering',
  'active downloading rows should show the buffering file badge overlay',
);

assert.equal(
  fileBadgeActivityState({ ...baseJob, state: 'starting' }, false),
  'buffering',
  'starting rows should show the buffering file badge overlay',
);

assert.equal(
  fileBadgeActivityState({ ...baseJob, state: 'completed' }, true),
  'completed',
  'rows that just completed should show a transient completed file badge overlay',
);

assert.equal(
  fileBadgeActivityState({ ...baseJob, state: 'completed' }, false),
  'none',
  'historically completed rows should not keep a file badge overlay',
);

assert.equal(clampQueueProgress(-20), 0);
assert.equal(clampQueueProgress(64.4), 64.4);
assert.equal(clampQueueProgress(140), 100);

assert.deepEqual(
  queueStatusPresentation({ ...baseJob, state: 'downloading' }),
  { label: 'Downloading', tone: 'primary' },
);

assert.deepEqual(
  queueStatusPresentation({ ...baseJob, state: 'completed' }),
  { label: 'Done', tone: 'success' },
);

assert.deepEqual(
  queueStatusPresentation({ ...baseJob, state: 'failed' }),
  { label: 'Error', tone: 'destructive' },
);

assert.deepEqual(
  queueStatusPresentation({
    ...baseJob,
    transferKind: 'torrent',
    state: 'seeding',
    torrent: { uploadedBytes: 2048, ratio: 2.0 },
  }),
  { label: 'Seeding', tone: 'primary' },
);

assert.equal(
  isTorrentSeedingRestore({
    ...baseJob,
    transferKind: 'torrent',
    state: 'starting',
    torrent: { uploadedBytes: 2048, ratio: 2.0, seedingStartedAt: 123_456 },
  }),
  true,
  'active torrents with a prior seeding timestamp should be treated as seeding restores',
);

assert.deepEqual(
  queueStatusPresentation({
    ...baseJob,
    transferKind: 'torrent',
    state: 'downloading',
    torrent: { uploadedBytes: 2048, ratio: 2.0, seedingStartedAt: 123_456 },
  }),
  { label: 'Restoring seeding', tone: 'warning' },
  'seeding restore torrents should not be labeled as fresh downloads',
);

assert.deepEqual(
  queueStatusPresentation({
    ...baseJob,
    transferKind: 'torrent',
    state: 'starting',
    totalBytes: 0,
    torrent: { uploadedBytes: 0, ratio: 0 },
  }),
  { label: 'Finding', tone: 'warning' },
  'metadata-pending torrents should show a specific Finding badge instead of Downloading',
);

assert.equal(
  torrentActivitySummary({
    ...baseJob,
    transferKind: 'torrent',
    state: 'starting',
    totalBytes: 0,
    torrent: { uploadedBytes: 2048, ratio: 2.0, seedingStartedAt: 123_456 },
  }),
  'Restoring seeding',
  'seeding restores should describe reseed recovery before metadata lookup wording',
);

assert.equal(
  torrentActivitySummary({
    ...baseJob,
    transferKind: 'torrent',
    state: 'starting',
    totalBytes: 0,
    torrent: { uploadedBytes: 0, ratio: 0 },
  }),
  'Finding metadata',
  'unresolved starting torrents should describe active metadata lookup',
);

assert.equal(
  torrentActivitySummary({
    ...baseJob,
    transferKind: 'torrent',
    state: 'seeding',
    totalBytes: 100,
    torrent: { infoHash: '420f3778a160fbe6eb0a67c8470256be13b0ecc8', uploadedBytes: 0, ratio: 0 },
  }),
  'No peer activity yet',
  'resolved seeding torrents without peer metrics should not be described as metadata pending',
);

assert.equal(
  torrentDisplayName({
    ...baseJob,
    filename: 'fallback-name',
    transferKind: 'torrent',
    torrent: { name: 'Torrent Name', infoHash: '420f3778a160fbe6eb0a67c8470256be13b0ecc8', uploadedBytes: 0, ratio: 0 },
  }),
  'Torrent Name',
);

assert.equal(
  torrentDisplayName({
    ...baseJob,
    filename: 'fallback-name',
    transferKind: 'torrent',
    torrent: { infoHash: '420f3778a160fbe6eb0a67c8470256be13b0ecc8', uploadedBytes: 0, ratio: 0 },
  }),
  'fallback-name',
);

assert.equal(
  torrentDisplayName({
    ...baseJob,
    filename: 'magnet-fallback',
    transferKind: 'torrent',
    state: 'starting',
    totalBytes: 0,
    torrent: { infoHash: '420f3778a160fbe6eb0a67c8470256be13b0ecc8', uploadedBytes: 0, ratio: 0 },
  }),
  'magnet-fallback',
  'details should use the filename fallback even while metadata is still unresolved',
);

assert.equal(
  torrentDisplayName({
    ...baseJob,
    filename: '',
    transferKind: 'torrent',
    torrent: { infoHash: '420f3778a160fbe6eb0a67c8470256be13b0ecc8', uploadedBytes: 0, ratio: 0 },
  }),
  'Torrent 420f3778a160',
);

assert.deepEqual(
  torrentDetailMetrics({
    ...baseJob,
    transferKind: 'torrent',
    state: 'seeding',
    torrent: { uploadedBytes: 2048, ratio: 2.0, peers: 9, seeds: 46 },
  }),
  [
    { kind: 'upload', label: 'Uploaded', value: 2048 },
    { kind: 'peers', label: 'Peers', value: 9 },
    { kind: 'seeds', label: 'Seeds', value: 46 },
  ],
  'torrent row metrics should omit ratio and keep upload/peer/seed indicators separate',
);

const byteLabel = (bytes: number) => `${bytes} B`;

assert.equal(
  formatQueueSize({ ...baseJob, state: 'downloading', downloadedBytes: 40, totalBytes: 100 }, byteLabel),
  '40 B / 100 B',
  'active downloads should show downloaded and total size',
);

assert.equal(
  formatQueueSize({ ...baseJob, transferKind: 'torrent', state: 'downloading', downloadedBytes: 40, totalBytes: 100 }, byteLabel),
  '100 B',
  'torrent size cells should show total size instead of verified bytes over total',
);

const torrentWithCheckedProgressJump: DownloadJob = {
  ...baseJob,
  transferKind: 'torrent',
  downloadedBytes: 1_700_000_000,
  totalBytes: 2_700_000_000,
  torrent: {
    uploadedBytes: 0,
    fetchedBytes: 0,
    ratio: 0,
  },
};

assert.equal(
  formatTorrentVerifiedSize(torrentWithCheckedProgressJump, byteLabel),
  'Verified 1700000000 B / 2700000000 B',
  'torrent verified size should label checked progress jumps explicitly',
);

assert.equal(
  formatTorrentFetchedSize(torrentWithCheckedProgressJump, byteLabel),
  '0 B / 2700000000 B from peers',
  'torrent fetched size should not present checked progress jumps as peer downloads',
);

assert.equal(
  formatQueueSizeTitle(torrentWithCheckedProgressJump, byteLabel),
  'Verified 1700000000 B / 2700000000 B; Downloaded 0 B / 2700000000 B from peers',
  'torrent size tooltip should expose verified progress separately from peer-fetched bytes',
);

assert.equal(
  formatQueueSize({ ...baseJob, state: 'completed', downloadedBytes: 100, totalBytes: 100 }, byteLabel),
  '100 B',
  'completed downloads should show only their final total size',
);
