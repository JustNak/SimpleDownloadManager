import assert from 'node:assert/strict';
import { formatDiagnosticsReport } from '../src/diagnosticsReport.ts';
import type { DiagnosticsSnapshot } from '../src/types.ts';

const diagnostics: DiagnosticsSnapshot = {
  connectionState: 'connected',
  lastHostContactSecondsAgo: 2,
  queueSummary: {
    total: 1,
    active: 0,
    attention: 0,
    queued: 0,
    downloading: 0,
    completed: 1,
    failed: 0,
  },
  hostRegistration: {
    status: 'configured',
    entries: [],
  },
  torrentDiagnostics: [
    {
      jobId: 'job_torrent',
      filename: 'ubuntu.iso',
      infoHash: '420f3778a160fbe6eb0a67c8470256be13b0ecc8',
      diagnostics: {
        livePeers: 12,
        queuedPeers: 3,
        connectingPeers: 2,
        seenPeers: 30,
        deadPeers: 4,
        notNeededPeers: 5,
        contributingPeers: 2,
        peerErrors: 1,
        peersWithErrors: 1,
        peerConnectionAttempts: 7,
        sessionDownloadSpeed: 65_536,
        sessionUploadSpeed: 8_192,
        listenPort: 42000,
        listenerFallback: true,
        peerSamples: [
          {
            state: 'live',
            fetchedBytes: 262_144,
            errors: 1,
            downloadedPieces: 2,
            connectionAttempts: 1,
          },
        ],
      },
    },
  ],
  recentEvents: [
    {
      timestamp: 1_714_000_000_000,
      level: 'info',
      category: 'download',
      message: 'Completed file.zip',
      jobId: 'job_1',
    },
  ],
};

const report = formatDiagnosticsReport(diagnostics);

assert.match(report, /Recent Events:/, 'diagnostics report should include recent events');
assert.match(report, /info download job_1 Completed file\.zip/, 'diagnostics event details should be included');
assert.match(report, /Torrent Diagnostics:/, 'diagnostics report should include torrent diagnostics');
assert.match(report, /job_torrent ubuntu\.iso/, 'torrent diagnostics should identify the affected torrent job');
assert.match(report, /Live Peers: 12/, 'torrent diagnostics should include live peer count');
assert.match(report, /Peer Error Events: 1/, 'torrent diagnostics should include peer error event totals');
assert.match(report, /Peers With Errors: 1/, 'torrent diagnostics should include errored peer totals');
assert.match(report, /Peer Connection Attempts: 7/, 'torrent diagnostics should include peer connection attempts');
assert.match(report, /Listen Port: 42000 \(fallback active\)/, 'torrent diagnostics should include listener fallback state');
assert.match(report, /Peer Samples:/, 'torrent diagnostics should include bounded peer samples');
