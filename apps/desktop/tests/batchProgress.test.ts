import assert from 'node:assert/strict';
import {
  activeBulkFinalizingStepId,
  bulkFinalizingSteps,
  bulkReviewStartSelection,
  calculateBatchProgress,
  deriveBulkPhase,
  deriveBulkUiState,
  isUntouchedBulkReviewGate,
  progressPopupIntentForSubmission,
} from '../src/batchProgress.ts';
import type { AddJobResult, AddJobsResult } from '../src/backend.ts';
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

assert.deepEqual(
  calculateBatchProgress([
    job({ id: 'job_1', state: 'downloading', downloadedBytes: 50, totalBytes: 100 }),
    job({ id: 'job_2', state: 'downloading', downloadedBytes: 25, totalBytes: 100 }),
  ]),
  {
    progress: 37.5,
    downloadedBytes: 75,
    totalBytes: 200,
    knownTotal: true,
    completedCount: 0,
    failedCount: 0,
    activeCount: 2,
    totalCount: 2,
  },
  'aggregate progress should use summed bytes when all totals are known',
);

assert.equal(
  calculateBatchProgress([
    job({ id: 'job_1', state: 'completed', progress: 100 }),
    job({ id: 'job_2', state: 'queued', progress: 0 }),
    job({ id: 'job_3', state: 'failed', progress: 15 }),
    job({ id: 'job_4', state: 'downloading', progress: 40 }),
  ]).progress,
  50,
  'unknown-size progress should fall back to completed or terminal item count',
);

assert.equal(
  deriveBulkPhase([
    job({ id: 'job_1', state: 'downloading' }),
    job({ id: 'job_2', state: 'queued' }),
  ]),
  'downloading',
  'bulk phase should remain downloading while jobs are unfinished',
);

assert.equal(
  deriveBulkPhase([
    job({
      id: 'job_1',
      state: 'completed',
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'compressing' },
    }),
    job({
      id: 'job_2',
      state: 'completed',
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'compressing' },
    }),
  ]),
  'compressing',
  'bulk phase should expose archive compression after all downloads complete',
);

assert.equal(
  deriveBulkPhase([
    job({
      id: 'job_1',
      state: 'completed',
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'completed', outputPath: 'C:\\Downloads\\bundle.zip' },
    }),
  ]),
  'ready',
  'bulk phase should report ready when archive creation completes',
);

assert.equal(
  deriveBulkPhase([
    job({
      id: 'job_1',
      state: 'completed',
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'failed', error: 'zip failed' },
    }),
  ]),
  'failed',
  'bulk phase should report failed archive creation',
);

assert.equal(
  deriveBulkUiState([
    job({
      id: 'job_1',
      state: 'paused',
      progress: 0,
      downloadedBytes: 0,
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'pending' },
    }),
  ]),
  'review',
  'bulk UI should enter review while every pending member is paused at zero progress',
);

assert.equal(
  deriveBulkUiState([
    job({
      id: 'job_1',
      state: 'paused',
      progress: 0,
      downloadedBytes: 0,
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'pending' },
    }),
    job({
      id: 'job_2',
      state: 'queued',
      progress: 0,
      downloadedBytes: 0,
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'pending' },
    }),
  ]),
  'review',
  'bulk UI should keep the initial Start/checklist gate while pending members are paused or queued at zero progress',
);

assert.equal(
  isUntouchedBulkReviewGate([
    job({
      id: 'job_1',
      state: 'paused',
      progress: 0,
      downloadedBytes: 0,
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'pending' },
    }),
    job({
      id: 'job_2',
      state: 'queued',
      progress: 0,
      downloadedBytes: 0,
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'pending' },
    }),
  ]),
  true,
  'untouched pending bulk batches should be recognized as the selectable review gate',
);

assert.equal(
  deriveBulkUiState([
    job({
      id: 'job_1',
      state: 'downloading',
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'pending' },
    }),
  ]),
  'downloading',
  'bulk UI should enter downloading after selected jobs are resumed',
);

assert.equal(
  deriveBulkUiState([
    job({
      id: 'job_1',
      state: 'completed',
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'combining', outputKind: 'folder' },
    }),
  ]),
  'finalizing',
  'bulk UI should enter finalizing while completed members are being combined',
);

assert.equal(
  deriveBulkUiState([
    job({
      id: 'job_1',
      state: 'completed',
      bulkArchive: { id: 'bulk_1', name: 'bundle.zip', archiveStatus: 'completed', outputPath: 'C:\\Downloads\\bundle.zip' },
    }),
  ]),
  'ready',
  'bulk UI should enter ready when the combined output is available',
);

