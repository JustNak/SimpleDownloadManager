import assert from 'node:assert/strict';
import {
  clampQueueProgress,
  formatQueueSize,
  QUEUE_TABLE_COLUMNS,
  queueTableColumnsForView,
  queueStatusPresentation,
  shouldShowNameProgress,
  torrentDetailMetrics,
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
  formatQueueSize({ ...baseJob, state: 'completed', downloadedBytes: 100, totalBytes: 100 }, byteLabel),
  '100 B',
  'completed downloads should show only their final total size',
);
