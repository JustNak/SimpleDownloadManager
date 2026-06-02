import assert from 'node:assert/strict';
import { applyDownloadUpdateBatch, mergeDownloadUpdateBatches } from '../src/downloadUpdateBatch.ts';
import type { DownloadJob } from '../src/types.ts';

function job(id: string, filename = `${id}.bin`): DownloadJob {
  return {
    id,
    url: `https://example.com/${filename}`,
    filename,
    transferKind: 'http',
    state: 'downloading',
    createdAt: 1_000,
    progress: 10,
    totalBytes: 100,
    downloadedBytes: 10,
    speed: 100,
    eta: 1,
    targetPath: `C:\\Downloads\\${filename}`,
  };
}

const originalFirst = job('job_1');
const originalSecond = job('job_2');
const updatedSecond = {
  ...originalSecond,
  progress: 45,
  downloadedBytes: 45,
  speed: 400,
};
const insertedThird = job('job_3', 'new.bin');

const nextJobs = applyDownloadUpdateBatch(
  [originalFirst, originalSecond],
  {
    jobs: [updatedSecond, insertedThird],
    removedJobIds: ['job_1'],
  },
);

assert.deepEqual(
  nextJobs.map((candidate) => candidate.id),
  ['job_2', 'job_3'],
  'batch updates should remove missing rows, update existing rows in place, and append new rows',
);

assert.deepEqual(nextJobs[0], updatedSecond, 'existing rows should be replaced by matching id');
assert.deepEqual(nextJobs[1], insertedThird, 'new rows should be appended in update order');

const partiallyChangedJobs = applyDownloadUpdateBatch(
  [originalFirst, originalSecond],
  {
    jobs: [updatedSecond],
    removedJobIds: [],
  },
);

assert.equal(partiallyChangedJobs[0], originalFirst, 'rows missing from a non-empty batch should preserve object identity');
assert.deepEqual(partiallyChangedJobs[1], updatedSecond, 'changed rows should still be replaced when other rows are preserved');

const unchangedJobs = applyDownloadUpdateBatch(
  [originalFirst, originalSecond],
  {
    jobs: [],
    removedJobIds: [],
  },
);

assert.equal(unchangedJobs[0], originalFirst, 'unchanged rows should preserve object identity');
assert.equal(unchangedJobs[1], originalSecond, 'unchanged rows should preserve object identity');

const coalescedBatch = mergeDownloadUpdateBatches(
  {
    jobs: [
      { ...originalFirst, progress: 20, downloadedBytes: 20 },
      { ...originalSecond, progress: 30, downloadedBytes: 30 },
    ],
    removedJobIds: ['job_removed_first'],
  },
  {
    jobs: [
      { ...originalFirst, progress: 60, downloadedBytes: 60 },
      insertedThird,
      job('job_removed_first'),
    ],
    removedJobIds: ['job_2'],
  },
);

assert.deepEqual(
  coalescedBatch,
  {
    jobs: [
      { ...originalFirst, progress: 60, downloadedBytes: 60 },
      insertedThird,
      job('job_removed_first'),
    ],
    removedJobIds: ['job_2'],
  },
  'merged hidden batches should keep only the latest update/removal per job while preserving sequential semantics',
);

const sequentialBatchResult = applyDownloadUpdateBatch(
  applyDownloadUpdateBatch(
    [originalFirst, originalSecond],
    {
      jobs: [
        { ...originalFirst, progress: 20, downloadedBytes: 20 },
        { ...originalSecond, progress: 30, downloadedBytes: 30 },
      ],
      removedJobIds: ['job_removed_first'],
    },
  ),
  {
    jobs: [
      { ...originalFirst, progress: 60, downloadedBytes: 60 },
      insertedThird,
      job('job_removed_first'),
    ],
    removedJobIds: ['job_2'],
  },
);

assert.deepEqual(
  applyDownloadUpdateBatch([originalFirst, originalSecond], coalescedBatch),
  sequentialBatchResult,
  'applying a merged hidden batch should match applying each hidden batch in order',
);

assert.deepEqual(
  mergeDownloadUpdateBatches(
    { jobs: [{ ...originalFirst, progress: 20, downloadedBytes: 20 }], removedJobIds: [] },
    { jobs: [{ ...originalFirst, progress: 80, downloadedBytes: 80 }], removedJobIds: ['job_1'] },
  ),
  {
    jobs: [],
    removedJobIds: ['job_1'],
  },
  'merged batches should preserve removal precedence when an event payload includes the same id in updates and removals',
);
