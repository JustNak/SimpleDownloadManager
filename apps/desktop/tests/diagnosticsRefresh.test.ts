import assert from 'node:assert/strict';
import { shouldNotifyDiagnosticsRefreshFailure } from '../src/diagnosticsRefresh.ts';

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
