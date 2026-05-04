import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/QueueView.svelte', import.meta.url), 'utf8');

assert.match(source, /deleteFromDisk = defaultDeleteFromDiskForJobs\(deletePromptJobs\)/, 'delete prompt should use the queue command helper for the default disk-delete checkbox state');
assert.match(source, /deleteActionLabelForJob\(job\)/, 'row action menu should use the delete label helper so paused seeding torrents can say Delete from disk...');
assert.match(source, /function openDeletePrompt\(job: DownloadJob\)/, 'row action menu should open the delete confirmation dialog instead of directly removing jobs');
assert.match(source, /function openDeletePromptForJobs\(jobs: QueueDisplayJob\[\]\)/, 'selected-row bulk deletion should use the same prompt setup helper as row menus');
assert.match(source, /function openDeleteFromDiskPrompt\(job: QueueDisplayJob\)/, 'row context menu should expose an explicit disk-delete prompt helper');
assert.match(source, /deleteJobIdsForPrompt\(deletePromptJobs\)/, 'delete confirmation should expand bulk aggregate rows to their member job ids');
assert.match(source, /isCanceledBulkAggregate\(job\)[\s\S]*Delete[\s\S]*openDeletePrompt\(job\)[\s\S]*Delete from disk[\s\S]*openDeleteFromDiskPrompt\(job\)/, 'canceled bulk aggregate menus should expose Delete and Delete from disk');
assert.match(source, /isCompletedBulkAggregate\(job\)[\s\S]*Delete from disk[\s\S]*openDeleteFromDiskPrompt\(job\)/, 'completed bulk archive context menus should offer Delete from disk');
assert.match(source, /isFailedBulkAggregate\(job\)[\s\S]*Delete from disk[\s\S]*openDeleteFromDiskPrompt\(job\)/, 'failed bulk archive context menus should offer Delete from disk for downloaded parts');
assert.match(source, /selectedJobs\.every\(\(job\) => canOpenSelectedDeletePrompt\(job\)\)[\s\S]*openDeletePromptForJobs\(selectedJobs\)/, 'selected canceled bulk aggregate rows should be able to open the delete prompt');
assert.doesNotMatch(source, /label="Remove"|>\s*Remove\s*</, 'row action menus should not bypass disk deletion through a direct Remove action');
