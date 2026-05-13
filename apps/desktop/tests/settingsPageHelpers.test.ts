import assert from 'node:assert/strict';
import { initialAppUpdateState } from '../src/appUpdates.ts';
import {
  formatUpdateProgress,
  newestFirstDiagnosticEvents,
  normalizeBulkSettings,
  normalizeListenPort,
  normalizeTorrentPort,
  renderUpdateStatus,
  updateProgressPercent,
  usesTorrentRatioLimit,
  usesTorrentTimeLimit,
} from '../src/settingsPageHelpers.ts';
import type { DiagnosticsSnapshot, Settings } from '../src/types.ts';

const bulkSettings: Settings['bulk'] = {
  outputDirectory: '',
  maxConcurrentDownloads: 99,
  speedLimitKibPerSecond: -10,
  downloadPerformanceMode: 'unknown' as Settings['bulk']['downloadPerformanceMode'],
  hosterFairnessMode: 'unknown' as Settings['bulk']['hosterFairnessMode'],
  hosterAccelerationMode: 'unknown' as Settings['bulk']['hosterAccelerationMode'],
  autoRetryOverrideEnabled: true,
  autoRetryAttempts: 99,
  startBehavior: 'review_then_start',
  expandActiveRowsByDefault: false,
};

assert.equal(normalizeListenPort('65536'), 1420, 'extension listen port should fall back outside TCP range');
assert.equal(normalizeListenPort('8080'), 8080, 'extension listen port should preserve valid ports');
assert.equal(normalizeTorrentPort('80'), 42000, 'torrent port should stay in the non-privileged range');
assert.equal(normalizeTorrentPort('49000'), 49000, 'torrent port should preserve valid non-privileged ports');

assert.deepEqual(
  normalizeBulkSettings(bulkSettings, 'D:\\Downloads'),
  {
    ...bulkSettings,
    outputDirectory: 'D:\\Downloads\\Bulk',
    maxConcurrentDownloads: 24,
    speedLimitKibPerSecond: 0,
    downloadPerformanceMode: 'fast',
    hosterFairnessMode: 'adaptive',
    hosterAccelerationMode: 'safe',
    autoRetryAttempts: 10,
  },
  'bulk settings normalization should clamp numeric values and default invalid modes',
);

assert.equal(usesTorrentRatioLimit('ratio_or_time'), true, 'ratio-or-time seeding should expose the ratio limit');
assert.equal(usesTorrentTimeLimit('ratio_or_time'), true, 'ratio-or-time seeding should expose the time limit');
assert.equal(usesTorrentRatioLimit('time'), false, 'time-only seeding should hide ratio limit inputs');

const diagnostics = {
  recentEvents: [
    { timestamp: 1, level: 'info', message: 'old' },
    { timestamp: 2, level: 'warning', message: 'new' },
  ],
} as DiagnosticsSnapshot;

assert.deepEqual(
  newestFirstDiagnosticEvents(diagnostics).map((event) => event.message),
  ['new', 'old'],
  'diagnostic events should be presented newest-first without mutating the snapshot',
);
assert.deepEqual(
  diagnostics.recentEvents.map((event) => event.message),
  ['old', 'new'],
  'diagnostic event normalization should leave backend snapshots untouched',
);

const downloadingUpdate = {
  ...initialAppUpdateState,
  status: 'downloading' as const,
  downloadedBytes: 1024,
  totalBytes: 4096,
};

assert.equal(updateProgressPercent(downloadingUpdate), 25, 'update progress should use downloaded and total bytes');
assert.equal(formatUpdateProgress(downloadingUpdate), '1.0 KiB / 4.0 KiB', 'update progress should render compact byte text');
assert.equal(
  renderUpdateStatus({ ...initialAppUpdateState, status: 'checking' }, false),
  'Checking GitHub Releases for a newer beta build.',
  'update status copy should stay outside the Svelte component',
);
