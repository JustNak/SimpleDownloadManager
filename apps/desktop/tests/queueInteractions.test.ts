import assert from 'node:assert/strict';
import {
  isJobArtifactMissing,
  selectJobRange,
  shouldBlurJobIdentity,
  shouldOpenJobFileOnDoubleClick,
} from '../src/queueInteractions.ts';

const completedJob = {
  id: 'job_1',
  url: 'https://example.com/file.zip',
  filename: 'file.zip',
  transferKind: 'http',
  state: 'completed',
  progress: 100,
  totalBytes: 1024,
  downloadedBytes: 1024,
  speed: 0,
  eta: 0,
  targetPath: 'C:\\Users\\Alice\\Downloads\\file.zip',
} as Parameters<typeof shouldOpenJobFileOnDoubleClick>[0];

assert.equal(
  shouldOpenJobFileOnDoubleClick(completedJob, 0),
  true,
  'left-button double click should open a job file with a target path',
);

assert.equal(
  shouldOpenJobFileOnDoubleClick({ ...completedJob, targetPath: '' }, 0),
  false,
  'double click should not open when no target path is recorded',
);

assert.equal(
  shouldOpenJobFileOnDoubleClick(completedJob, 2),
  false,
  'non-left double click should not open a job file',
);

assert.equal(
  isJobArtifactMissing({ ...completedJob, artifactExists: false } as typeof completedJob),
  true,
  'completed jobs with a missing artifact should be treated as missing',
);

assert.equal(
  isJobArtifactMissing({ ...completedJob, artifactExists: true } as typeof completedJob),
  false,
  'completed jobs with an existing artifact should not be treated as missing',
);

assert.equal(
  isJobArtifactMissing({ ...completedJob, artifactExists: false, state: 'downloading' } as typeof completedJob),
  false,
  'unfinished jobs should not use the completed-file missing state',
);

assert.equal(
  shouldBlurJobIdentity({ ...completedJob, state: 'downloading' } as typeof completedJob),
  true,
  'actively downloading jobs should blur the file identity',
);

assert.equal(
  shouldBlurJobIdentity({
    ...completedJob,
    transferKind: 'torrent',
    state: 'downloading',
    progress: 44,
    totalBytes: 3 * 1024,
    downloadedBytes: 1024,
    torrent: { uploadedBytes: 2048, fetchedBytes: 4096, ratio: 0.03, seedingStartedAt: 123_456 },
  } as typeof completedJob),
  false,
  'seeding restore validation should not blur the file identity like an active peer download',
);

assert.equal(
  shouldBlurJobIdentity(completedJob),
  false,
  'completed jobs should not blur the file identity',
);

assert.deepEqual(
  selectJobRange(['job_1', 'job_2', 'job_3', 'job_4'], 'job_2', 'job_4'),
  ['job_2', 'job_3', 'job_4'],
  'selection ranges should include all ids between the anchor and current job',
);

assert.deepEqual(
  selectJobRange(['job_1', 'job_2', 'job_3', 'job_4'], 'job_4', 'job_2'),
  ['job_2', 'job_3', 'job_4'],
  'selection ranges should support reverse dragging',
);

assert.deepEqual(
  selectJobRange(['job_1', 'job_2'], 'job_1', 'job_missing'),
  [],
  'selection ranges should be empty when either endpoint is not visible',
);
