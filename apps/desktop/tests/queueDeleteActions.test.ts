import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/QueueView.tsx', import.meta.url), 'utf8');

assert.match(
  source,
  /setDeleteFromDisk\(defaultDeleteFromDiskForJobs\(removableJobs\)\)/,
  'delete prompt should use the queue command helper for the default disk-delete checkbox state',
);

assert.match(
  source,
  /label=\{deleteActionLabelForJob\(job\)\}/,
  'row action menu should use the delete label helper so paused seeding torrents can say Delete from disk...',
);

assert.match(
  source,
  /openDeletePromptForJobs\(\[job\]\)/,
  'row action menu should open the delete confirmation dialog instead of directly removing jobs',
);

assert.doesNotMatch(
  source,
  /label="Remove"/,
  'row action menus should not bypass disk deletion through a direct Remove action',
);
