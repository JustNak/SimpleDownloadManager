import assert from 'node:assert/strict';
import {
  BULK_MEMBER_PANEL_MAX_HEIGHT,
  BULK_MEMBER_ROW_HEIGHT,
  bulkExpansionHeight,
  bulkMemberPanelHeight,
  pruneRecordKeys,
} from '../src/queueBulkExpansion.ts';
import {
  formatQueueSpeed,
  formatQueueTime,
  formatTorrentRatio,
  formatTorrentSeedMetric,
} from '../src/queueFormatting.ts';
import { deleteJobIdsForPrompt, selectedIdsForJob } from '../src/queueSelection.ts';
import type { BulkAggregateDownloadJob, QueueDisplayJob } from '../src/bulkQueueRows.ts';
import type { DownloadJob } from '../src/types.ts';

const baseJob: DownloadJob = {
  id: 'job_1',
  url: 'https://example.com/file.bin',
  filename: 'file.bin',
  transferKind: 'http',
  state: 'downloading',
  progress: 50,
  totalBytes: 2048,
  downloadedBytes: 1024,
  speed: 1024,
  eta: 60,
};

const bulkJob: BulkAggregateDownloadJob = {
  ...baseJob,
  id: 'bulk:archive_1',
  state: 'canceled',
  bulkAggregate: true,
  bulkArchiveId: 'archive_1',
  bulkMemberIds: ['member_1', 'member_2'],
  bulkRetryableMemberCount: 0,
  bulkArchiveFixable: false,
};

assert.equal(
  bulkMemberPanelHeight(3),
  3 * BULK_MEMBER_ROW_HEIGHT,
  'bulk member panels should size directly from visible member count below the cap',
);

assert.equal(
  bulkMemberPanelHeight(30),
  BULK_MEMBER_PANEL_MAX_HEIGHT,
  'bulk member panels should cap tall groups at the virtualized max height',
);

assert.equal(bulkExpansionHeight(0), 0, 'empty bulk groups should not reserve expansion space');
assert.equal(bulkExpansionHeight(2), (2 * BULK_MEMBER_ROW_HEIGHT) + 8, 'expanded bulk groups should include panel chrome');

assert.deepEqual(
  pruneRecordKeys({ visible: 1, stale: 2 }, new Set(['visible'])),
  { visible: 1 },
  'bulk scroll metrics should drop keys for rows no longer visible',
);

assert.deepEqual(
  selectedIdsForJob(baseJob, new Set(['job_1', 'job_2'])),
  ['job_1', 'job_2'],
  'selected row actions should operate on the current multi-selection',
);

assert.deepEqual(
  selectedIdsForJob({ ...baseJob, id: 'job_3' }, new Set(['job_1', 'job_2'])),
  ['job_3'],
  'unselected row actions should target only the clicked row',
);

assert.deepEqual(
  deleteJobIdsForPrompt([bulkJob, { ...baseJob, id: 'job_3' }] as QueueDisplayJob[]),
  ['member_1', 'member_2', 'job_3'],
  'delete prompts should expand bulk aggregate rows to their member job ids',
);

assert.equal(
  formatQueueSpeed(baseJob, 2048),
  '2.0 KB/s',
  'downloading rows should show average download speed',
);

assert.equal(
  formatQueueTime(baseJob, 75),
  '1m 15s',
  'downloading rows should show ETA text',
);

const seedingJob: DownloadJob = {
  ...baseJob,
  transferKind: 'torrent',
  state: 'seeding',
  torrent: { uploadedBytes: 4096, ratio: 1.25 },
};

assert.equal(formatTorrentSeedMetric(seedingJob), '4.0 KB', 'torrent seed columns should show uploaded bytes');
assert.equal(formatTorrentRatio(seedingJob), '1.25x', 'torrent ratio text should retain two decimals');
assert.equal(formatQueueTime(seedingJob, 0), '1.25x', 'seeding rows should show ratio in the time slot');
