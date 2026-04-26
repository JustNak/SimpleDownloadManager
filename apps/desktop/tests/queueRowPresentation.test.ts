import assert from 'node:assert/strict';
import {
  clampQueueProgress,
  QUEUE_TABLE_COLUMNS,
  queueStatusPresentation,
  shouldShowNameProgress,
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
