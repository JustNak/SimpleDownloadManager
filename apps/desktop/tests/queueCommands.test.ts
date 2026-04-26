import assert from 'node:assert/strict';
import {
  canClearCompletedDownloads,
  canRemoveDownloadImmediately,
  canRetryFailedDownloads,
} from '../src/queueCommands.ts';
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

function job(id: string, state: DownloadJob['state']): DownloadJob {
  return { ...baseJob, id, state };
}

assert.equal(
  canRetryFailedDownloads([job('job_1', 'completed'), job('job_2', 'failed')]),
  true,
  'retry failed command should be enabled when at least one failed download exists',
);

assert.equal(
  canRetryFailedDownloads([job('job_1', 'completed'), job('job_2', 'queued')]),
  false,
  'retry failed command should be disabled without failed downloads',
);

assert.equal(
  canClearCompletedDownloads([job('job_1', 'completed'), job('job_2', 'canceled')]),
  true,
  'clear finished command should be enabled for completed or canceled downloads',
);

assert.equal(
  canClearCompletedDownloads([job('job_1', 'failed'), job('job_2', 'queued')]),
  false,
  'clear finished command should be disabled without finished downloads',
);

assert.equal(
  canRemoveDownloadImmediately(job('job_1', 'downloading')),
  false,
  'active downloads should not be removed immediately because their worker owns the slot',
);

assert.equal(
  canRemoveDownloadImmediately(job('job_1', 'seeding')),
  false,
  'seeding torrents should be canceled before they are removed',
);

assert.equal(
  canRemoveDownloadImmediately(job('job_1', 'queued')),
  true,
  'inactive queued downloads can be removed immediately',
);

assert.equal(
  canRemoveDownloadImmediately(job('job_1', 'canceled')),
  true,
  'canceled transfers should be removable immediately even if backend cleanup is still settling',
);
