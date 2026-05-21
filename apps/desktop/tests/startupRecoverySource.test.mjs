import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

const cwd = process.cwd();
const desktopRoot = existsSync(path.join(cwd, 'src/backend.ts'))
  ? cwd
  : path.join(cwd, 'apps/desktop');

const backendSource = readFileSync(path.join(desktopRoot, 'src/backend.ts'), 'utf8');
const appSource = readFileSync(path.join(desktopRoot, 'src/App.svelte'), 'utf8');
const queueCommandsSource = readFileSync(path.join(desktopRoot, 'src/queueCommands.ts'), 'utf8');
const queueViewSource = readFileSync(path.join(desktopRoot, 'src/QueueView.svelte'), 'utf8');

assert.match(
  backendSource,
  /startupRecovery\?: StartupRecoverySummary/,
  'DesktopSnapshot should expose startup recovery details',
);
assert.match(
  backendSource,
  /previewLocalRecovery/,
  'backend should expose a local recovery preview command',
);
assert.match(
  backendSource,
  /importLocalRecovery/,
  'backend should expose a local recovery import command',
);

assert.match(
  appSource,
  /startupRecovery/,
  'App should read startup recovery from the initial snapshot',
);
assert.match(
  appSource,
  /autoClose:\s*false/,
  'startup recovery warning should not auto-dismiss',
);
assert.match(
  appSource,
  /Review local files/,
  'startup recovery UI should offer review-based local file recovery',
);

assert.match(
  queueCommandsSource,
  /isRecoveredLocalJob/,
  'queue command helpers should identify recovered local rows',
);
assert.match(
  queueCommandsSource,
  /canRetryJob/,
  'queue command helpers should centralize retry eligibility',
);
assert.match(
  queueViewSource,
  /canRetryJob/,
  'row actions should use retry eligibility instead of raw state checks',
);
