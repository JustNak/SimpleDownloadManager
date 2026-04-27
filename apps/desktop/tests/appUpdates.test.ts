import assert from 'node:assert/strict';
import {
  applyInstallProgressEvent,
  finishUpdateCheck,
  initialAppUpdateState,
  shouldNotifyUpdateCheckFailure,
  shouldRunStartupUpdateCheck,
  startUpdateCheck,
} from '../src/appUpdates.ts';

assert.equal(shouldRunStartupUpdateCheck(false), true, 'startup update check should run before the first attempt');
assert.equal(shouldRunStartupUpdateCheck(true), false, 'startup update check should only run once');
assert.equal(shouldNotifyUpdateCheckFailure('startup'), false, 'startup update failures should stay silent');
assert.equal(shouldNotifyUpdateCheckFailure('manual'), true, 'manual update failures should be shown to the user');

const checking = startUpdateCheck(initialAppUpdateState, 'manual');
assert.equal(checking.status, 'checking');
assert.equal(checking.lastCheckMode, 'manual');

const available = finishUpdateCheck(checking, {
  version: '0.3.5-alpha',
  currentVersion: '0.3.4-alpha',
  body: 'Faster downloads',
});
assert.equal(available.status, 'available');
assert.equal(available.availableUpdate?.version, '0.3.5-alpha');
assert.equal(available.errorMessage, null);

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
