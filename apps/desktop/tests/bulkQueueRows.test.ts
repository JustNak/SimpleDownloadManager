import assert from 'node:assert/strict';
import { groupBulkMembersByArchiveId, groupBulkQueueRows, isBulkAggregateJob } from '../src/bulkQueueRows.ts';
import { filterJobsForView, getQueueCounts } from '../src/downloadViews.ts';
import { queueStatusPresentation } from '../src/queueRowPresentation.ts';
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

const partOne = job({
  id: 'part_1',
  filename: 'Game.part01.rar',
  url: 'https://dl.fuckingfast.co/dl/one',
  state: 'completed',
  progress: 100,
  totalBytes: 500,
  downloadedBytes: 500,
  speed: 0,
  bulkArchive,
});
const partTwo = job({
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
});
const looseJob = job({
  id: 'loose',
  filename: 'guide.pdf',
  state: 'completed',
  progress: 100,
  totalBytes: 100,
  downloadedBytes: 100,
});

const rows = groupBulkQueueRows([partOne, partTwo, looseJob]);
const membersByArchiveId = groupBulkMembersByArchiveId([partOne, partTwo, looseJob]);

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
  membersByArchiveId.bulk_1.map((member) => member.id),
  ['part_1', 'part_2'],
  'bulk member lookup should expose member data for inline expansion',
);
assert.equal(membersByArchiveId.bulk_1[0], partOne, 'bulk member lookup should keep original job object references');
assert.equal('bulkMembers' in aggregate, false, 'aggregate rows should not clone member data');
assert.equal('bulkArchiveMemberSearchText' in aggregate, false, 'aggregate rows should not store always-built member search text');

const outOfOrderRows = groupBulkQueueRows([
  job({ id: 'part_19', filename: 'WWE_2K25.part019.rar', bulkArchive }),
  job({ id: 'part_17', filename: 'WWE_2K25.part017.rar', bulkArchive }),
  job({ id: 'part_18', filename: 'WWE_2K25.part018.rar', bulkArchive }),
]);
const outOfOrderMembers = groupBulkMembersByArchiveId([
  job({ id: 'part_19', filename: 'WWE_2K25.part019.rar', bulkArchive }),
  job({ id: 'part_17', filename: 'WWE_2K25.part017.rar', bulkArchive }),
  job({ id: 'part_18', filename: 'WWE_2K25.part018.rar', bulkArchive }),
]);

if (!isBulkAggregateJob(outOfOrderRows[0])) {
  throw new Error('out-of-order aggregate should expose bulk metadata');
}
assert.deepEqual(
  outOfOrderRows[0].bulkMemberIds,
  ['part_17', 'part_18', 'part_19'],
  'aggregate metadata should expose multipart members in detected part order',
);
assert.deepEqual(
  outOfOrderMembers.bulk_1.map((member) => member.id),
  ['part_17', 'part_18', 'part_19'],
  'inline bulk member lookup should sort multipart files by sequence, not queue insertion order',
);

assert.deepEqual(
  getQueueCounts(rows),
  {
    all: 1,
    active: 0,
    attention: 0,
    queued: 0,
    completed: 1,
    categories: {
      document: 1,
      program: 0,
      picture: 0,
      video: 0,
      compressed: 0,
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
    bulk: {
      all: 1,
      active: 1,
      completed: 0,
    },
  },
  'counts should isolate the bulk archive from regular downloads',
);

assert.deepEqual(
  filterJobsForView(rows, 'all', 'part02').map((row) => row.id),
  [],
  'normal all-download search should not surface bulk archive members',
);

assert.deepEqual(
  filterJobsForView(rows, 'bulk', 'part02', membersByArchiveId).map((row) => row.id),
  [aggregate.id],
  'bulk search should match filenames from collapsed bulk members via the lookup',
);

assert.deepEqual(
  filterJobsForView(rows, 'bulk', 'dl.fuckingfast.co/dl/two', membersByArchiveId).map((row) => row.id),
  [aggregate.id],
  'bulk search should match URLs from collapsed bulk members via the lookup',
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

const finalizingRows = groupBulkQueueRows([
  job({
    id: 'part_finalizing_1',
    filename: 'Game.part01.rar',
    state: 'completed',
    progress: 100,
    totalBytes: 500,
    downloadedBytes: 500,
    bulkArchive: {
      id: 'bulk_finalizing',
      name: 'Game',
      archiveStatus: 'combining',
      outputKind: 'folder',
      finalizeTotalBytes: 1_000,
      finalizeProcessedBytes: 400,
    },
  }),
  job({
    id: 'part_finalizing_2',
    filename: 'Game.part02.rar',
    state: 'completed',
    progress: 100,
    totalBytes: 500,
    downloadedBytes: 500,
    bulkArchive: {
      id: 'bulk_finalizing',
      name: 'Game',
      archiveStatus: 'combining',
      outputKind: 'folder',
      finalizeTotalBytes: 1_000,
      finalizeProcessedBytes: 400,
    },
  }),
]);

assert.equal(
  finalizingRows[0].state,
  'downloading',
  'finalizing aggregate rows should remain active while combining files',
);
assert.equal(
  finalizingRows[0].progress,
  40,
  'finalizing aggregate rows should reflect archive finalization progress',
);

const removingRows = groupBulkQueueRows([
  job({
    id: 'part_removing',
    filename: 'Game.part01.rar',
    state: 'canceled',
    removalState: 'removing',
    bulkArchive,
  }),
  job({
    id: 'part_done',
    filename: 'Game.part02.rar',
    state: 'completed',
    progress: 100,
    totalBytes: 500,
    downloadedBytes: 500,
    bulkArchive,
  }),
]);

if (!isBulkAggregateJob(removingRows[0])) {
  throw new Error('removing aggregate should expose bulk metadata');
}
assert.equal(
  removingRows[0].removalState,
  'removing',
  'bulk aggregate should inherit removing state while destructive cleanup is active',
);
assert.deepEqual(
  queueStatusPresentation(removingRows[0]),
  { label: 'Removing', tone: 'warning' },
  'bulk aggregate rows should display Removing while member cleanup is active',
);

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
