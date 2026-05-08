import assert from 'node:assert/strict';
import { groupBulkQueueRows, isBulkAggregateJob } from '../src/bulkQueueRows.ts';
import { filterJobsForView, getQueueCounts } from '../src/downloadViews.ts';
import type { DownloadJob } from '../src/types.ts';

const baseJob: DownloadJob = {
  id: 'job_1',
  url: 'https://example.com/file.zip',
  filename: 'file.zip',
  transferKind: 'http',
  state: 'queued',
  progress: 0,
  totalBytes: 0,
  downloadedBytes: 0,
  speed: 0,
  eta: 0,
};

function job(update: Partial<DownloadJob>): DownloadJob {
  return { ...baseJob, ...update };
}

const bulkArchive = {
  id: 'bulk_1',
  name: 'bulk-download.zip',
  archiveStatus: 'pending' as const,
};

const rows = groupBulkQueueRows([
  job({
    id: 'part_1',
    filename: 'Game.part01.rar',
    url: 'https://dl.fuckingfast.co/dl/one',
    state: 'completed',
    progress: 100,
    totalBytes: 500,
    downloadedBytes: 500,
    speed: 0,
    bulkArchive,
  }),
  job({
    id: 'part_2',
    filename: 'Game.part02.rar',
    url: 'https://dl.fuckingfast.co/dl/two',
    state: 'downloading',
    progress: 50,
    totalBytes: 500,
    downloadedBytes: 250,
    speed: 25,
    eta: 10,
    bulkArchive,
  }),
  job({
    id: 'loose',
    filename: 'guide.pdf',
    state: 'completed',
    progress: 100,
    totalBytes: 100,
    downloadedBytes: 100,
  }),
]);

assert.equal(rows.length, 2, 'bulk archive members should collapse into one visible queue row');
assert.equal(rows[0].filename, 'bulk-download.zip', 'aggregate row should use the archive filename');
assert.equal(isBulkAggregateJob(rows[0]), true, 'aggregate row should expose bulk metadata');

const aggregate = rows[0];
assert.equal(aggregate.totalBytes, 1_000, 'aggregate total size should be the sum of members');
assert.equal(aggregate.downloadedBytes, 750, 'aggregate downloaded bytes should be the sum of members');
assert.equal(aggregate.progress, 75, 'aggregate progress should be derived from summed bytes');
assert.equal(aggregate.speed, 25, 'aggregate speed should sum active member speeds');
assert.equal(aggregate.state, 'downloading', 'aggregate should remain active while any member is active');

if (!isBulkAggregateJob(aggregate)) {
  throw new Error('aggregate should be narrowed for metadata assertions');
}
assert.deepEqual(aggregate.bulkMemberIds, ['part_1', 'part_2']);
assert.equal(aggregate.bulkArchiveId, 'bulk_1');

assert.deepEqual(
  getQueueCounts(rows),
  {
    all: 2,
    active: 1,
    attention: 0,
    queued: 0,
    completed: 1,
    categories: {
      document: 1,
      program: 0,
      picture: 0,
      video: 0,
      compressed: 1,
      music: 0,
      other: 0,
    },
    torrents: {
      all: 0,
      active: 0,
      seeding: 0,
      attention: 0,
      queued: 0,
      completed: 0,
    },
  },
  'counts should treat the bulk archive as one regular compressed download',
);

assert.deepEqual(
  filterJobsForView(rows, 'all', 'part02').map((row) => row.id),
  [aggregate.id],
  'search should match filenames from collapsed bulk members',
);

const completedRows = groupBulkQueueRows([
  job({
    id: 'part_3',
    filename: 'Game.part01.rar',
    state: 'completed',
    progress: 100,
    totalBytes: 500,
    downloadedBytes: 500,
    targetPath: 'C:\\Downloads\\Game.part01.rar',
    bulkArchive: {
      id: 'bulk_2',
      name: 'bulk-download.zip',
      archiveStatus: 'completed',
      outputPath: 'C:\\Downloads\\bulk-download.zip',
    },
  }),
  job({
    id: 'part_4',
    filename: 'Game.part02.rar',
    state: 'completed',
    progress: 100,
    totalBytes: 500,
    downloadedBytes: 500,
    targetPath: 'C:\\Downloads\\Game.part02.rar',
    bulkArchive: {
      id: 'bulk_2',
      name: 'bulk-download.zip',
      archiveStatus: 'completed',
      outputPath: 'C:\\Downloads\\bulk-download.zip',
    },
  }),
]);

assert.equal(completedRows[0].state, 'completed', 'completed archive rows should represent the archive status');
assert.equal(completedRows[0].targetPath, 'C:\\Downloads\\bulk-download.zip', 'completed aggregate target should be the archive output path');

const failedMemberRows = groupBulkQueueRows([
  job({
    id: 'part_failed',
    filename: 'Game.part01.rar',
    state: 'failed',
    error: 'HTTP 403',
    failureCategory: 'http',
    bulkArchive,
  }),
  job({
    id: 'part_completed',
    filename: 'Game.part02.rar',
    state: 'completed',
    progress: 100,
    totalBytes: 500,
    downloadedBytes: 500,
    bulkArchive,
  }),
]);

if (!isBulkAggregateJob(failedMemberRows[0])) {
  throw new Error('failed member aggregate should expose bulk metadata');
}
assert.equal(
  failedMemberRows[0].bulkRetryableMemberCount,
  1,
  'aggregate rows should count failed pending HTTP members that can be retried',
);

const failedArchiveRows = groupBulkQueueRows([
  job({
    id: 'part_done_1',
    filename: 'Game.part01.rar',
    state: 'completed',
    progress: 100,
    totalBytes: 500,
    downloadedBytes: 500,
    targetPath: 'C:\\Downloads\\Game.part01.rar',
    bulkArchive: {
      id: 'bulk_failed_archive',
      name: 'Game.zip',
      archiveStatus: 'failed',
      outputPath: 'C:\\Downloads\\Game.zip',
      error: 'Bulk archive finalization was interrupted by app shutdown.',
    },
  }),
  job({
    id: 'part_done_2',
    filename: 'Game.part02.rar',
    state: 'completed',
    progress: 100,
    totalBytes: 500,
    downloadedBytes: 500,
    targetPath: 'C:\\Downloads\\Game.part02.rar',
    bulkArchive: {
      id: 'bulk_failed_archive',
      name: 'Game.zip',
      archiveStatus: 'failed',
      outputPath: 'C:\\Downloads\\Game.zip',
      error: 'Bulk archive finalization was interrupted by app shutdown.',
    },
  }),
]);

if (!isBulkAggregateJob(failedArchiveRows[0])) {
  throw new Error('failed archive aggregate should expose bulk metadata');
}
assert.equal(
  failedArchiveRows[0].bulkArchiveFixable,
  true,
  'failed bulk archives with every member completed should expose a Fix archive action',
);
