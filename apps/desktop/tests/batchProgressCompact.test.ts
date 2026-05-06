import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const batchSource = readFileSync(new URL('../src/BatchProgressWindow.svelte', import.meta.url), 'utf8');

assert.match(batchSource, /#snippet BatchJobList\(jobs: DownloadJob\[\], failedItems: FailedBatchItem\[\]\)/, 'batch progress rows should be rendered by a named compact boxed list snippet with resolver failures');
assert.match(batchSource, /@render BatchJobList\(jobs, failedItems\)/, 'batch progress window should route job rows and resolver failures through the boxed list helper');
assert.match(batchSource, /rounded border border-border\/60 bg-background\/40/, 'batch progress job list should be visually contained in a softened box');
assert.match(batchSource, /ActionButton\('Show'[\s\S]*revealBulkArchive\(completedArchive\.id\)[\s\S]*\{ closeOnSuccess: true \}/, 'completed batch Show should close the popup after a successful action');
assert.doesNotMatch(batchSource, /ActionButton\(bulkOpenLabel\(completedArchive\)[\s\S]*openBulkArchive\(completedArchive\.id\)/, 'completed bulk popup should no longer show a second Open action');
assert.match(batchSource, /FailedBatchItemRow/, 'batch progress should render hoster resolver failures as explicit rows');
assert.match(batchSource, /Not queued/, 'resolver failure rows should be labeled as not queued instead of pretending to be failed jobs');
assert.match(batchSource, /retryBulkArchive\(failedArchive\.id\)/, 'archive creation failures should expose retry archive from the popup');
assert.match(batchSource, /bulkUiState === 'failed'[\s\S]*failedArchive/, 'archive-failed controls should be tied to actual failed archive metadata');
