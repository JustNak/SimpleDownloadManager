import assert from 'node:assert/strict';
import { shouldRevealJobDirectoryOnDoubleClick } from '../src/queueInteractions.ts';

const completedJob = {
  id: 'job_1',
  url: 'https://example.com/file.zip',
  filename: 'file.zip',
  state: 'completed',
  progress: 100,
  totalBytes: 1024,
  downloadedBytes: 1024,
  speed: 0,
  eta: 0,
  targetPath: 'C:\\Users\\Alice\\Downloads\\file.zip',
} as Parameters<typeof shouldRevealJobDirectoryOnDoubleClick>[0];

assert.equal(
  shouldRevealJobDirectoryOnDoubleClick(completedJob, 0),
  true,
  'left-button double click should reveal a job with a target path',
);

assert.equal(
  shouldRevealJobDirectoryOnDoubleClick({ ...completedJob, targetPath: '' }, 0),
  false,
  'double click should not reveal when no target path is recorded',
);

assert.equal(
  shouldRevealJobDirectoryOnDoubleClick(completedJob, 2),
  false,
  'non-left double click should not reveal a job directory',
);