assert.deepEqual(
  bulkReviewStartSelection([
    job({ id: 'job_1', state: 'paused' }),
    job({ id: 'job_2', state: 'paused' }),
    job({ id: 'job_3', state: 'paused' }),
  ], new Set(['job_1', 'job_3'])),
  {
    includedJobs: [job({ id: 'job_1', state: 'paused' }), job({ id: 'job_3', state: 'paused' })],
    excludedJobs: [job({ id: 'job_2', state: 'paused' })],
    resumableJobs: [job({ id: 'job_1', state: 'paused' }), job({ id: 'job_3', state: 'paused' })],
  },
  'starting a reviewed bulk batch should delete unchecked jobs and resume only checked resumable jobs',
);

assert.deepEqual(
  bulkFinalizingSteps([
    job({
      id: 'job_1',
      state: 'completed',
      bulkArchive: {
        id: 'bulk_1',
        name: 'bundle.zip',
        outputKind: 'archive',
        archiveStatus: 'extracting',
        requiresExtraction: true,
      },
    }),
  ]),
  [
    { id: 'uncompressing', label: 'Uncompressing' },
    { id: 'combining', label: 'Combining' },
    { id: 'compressing', label: 'Compressing' },
  ],
  'archive output with extraction should show uncompressing, combining, then compressing',
);

assert.deepEqual(
  bulkFinalizingSteps([
    job({
      id: 'job_1',
      state: 'completed',
      bulkArchive: {
        id: 'bulk_1',
        name: 'bundle.zip',
        outputKind: 'archive',
        archiveStatus: 'combining',
        requiresExtraction: false,
      },
    }),
  ]),
  [
    { id: 'combining', label: 'Combining' },
    { id: 'compressing', label: 'Compressing' },
  ],
  'archive output without extraction should skip uncompressing but still show combining and compressing',
);

assert.deepEqual(
  bulkFinalizingSteps([
    job({
      id: 'job_1',
      state: 'completed',
      bulkArchive: {
        id: 'bulk_1',
        name: 'bundle',
        outputKind: 'folder',
        archiveStatus: 'combining',
        requiresExtraction: true,
      },
    }),
  ]),
  [
    { id: 'uncompressing', label: 'Uncompressing' },
    { id: 'combining', label: 'Combining' },
  ],
  'folder output with extraction should show uncompressing and combining only',
);

assert.deepEqual(
  bulkFinalizingSteps([
    job({
      id: 'job_1',
      state: 'completed',
      bulkArchive: {
        id: 'bulk_1',
        name: 'bundle',
        outputKind: 'folder',
        archiveStatus: 'combining',
        requiresExtraction: false,
      },
    }),
  ]),
  [{ id: 'combining', label: 'Combining' }],
  'folder output without extraction should show combining only before ready',
);

assert.equal(
  activeBulkFinalizingStepId('extracting'),
  'uncompressing',
  'extracting archive status should activate the uncompressing finalizing step',
);

assert.equal(
  activeBulkFinalizingStepId('combining'),
  'combining',
  'combining archive status should activate the combining finalizing step',
);

assert.equal(
  activeBulkFinalizingStepId('compressing'),
  'compressing',
  'compressing archive status should activate the compressing finalizing step',
);

const singleQueued: AddJobResult = { jobId: 'job_1', filename: 'file.zip', status: 'queued' };
const singleDuplicate: AddJobResult = { jobId: 'job_1', filename: 'file.zip', status: 'duplicate_existing_job' };
const multiResult: AddJobsResult = {
  queuedCount: 2,
  duplicateCount: 1,
  results: [
    { jobId: 'job_1', filename: 'one.zip', status: 'queued' },
    { jobId: 'job_2', filename: 'two.zip', status: 'duplicate_existing_job' },
    { jobId: 'job_3', filename: 'three.zip', status: 'queued' },
  ],
};

assert.deepEqual(
  progressPopupIntentForSubmission('single', singleQueued),
  { type: 'single', jobId: 'job_1' },
  'single queued downloads should open the existing per-file progress popup',
);

assert.deepEqual(
  progressPopupIntentForSubmission('torrent', singleQueued),
  { type: 'single', jobId: 'job_1' },
  'torrent downloads should open the existing per-item progress popup',
);

assert.equal(
  progressPopupIntentForSubmission('single', singleDuplicate),
  null,
  'duplicate single downloads should not open a progress popup',
);

assert.deepEqual(
  progressPopupIntentForSubmission('multi', multiResult),
  {
    type: 'batch',
    context: {
      kind: 'multi',
      jobIds: ['job_1', 'job_3'],
      title: 'Multi-download progress',
    },
  },
  'multi downloads should open one batch popup for queued jobs only',
);

assert.deepEqual(
  progressPopupIntentForSubmission('bulk', multiResult, 'bundle.zip'),
  {
    type: 'batch',
    context: {
      kind: 'bulk',
      jobIds: ['job_1', 'job_3'],
      title: 'Bulk download progress',
      archiveName: 'bundle.zip',
    },
  },
  'bulk downloads should open one bulk popup with the archive name',
);
