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
