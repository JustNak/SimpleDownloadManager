import assert from 'node:assert/strict';
import { loadInitialAppData } from '../src/appBootstrap';
import { ConnectionState } from '../src/types';
import type { DesktopSnapshot } from '../src/backend';
import type { DiagnosticsSnapshot } from '../src/types';

const snapshot: DesktopSnapshot = {
  connectionState: ConnectionState.Connected,
  jobs: [
    {
      id: 'job_1',
      url: 'https://example.com/file.zip',
      filename: 'file.zip',
      state: 'completed',
      progress: 100,
      totalBytes: 10,
      downloadedBytes: 10,
      speed: 0,
      eta: 0,
    },
  ],
  settings: {
    downloadDirectory: 'C:\\Users\\You\\Downloads',
    maxConcurrentDownloads: 3,
    autoRetryAttempts: 3,
    speedLimitKibPerSecond: 0,
    notificationsEnabled: true,
    theme: 'system',
    accentColor: '#3b82f6',
    extensionIntegration: {
      enabled: true,
      downloadHandoffMode: 'ask',
      listenPort: 1420,
      contextMenuEnabled: true,
      showProgressAfterHandoff: true,
      showBadgeStatus: true,
      excludedHosts: [],
      ignoredFileExtensions: [],
    },
  },
};

const result = await loadInitialAppData(
  async () => snapshot,
  async (): Promise<DiagnosticsSnapshot> => {
    throw new Error('diagnostics failed');
  },
);

assert.equal(result.snapshot?.jobs.length, 1);
assert.equal(result.snapshot?.jobs[0].id, 'job_1');
assert.equal(result.diagnostics, null);
assert.equal(result.snapshotError, null);
assert.match(String(result.diagnosticsError), /diagnostics failed/);
