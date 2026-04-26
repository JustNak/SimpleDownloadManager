import assert from 'node:assert/strict';
import {
  compareDownloadsForSort,
  nextSortModeForColumn,
  sortModeDirection,
  sortModeKey,
} from '../src/downloadSorting.ts';
import type { DownloadJob } from '../src/types.ts';

const baseJob: DownloadJob = {
  id: 'job_1',
  url: 'https://example.com/file.zip',
  filename: 'file.zip',
  transferKind: 'http',
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
  [oldest, newest].sort((a, b) => compareDownloadsForSort(a, b, 'date:desc')).map((job) => job.id),
  ['job_2', 'job_1'],
  'date descending sort should put the most recently created job first',
);

assert.deepEqual(
  [newest, oldest].sort((a, b) => compareDownloadsForSort(a, b, 'date:asc')).map((job) => job.id),
  ['job_1', 'job_2'],
  'date ascending sort should put the oldest created job first',
);

assert.deepEqual(
  [undated, newest].sort((a, b) => compareDownloadsForSort(a, b, 'date:desc')).map((job) => job.id),
  ['job_2', 'job_3'],
  'undated jobs should sort after dated jobs in date order',
);

assert.deepEqual(
  [
    { ...baseJob, id: 'completed', filename: 'completed.zip', state: 'completed' },
    { ...baseJob, id: 'seeding', filename: 'seeding.iso', transferKind: 'torrent', state: 'seeding' },
    { ...baseJob, id: 'queued', filename: 'queued.zip', state: 'queued' },
  ].sort((a, b) => compareDownloadsForSort(a, b, 'name:asc')).map((job) => job.id),
  ['completed', 'queued', 'seeding'],
  'name sorting should be driven by the header sort mode',
);

assert.deepEqual(
  [
    { ...baseJob, id: 'small', filename: 'small.zip', totalBytes: 10 },
    { ...baseJob, id: 'large', filename: 'large.zip', totalBytes: 1000 },
  ].sort((a, b) => compareDownloadsForSort(a, b, 'size:desc')).map((job) => job.id),
  ['large', 'small'],
  'size descending should put larger downloads first',
);

assert.equal(sortModeKey('date:asc'), 'date');
assert.equal(sortModeDirection('date:asc'), 'asc');
assert.equal(nextSortModeForColumn('date:desc', 'date'), 'date:asc');
assert.equal(nextSortModeForColumn('date:asc', 'name'), 'name:asc');
assert.equal(nextSortModeForColumn('name:asc', 'size'), 'size:desc');
