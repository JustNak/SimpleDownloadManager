import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const appSource = readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8');
const backendSource = readFileSync(new URL('../src/backend.ts', import.meta.url), 'utf8');
const queueSource = readFileSync(new URL('../src/QueueView.svelte', import.meta.url), 'utf8');
const batchSource = readFileSync(new URL('../src/BatchProgressWindow.svelte', import.meta.url), 'utf8');

assert.match(appSource, /groupBulkQueueRows\(jobs\)/, 'App should group bulk members before queue counts, filtering, sorting, and rendering');
assert.match(appSource, /openBatchProgressWindow\(\{[\s\S]*kind: 'bulk'[\s\S]*bulkMemberIds/, 'bulk aggregate Show Popup should open the batch progress popup for all member jobs');
assert.match(backendSource, /export async function openBulkArchive\(archiveId: string\)/, 'backend should expose an archive-level open wrapper');
assert.match(backendSource, /export async function revealBulkArchive\(archiveId: string\)/, 'backend should expose an archive-level reveal wrapper');
assert.match(queueSource, /function bulkOpenLabel[\s\S]*Open Folder[\s\S]*Open File/, 'completed bulk aggregate menus should label folder outputs as Open Folder and archives as Open File');
assert.doesNotMatch(batchSource, /Reveal completed/, 'bulk progress popup should not expose the old Reveal completed action');
assert.match(batchSource, /Extracting archive/, 'bulk progress popup should show extraction as a distinct phase');
assert.match(batchSource, /Creating folder/, 'bulk progress popup should show folder finalization as a distinct label');
assert.match(batchSource, /Review links/, 'bulk progress popup should show the pre-download review phase');
assert.match(batchSource, /deleteJobs/, 'bulk progress popup should let the initial review state cancel the queued batch');
assert.match(batchSource, /ActionButton\('Start'[\s\S]*resumeJob/, 'bulk progress popup should start paused bulk jobs only after user confirmation');
assert.match(batchSource, /isBulkReviewPhase[\s\S]*ActionButton\('Cancel'[\s\S]*ActionButton\('Start'/, 'review footer should expose only Cancel and Start before active controls');
assert.doesNotMatch(batchSource, /isBulkReviewPhase[\s\S]{0,260}Pause all/, 'review footer should not show Pause all');
assert.doesNotMatch(batchSource, /isBulkReviewPhase[\s\S]{0,260}Resume all/, 'review footer should not show Resume all');
assert.doesNotMatch(batchSource, /isBulkReviewPhase[\s\S]{0,260}Cancel active/, 'review footer should not show Cancel active');
assert.match(batchSource, /archive\?\.warning/, 'bulk progress popup should surface cleanup warnings after a completed archive');
assert.match(batchSource, /bulkOpenLabel\(completedArchive\)/, 'completed bulk popup should use output-aware open labels');
assert.doesNotMatch(batchSource, /summary\.activeCount === 0[\s\S]{0,240}Pause all/, 'inactive batch popup footer should not keep disabled pause controls visible');
