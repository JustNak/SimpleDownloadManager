import assert from 'node:assert/strict';
import {
  applyInstallProgressEvent,
  bulkUpdateBlockerForJobs,
  finishUpdateCheck,
  initialAppUpdateState,
  shouldOpenUpdatePrompt,
  shouldNotifyUpdateCheckFailure,
  shouldRunStartupUpdateCheck,
  startUpdateCheck,
  updateVersionIndicator,
} from '../src/appUpdates.ts';
import type { DownloadJob } from '../src/types.ts';

const baseJob: DownloadJob = {
  id: 'job_1',
  url: 'https://example.com/file.bin',
  filename: 'file.bin',
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

const pendingBulkArchive = {
  id: 'bulk_1',
  name: 'bulk-download.zip',
  archiveStatus: 'pending' as const,
};

const activeBulkBlocker = bulkUpdateBlockerForJobs([
  job({ state: 'queued', bulkArchive: pendingBulkArchive }),
]);

assert.equal(activeBulkBlocker?.kind, 'bulk_download', 'queued HTTP bulk members should block app update installs');
assert.equal(
  bulkUpdateBlockerForJobs([
    job({
      state: 'completed',
      bulkArchive: { ...pendingBulkArchive, archiveStatus: 'extracting' },
    }),
  ])?.kind,
  'bulk_archive',
  'bulk archive finalization should block app update installs',
);
assert.equal(
  bulkUpdateBlockerForJobs([
    job({ state: 'paused', bulkArchive: pendingBulkArchive }),
    job({ id: 'job_2', state: 'completed', bulkArchive: { ...pendingBulkArchive, archiveStatus: 'failed' } }),
    job({ id: 'job_3', state: 'canceled', bulkArchive: pendingBulkArchive }),
    job({ id: 'job_4', state: 'failed', bulkArchive: pendingBulkArchive }),
  ]),
  null,
  'paused, completed, canceled, and failed bulk rows should not block updates',
);

assert.equal(shouldRunStartupUpdateCheck(false, null), true, 'startup update check should run before the first attempt');
assert.equal(shouldRunStartupUpdateCheck(false, activeBulkBlocker), false, 'startup update check should wait for active bulk work to clear');
assert.equal(shouldRunStartupUpdateCheck(true, null), false, 'startup update check should only run once');
assert.equal(shouldOpenUpdatePrompt(null), true, 'update prompts should open when no bulk work is active');
assert.equal(shouldOpenUpdatePrompt(activeBulkBlocker), false, 'update prompts should stay deferred while bulk work is active');
assert.equal(shouldNotifyUpdateCheckFailure('startup'), false, 'startup update failures should stay silent');
assert.equal(shouldNotifyUpdateCheckFailure('manual'), true, 'manual update failures should be shown to the user');

const checking = startUpdateCheck(initialAppUpdateState, 'manual');
assert.equal(checking.status, 'checking');
assert.equal(checking.lastCheckMode, 'manual');

const available = finishUpdateCheck(checking, {
  version: '0.3.45-alpha',
  currentVersion: '0.3.4-alpha',
  body: 'Faster downloads',
});
assert.equal(available.status, 'available');
assert.equal(available.availableUpdate?.version, '0.3.45-alpha');
assert.equal(available.errorMessage, null);

assert.deepEqual(
  updateVersionIndicator(available, '0.3.44-alpha'),
  {
    currentVersion: '0.3.4-alpha',
    newVersion: '0.3.45-alpha',
    newVersionTone: 'available',
  },
  'available updates should expose current and new version indicators from updater metadata',
);

assert.deepEqual(
  updateVersionIndicator(finishUpdateCheck(checking, null), '0.3.49-alpha'),
  {
    currentVersion: '0.3.49-alpha',
    newVersion: '0.3.49-alpha',
    newVersionTone: 'current',
  },
  'latest builds should show the installed version as both current and new',
);

assert.deepEqual(
  updateVersionIndicator(checking, '0.3.49-alpha'),
  {
    currentVersion: '0.3.49-alpha',
    newVersion: 'Checking...',
    newVersionTone: 'pending',
  },
  'checking state should show a pending new-version indicator',
);

assert.deepEqual(
  updateVersionIndicator({ ...checking, status: 'error', errorMessage: 'network unavailable' }, '0.3.49-alpha'),
  {
    currentVersion: '0.3.49-alpha',
    newVersion: 'Unavailable',
    newVersionTone: 'error',
  },
  'error state should keep showing the installed version as current',
);

const started = applyInstallProgressEvent(available, {
  event: 'started',
  data: { contentLength: 100 },
});
assert.equal(started.status, 'downloading');
assert.equal(started.downloadedBytes, 0);
assert.equal(started.totalBytes, 100);

const progressed = applyInstallProgressEvent(started, {
  event: 'progress',
  data: { chunkLength: 25 },
});
assert.equal(progressed.downloadedBytes, 25);
assert.equal(progressed.totalBytes, 100);

const finished = applyInstallProgressEvent(progressed, { event: 'finished' });
assert.equal(finished.status, 'installing');
