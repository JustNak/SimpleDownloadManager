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
