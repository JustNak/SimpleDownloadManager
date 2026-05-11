import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const appSource = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
const queueViewSource = await readFile(new URL('../src/QueueView.svelte', import.meta.url), 'utf8');

assert.match(
  appSource,
  /pauseJobs/,
  'main queue pause handling should use the scoped pauseJobs backend command for bulk aggregate rows',
);

assert.match(
  appSource,
  /resumeJobs/,
  'main queue resume handling should use the scoped resumeJobs backend command for bulk aggregate rows',
);

assert.match(
  appSource,
  /cancelJobs/,
  'main queue cancel handling should use the scoped cancelJobs backend command for bulk aggregate rows',
);

assert.doesNotMatch(
  appSource,
  /async function handleCancel[\s\S]*deleteFromDisk:\s*true[\s\S]*async function handleRetry/,
  'main queue cancel handling should remain stop-only and not delete files from disk',
);

assert.doesNotMatch(
  appSource,
  /Promise\.all\(actionableIds\.map\(\(jobId\) => action\(jobId\)\)\)/,
  'main queue bulk actions should not fan out one IPC call per member',
);

assert.match(
  appSource,
  /let pendingQueueActionIds = \$state<Set<string>>\(new Set\(\)\)/,
  'main queue should track jobs with an in-flight pause, resume, or cancel action',
);

assert.match(
  appSource,
  /pendingActionIds=\{pendingQueueActionIds\}/,
  'main queue should pass pending action ids into QueueView',
);

assert.match(
  queueViewSource,
  /pendingActionIds: Set<string>/,
  'QueueView should accept pending action ids from the main app',
);

assert.match(
  queueViewSource,
  /function isActionPending\(job: QueueDisplayJob\)/,
  'QueueView should centralize pending action checks for rows and bulk aggregates',
);

assert.match(
  queueViewSource,
  /disabled=\{isActionPending\(job\)\}/,
  'direct pause and resume row buttons should be disabled while their action is pending',
);

assert.match(
  queueViewSource,
  /MenuItem\(Pause, 'Pause'[\s\S]*isActionPending\(job\)/,
  'pause menu items should be disabled while their action is pending',
);
