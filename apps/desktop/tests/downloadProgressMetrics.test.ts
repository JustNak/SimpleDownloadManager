import assert from 'node:assert/strict';
import {
  calculateDownloadProgressMetricsByJobId,
  calculateDownloadProgressMetrics,
  recordProgressSample,
  shouldShowCompletedFileAction,
} from '../src/downloadProgressMetrics.ts';
import type { DownloadJob } from '../src/types.ts';

const baseJob: DownloadJob = {
  id: 'job_1',
  url: 'https://example.com/file.zip',
  filename: 'file.zip',
  transferKind: 'http',
  state: 'downloading',
  createdAt: 1_000,
  progress: 0,
  totalBytes: 31_000,
  downloadedBytes: 1_000,
  speed: 80_000,
  eta: 1,
  targetPath: 'C:\\Downloads\\file.zip',
};

const metadataPendingTorrent: DownloadJob = {
  ...baseJob,
  id: 'torrent_1',
  url: 'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Movie',
  filename: 'Movie',
  transferKind: 'torrent',
  state: 'downloading',
  progress: 0,
  totalBytes: 0,
  downloadedBytes: 0,
  speed: 0,
  eta: 0,
  targetPath: 'C:\\Downloads\\Movie',
  torrent: {
    peers: 2,
  },
};

const initialSamples = recordProgressSample([], baseJob, 1_000);
const averageSamples = recordProgressSample(
  initialSamples,
  { ...baseJob, downloadedBytes: 11_000, speed: 80_000 },
  11_000,
);

assert.deepEqual(
  calculateDownloadProgressMetrics({ ...baseJob, downloadedBytes: 11_000, speed: 80_000 }, averageSamples, 11_000),
  {
    averageSpeed: 1_000,
    timeRemaining: 20,
  },
  'progress metrics should use average observed byte rate and derive Time from that average',
);

assert.deepEqual(
  calculateDownloadProgressMetricsByJobId(
    [
      { ...baseJob, id: 'job_1', downloadedBytes: 11_000, speed: 80_000 },
      { ...baseJob, id: 'job_2', downloadedBytes: 10_000, speed: 2_000 },
    ],
    averageSamples,
    11_000,
  ),
  {
    job_1: {
      averageSpeed: 1_000,
      timeRemaining: 20,
    },
    job_2: {
      averageSpeed: 2_000,
      timeRemaining: 11,
    },
  },
  'queue progress metrics should be available by job id for table rows',
);

assert.deepEqual(
  calculateDownloadProgressMetrics({ ...baseJob, downloadedBytes: 10_000, speed: 2_000 }, [], 6_000),
  {
    averageSpeed: 2_000,
    timeRemaining: 11,
  },
  'progress metrics should fall back to backend speed when no average sample is available',
);

assert.deepEqual(
  calculateDownloadProgressMetrics({ ...baseJob, downloadedBytes: 10_000, speed: 80_000 }, [], 6_000),
  {
    averageSpeed: 80_000,
    timeRemaining: 1,
  },
  'backend speed should be preferred over lifetime average when no observed sample is available',
);

assert.deepEqual(
  calculateDownloadProgressMetrics({ ...baseJob, downloadedBytes: 10_000, speed: 0 }, [], 6_000),
  {
    averageSpeed: 0,
    timeRemaining: 0,
  },
  'progress metrics should not invent active speed from createdAt when backend speed and observed samples are unavailable',
);

const metadataSamples = recordProgressSample(averageSamples, metadataPendingTorrent, 12_000);
assert.deepEqual(
  metadataSamples.filter((sample) => sample.jobId === metadataPendingTorrent.id),
  [],
  'metadata-pending torrents should not record download samples',
);

const resolvedTorrentSamples = recordProgressSample(
  metadataSamples,
  {
    ...metadataPendingTorrent,
    totalBytes: 100_000,
    downloadedBytes: 50_000,
    progress: 50,
    speed: 0,
    torrent: {
      ...metadataPendingTorrent.torrent,
      infoHash: '0123456789abcdef0123456789abcdef01234567',
      name: 'Movie',
      totalFiles: 1,
    },
  },
  13_000,
);
assert.deepEqual(
  calculateDownloadProgressMetrics(
    {
      ...metadataPendingTorrent,
      totalBytes: 100_000,
      downloadedBytes: 50_000,
      progress: 50,
      speed: 0,
      torrent: {
        ...metadataPendingTorrent.torrent,
        infoHash: '0123456789abcdef0123456789abcdef01234567',
        name: 'Movie',
        totalFiles: 1,
      },
    },
    resolvedTorrentSamples,
    13_000,
  ),
  {
    averageSpeed: 0,
    timeRemaining: 0,
  },
  'first metadata-resolved torrent snapshot should become the baseline instead of fake download speed',
);

const torrentWithResolvedMetadata: DownloadJob = {
  ...metadataPendingTorrent,
  totalBytes: 2_147_483_648,
  downloadedBytes: 1_000,
  progress: 0.01,
  speed: 262_144,
  torrent: {
    ...metadataPendingTorrent.torrent,
    infoHash: '0123456789abcdef0123456789abcdef01234567',
    name: 'Movie',
    totalFiles: 1,
  },
};
const torrentBaselineSamples = recordProgressSample(
  metadataSamples,
  torrentWithResolvedMetadata,
  14_000,
);
const torrentAfterProgressJump = {
  ...torrentWithResolvedMetadata,
  downloadedBytes: 1_073_742_824,
  progress: 50,
  speed: 262_144,
};
const torrentJumpSamples = recordProgressSample(
  torrentBaselineSamples,
  torrentAfterProgressJump,
  15_000,
);
assert.deepEqual(
  calculateDownloadProgressMetrics(torrentAfterProgressJump, torrentJumpSamples, 15_000),
  {
    averageSpeed: 262_144,
    timeRemaining: 4_096,
  },
  'torrent metrics should use backend live speed instead of treating metadata progress jumps as throughput',
);

assert.equal(
  shouldShowCompletedFileAction({ ...baseJob, state: 'downloading' }),
  false,
  'Show should not appear while a download is active',
);

assert.equal(
  shouldShowCompletedFileAction({ ...baseJob, state: 'completed' }),
  true,
  'Show should appear when a completed download has a target path',
);

assert.equal(
  shouldShowCompletedFileAction({ ...baseJob, state: 'completed', targetPath: '' }),
  false,
  'Show should stay hidden when there is no completed file path',
);
