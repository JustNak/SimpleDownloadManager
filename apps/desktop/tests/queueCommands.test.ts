import assert from 'node:assert/strict';
import {
  canSwapFailedDownloadToBrowser,
  canClearCompletedDownloads,
  canRemoveDownloadImmediately,
  canShowProgressPopup,
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

assert.equal(
  canShowProgressPopup(job('job_1', 'queued')),
  true,
  'queued downloads should be eligible for a progress popup',
);

assert.equal(
  canShowProgressPopup(job('job_1', 'starting')),
  true,
  'starting downloads should be eligible for a progress popup',
);

assert.equal(
  canShowProgressPopup(job('job_1', 'downloading')),
  true,
  'downloading jobs should be eligible for a progress popup',
);

assert.equal(
  canShowProgressPopup(job('job_1', 'seeding')),
  true,
  'seeding torrents should be eligible for a progress popup',
);

assert.equal(
  canShowProgressPopup(job('job_1', 'paused')),
  false,
  'paused downloads should not show a progress popup action',
);

assert.equal(
  canShowProgressPopup(job('job_1', 'completed')),
  false,
  'completed downloads should use open/show actions instead of progress popup',
);

assert.equal(
  canSwapFailedDownloadToBrowser({
    ...job('job_1', 'failed'),
    source: {
      entryPoint: 'browser_download',
      browser: 'chrome',
      extensionVersion: '0.3.51',
    },
  }),
  true,
  'failed browser-origin HTTP downloads should be eligible for Swap',
);

assert.equal(
  canSwapFailedDownloadToBrowser({
    ...job('job_1', 'failed'),
    source: {
      entryPoint: 'popup',
      browser: 'chrome',
      extensionVersion: '0.3.51',
    },
  }),
  false,
  'manual extension downloads should not show browser Swap after failure',
);

assert.equal(
  canSwapFailedDownloadToBrowser({
    ...job('job_1', 'downloading'),
    source: {
      entryPoint: 'browser_download',
      browser: 'chrome',
      extensionVersion: '0.3.51',
    },
  }),
  false,
  'active downloads should not show failed-download Swap',
);

assert.equal(
  canSwapFailedDownloadToBrowser({
    ...job('job_1', 'failed'),
    url: 'magnet:?xt=urn:btih:example',
    transferKind: 'torrent',
    source: {
      entryPoint: 'browser_download',
      browser: 'chrome',
      extensionVersion: '0.3.51',
    },
  }),
  false,
  'torrent failures should not be swapped to the browser download UI',
);
