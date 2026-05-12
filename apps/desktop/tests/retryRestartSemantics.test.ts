import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const queueSource = readFileSync(new URL('../src/QueueView.svelte', import.meta.url), 'utf8');
const appSource = readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8');

assert.match(
  queueSource,
  /function canOpenJobFile\(job: QueueDisplayJob\)[\s\S]*job\.state === JobState\.Completed/,
  'QueueView should only expose Open File for finalized downloads',
);

assert.match(
  queueSource,
  /\{#if canOpenJobFile\(job\)\}\s*\{@render MenuItem\(FileText, 'Open File', \(\) => onOpen\(job\.id\)\)\}\s*\{\/if\}/,
  'Open File menu item should be hidden for unfinished partial downloads',
);

assert.match(
  queueSource,
  /MenuItem\(RotateCw, 'Retry'[\s\S]*onRetry\(job\.id\)/,
  'Retry should remain a preserve-progress action',
);

assert.match(
  queueSource,
  /MenuItem\(RotateCcw, 'Restart'[\s\S]*onRestart\(job\.id\)/,
  'Restart should remain the explicit start-from-zero action',
);

assert.match(
  appSource,
  /title: 'Restarting Download'[\s\S]*Partial progress was cleared/,
  'Restart toast should clearly communicate that partial progress is discarded',
);
