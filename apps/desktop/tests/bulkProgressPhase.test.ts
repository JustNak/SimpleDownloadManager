import assert from 'node:assert/strict';
import { deriveBulkPhase } from '../src/batchProgress.ts';
import type { DownloadJob } from '../src/types.ts';

const baseJob: DownloadJob = {
  id: 'job_1',
  url: 'https://example.com/file.bin',
  filename: 'file.bin',
  transferKind: 'http',
  state: 'completed',
  progress: 100,
  totalBytes: 100,
  downloadedBytes: 100,
  speed: 0,
  eta: 0,
};

assert.equal(
  deriveBulkPhase([
    {
      ...baseJob,
      bulkArchive: {
        id: 'bulk_1',
        name: 'bulk-download.zip',
        archiveStatus: 'extracting',
      },
    },
  ]),
  'extracting',
  'bulk progress should expose an extracting archive phase before compression',
);

assert.equal(
  deriveBulkPhase([
    {
      ...baseJob,
      bulkArchive: {
        id: 'bulk_1',
        name: 'bulk-download.zip',
        archiveStatus: 'completed',
        outputPath: 'C:\\Downloads\\bulk-download.zip',
        warning: 'Could not delete one downloaded archive part.',
      },
    },
  ]),
  'ready',
  'cleanup warnings should keep the archive in the ready phase',
);

assert.equal(
  deriveBulkPhase([
    {
      ...baseJob,
      state: 'paused',
      progress: 0,
      totalBytes: 0,
      downloadedBytes: 0,
      bulkArchive: {
        id: 'bulk_1',
        name: 'I_Am_Jesus_Christ_--_fitgirl-repacks.site_--_.zip',
        archiveStatus: 'pending',
      },
    },
  ]),
  'review',
  'paused newly-added bulk batches should wait in the review phase until the user clicks Start',
);
