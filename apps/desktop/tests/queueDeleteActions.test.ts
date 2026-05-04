import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/QueueView.svelte', import.meta.url), 'utf8');

assert.match(source, /deleteFromDisk = defaultDeleteFromDiskForJobs\(deletePromptJobs\)/, 'delete prompt should use the queue command helper for the default disk-delete checkbox state');
assert.match(source, /deleteActionLabelForJob\(job\)/, 'row action menu should use the delete label helper so paused seeding torrents can say Delete from disk...');
assert.match(source, /function openDeletePrompt\(job: DownloadJob\)/, 'row action menu should open the delete confirmation dialog instead of directly removing jobs');
assert.match(source, /function openDeleteFromDiskPrompt\(job: QueueDisplayJob\)/, 'row context menu should expose an explicit disk-delete prompt helper');
assert.match(source, /deleteJobIdsForPrompt\(deletePromptJobs\)/, 'delete confirmation should expand bulk aggregate rows to their member job ids');
assert.match(source, /isCompletedBulkAggregate\(job\)[\s\S]*Delete from disk[\s\S]*openDeleteFromDiskPrompt\(job\)/, 'completed bulk archive context menus should offer Delete from disk');
assert.match(source, /isFailedBulkAggregate\(job\)[\s\S]*Delete from disk[\s\S]*openDeleteFromDiskPrompt\(job\)/, 'failed bulk archive context menus should offer Delete from disk for downloaded parts');
assert.doesNotMatch(source, /label="Remove"|>\s*Remove\s*</, 'row action menus should not bypass disk deletion through a direct Remove action');
