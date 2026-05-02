import assert from 'node:assert/strict';
import {
  shouldNotifyDiagnosticsRefreshFailure,
  shouldRefreshDiagnostics,
} from '../src/diagnosticsRefresh.ts';

assert.equal(
  shouldNotifyDiagnosticsRefreshFailure({ silent: true }),
  false,
  'background diagnostics refresh failures should not create toasts',
);

assert.equal(
  shouldNotifyDiagnosticsRefreshFailure({ silent: false }),
  true,
  'user-initiated diagnostics refresh failures should still create toasts',
);

assert.equal(
  shouldRefreshDiagnostics(30_000, 0, { silent: true }),
  true,
  'background diagnostics refresh should run once the throttle interval has elapsed',
);

assert.equal(
  shouldRefreshDiagnostics(29_999, 0, { silent: true }),
  false,
  'background diagnostics refresh should be throttled before 30 seconds',
);

assert.equal(
  shouldRefreshDiagnostics(1_000, 0, { silent: false }),
  true,
  'user-initiated diagnostics refresh should bypass the background throttle',
);
