import assert from 'node:assert/strict';
import { loadInitialAppData } from '../src/appBootstrap.ts';
import type { DesktopSnapshot } from '../src/backend.ts';
import type { DiagnosticsSnapshot } from '../src/types.ts';

const snapshot: DesktopSnapshot = {
  connectionState: 'connected' as DesktopSnapshot['connectionState'],
  jobs: [
    {
      id: 'job_1',
      url: 'https://example.com/file.zip',
      filename: 'file.zip',
      transferKind: 'http',
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
    downloadPerformanceMode: 'balanced',
    torrent: {
      enabled: true,
      downloadDirectory: 'C:\\Users\\You\\Downloads\\Torrent',
      seedMode: 'forever',
      seedRatioLimit: 1,
      seedTimeLimitMinutes: 60,
      uploadLimitKibPerSecond: 0,
      portForwardingEnabled: false,
      portForwardingPort: 42000,
      peerConnectionWatchdogMode: 'diagnose',
    },
    notificationsEnabled: true,
    theme: 'system',
    accentColor: '#3b82f6',
    showDetailsOnClick: true,
    queueRowSize: 'medium',
    startOnStartup: true,
    startupLaunchMode: 'tray',
    extensionIntegration: {
      enabled: true,
      downloadHandoffMode: 'ask',
      listenPort: 1420,
      contextMenuEnabled: true,
      showProgressAfterHandoff: true,
      showBadgeStatus: true,
      excludedHosts: [],
      ignoredFileExtensions: [],
      authenticatedHandoffEnabled: false,
      authenticatedHandoffHosts: [],
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
