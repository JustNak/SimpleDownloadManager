import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const batchSource = readFileSync(new URL('../src/BatchProgressWindow.svelte', import.meta.url), 'utf8');

assert.match(batchSource, /#snippet BatchJobList\(jobs: DownloadJob\[\]\)/, 'batch progress rows should be rendered by a named compact boxed list snippet');
assert.match(batchSource, /@render BatchJobList\(jobs\)/, 'batch progress window should route job rows through the boxed list helper');
assert.match(batchSource, /rounded border border-border\/60 bg-background\/40/, 'batch progress job list should be visually contained in a softened box');
assert.match(batchSource, /ActionButton\('Show'[\s\S]*revealBulkArchive\(completedArchive\.id\)[\s\S]*\{ closeOnSuccess: true \}/, 'completed batch Show should close the popup after a successful action');
assert.doesNotMatch(batchSource, /ActionButton\(bulkOpenLabel\(completedArchive\)[\s\S]*openBulkArchive\(completedArchive\.id\)/, 'completed bulk popup should no longer show a second Open action');
