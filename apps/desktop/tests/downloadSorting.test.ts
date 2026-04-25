import assert from 'node:assert/strict';
import { compareDownloadsForSort } from '../src/downloadSorting.ts';
import type { DownloadJob } from '../src/types.ts';

const baseJob: DownloadJob = {
  id: 'job_1',
  url: 'https://example.com/file.zip',
  filename: 'file.zip',
  state: 'completed',
  progress: 100,
  totalBytes: 100,
  downloadedBytes: 100,
  speed: 0,
  eta: 0,
  targetPath: 'C:\\Downloads\\file.zip',
};

const oldest = { ...baseJob, id: 'job_1', filename: 'alpha.zip', createdAt: 1_700_000_000_000 };
const newest = { ...baseJob, id: 'job_2', filename: 'beta.zip', createdAt: 1_800_000_000_000 };
const undated = { ...baseJob, id: 'job_3', filename: 'gamma.zip' };

assert.deepEqual(
  [oldest, newest].sort((a, b) => compareDownloadsForSort(a, b, 'newest')).map((job) => job.id),
  ['job_2', 'job_1'],
  'newest sort should put the most recently created job first',
);

assert.deepEqual(
  [newest, oldest].sort((a, b) => compareDownloadsForSort(a, b, 'oldest')).map((job) => job.id),
  ['job_1', 'job_2'],
  'oldest sort should put the oldest created job first',
);

assert.deepEqual(
  [undated, newest].sort((a, b) => compareDownloadsForSort(a, b, 'newest')).map((job) => job.id),
  ['job_2', 'job_3'],
  'undated jobs should sort after dated jobs in newest order',
);
