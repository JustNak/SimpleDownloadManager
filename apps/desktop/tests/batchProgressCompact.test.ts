import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const batchSource = readFileSync(new URL('../src/BatchProgressWindow.tsx', import.meta.url), 'utf8');

assert.match(
  batchSource,
  /function BatchJobList\(\{ jobs \}: \{ jobs: DownloadJob\[\] \}\)/,
  'batch progress rows should be rendered by a named compact boxed list helper',
);

assert.match(
  batchSource,
  /<BatchJobList jobs=\{jobs\} \/>/,
  'batch progress window should route job rows through the boxed list helper',
);

assert.match(
  batchSource,
  /rounded border border-border\/60 bg-background\/40/,
  'batch progress job list should be visually contained in a softened box',
);
