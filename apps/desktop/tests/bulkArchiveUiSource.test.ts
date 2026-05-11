import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const appSource = readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8');
const backendSource = readFileSync(new URL('../src/backend.ts', import.meta.url), 'utf8');
const backendPreviewSource = readFileSync(new URL('../src/backendPreview.ts', import.meta.url), 'utf8');
const queueSource = readFileSync(new URL('../src/QueueView.svelte', import.meta.url), 'utf8');
const batchSource = readFileSync(new URL('../src/BatchProgressWindow.svelte', import.meta.url), 'utf8');
const bulkFooterSource = batchSource.match(/\{#snippet BulkFooter\(\)\}([\s\S]*?)\{\/snippet\}/)?.[1] ?? '';
assert.ok(bulkFooterSource, 'bulk progress popup should define a BulkFooter snippet');

assert.match(appSource, /groupBulkQueueRows\(jobs\)/, 'App should group bulk members before queue counts, filtering, sorting, and rendering');
assert.match(appSource, /openBatchProgressWindow\(\{[\s\S]*kind: 'bulk'[\s\S]*bulkMemberIds/, 'bulk aggregate Show Popup should open the batch progress popup for all member jobs');
assert.match(backendSource, /export async function openBulkArchive\(archiveId: string\)/, 'backend should expose an archive-level open wrapper');
assert.match(backendSource, /export async function revealBulkArchive\(archiveId: string\)/, 'backend should expose an archive-level reveal wrapper');
const completedBulkRowActionBranch = queueSource.match(
  /\{#if isCompletedBulkAggregate\(job\)\}([\s\S]*?)\{:else if isBulkAggregateJob\(job\) && canShowBulkPrimaryAction\(job\)\}/,
)?.[1] ?? '';
assert.ok(completedBulkRowActionBranch, 'completed bulk aggregate rows should define a dedicated row action branch');
assert.match(completedBulkRowActionBranch, /title="Show"[\s\S]*onclick=\{\(\) => onReveal\(job\.id\)\}/, 'completed bulk aggregate rows should keep the Show action');
assert.doesNotMatch(completedBulkRowActionBranch, /bulkOpenLabel|onOpen\(job\.id\)|FileArchive/, 'completed bulk aggregate rows should not render a second Open Folder action');
const completedBulkMenuBranch = queueSource.match(
  /\{#if isCompletedBulkAggregate\(job\)\}([\s\S]*?)\{:else if isFailedBulkAggregate\(job\)\}/,
)?.[1] ?? '';
assert.ok(completedBulkMenuBranch, 'completed bulk aggregate menus should define a dedicated menu branch');
assert.match(completedBulkMenuBranch, /MenuItem\(FolderOpen, 'Show'[\s\S]*onReveal\(job\.id\)/, 'completed bulk aggregate menus should keep Show');
assert.doesNotMatch(completedBulkMenuBranch, /bulkOpenLabel|Open Folder|onOpen\(job\.id\)|FileArchive/, 'completed bulk aggregate menus should not render a duplicate Open Folder item');
assert.match(queueSource, /expandedBulkRowIds/, 'bulk aggregate rows should keep inline expansion state in the queue view');
assert.match(queueSource, /bulkMembersByArchiveId/, 'bulk aggregate expansion should read member file data from a lookup');
assert.match(queueSource, /bulkMembersForJob/, 'bulk aggregate expansion should resolve member rows without cloned aggregate data');
assert.match(queueSource, /bulkMemberVirtualQueue/, 'bulk aggregate expansion should virtualize large inline member lists');
assert.match(queueSource, /type="checkbox"[\s\S]*setBulkMemberIncluded/, 'bulk review rows should keep per-member include checkboxes inline');
assert.match(queueSource, /excludedBulkMemberIds/, 'bulk review rows should track excluded member files in the main bulk view');
assert.match(queueSource, /startBulkReview/, 'the main bulk view should start included members and remove excluded queued members');
assert.match(queueSource, /function bulkPrimaryActionLabel[\s\S]*\? 'Start' : 'Resume'/, 'bulk review groups should expose Start on the aggregate row');
assert.match(queueSource, /function runBulkPrimaryAction[\s\S]*startBulkReview\(job\)/, 'bulk review Start should run the include/exclude review flow');
assert.doesNotMatch(queueSource, /Bulk[\s\S]{0,220}ETA|ETA[\s\S]{0,220}Bulk/, 'bulk queue UI should not label ETA/time estimates');
assert.match(
  queueSource,
  /function canExpandBulkAggregate\(job: QueueDisplayJob\): job is BulkAggregateDownloadJob \{[\s\S]*!isCompletedBulkAggregate\(job\)/,
  'completed bulk aggregates should not keep the inline member dropdown after finalization finishes',
);
assert.match(
  queueSource,
  /jobs\.filter\(canExpandBulkAggregate\)[\s\S]*expandedBulkRowIds\.delete\(id\)/,
  'completed bulk aggregates should be pruned from expanded row state',
);
assert.match(
  queueSource,
  /\{#if isBulkTable && canExpandBulkAggregate\(job\)\}[\s\S]*BulkChevron/,
  'completed bulk aggregates should not render the inline dropdown trigger',
);
const bulkExpansionSource = queueSource.match(
  /\{#if isBulkTable && canExpandBulkAggregate\(job\) && expandedBulkRowIds\.has\(job\.id\)\}([\s\S]*?)\r?\n            \{\/if\}\r?\n          \{\/each\}/,
)?.[1] ?? '';
assert.ok(bulkExpansionSource, 'bulk queue should render an inline expansion block');
assert.doesNotMatch(bulkExpansionSource, /files included|includedBulkMemberCount/, 'bulk inline expansion should not render a separate review summary/start strip');
assert.doesNotMatch(bulkExpansionSource, /<FileText|<Play/, 'bulk inline expansion should stay text-first without decorative SVG icons');
assert.match(bulkExpansionSource, /text-\[11px\]/, 'bulk inline expansion should use a compact detail density');
assert.match(bulkExpansionSource, /py-1/, 'bulk inline member rows should use compact vertical padding');
assert.doesNotMatch(batchSource, /Reveal completed/, 'bulk progress popup should not expose the old Reveal completed action');
assert.match(batchSource, /Uncompressing/, 'bulk progress popup should show uncompressing as a distinct finalizing phase');
assert.match(batchSource, /Combining/, 'bulk progress popup should show combining as a distinct finalizing phase');
assert.doesNotMatch(batchSource, /Compressing/, 'bulk folder output should not expose a compression finalizing phase');
assert.match(batchSource, /Review links/, 'bulk progress popup should show the pre-download review phase');
assert.match(batchSource, /deleteJobs/, 'bulk progress popup should let the initial review state cancel the queued batch');
assert.match(batchSource, /function startBulkDownload\(\)[\s\S]*resumeJobs/, 'bulk progress popup should start paused bulk jobs only after user confirmation');
assert.match(bulkFooterSource, /<div class="mt-3 flex min-h-\[45px\] shrink-0 items-center justify-between gap-3 border-t border-border pt-3">/, 'bulk footer should split left cancel actions from right primary actions');
assert.match(bulkFooterSource, /<div class="flex justify-start">[\s\S]*bulkUiState === 'review' \|\| bulkUiState === 'downloading'[\s\S]*ActionButton\(isConfirmingCancel \? 'Confirm delete' : 'Cancel'/, 'bulk footer should render Cancel/Confirm delete in the left footer slot');
assert.match(bulkFooterSource, /<div class="flex justify-end gap-3">[\s\S]*bulkUiState === 'review'[\s\S]*ActionButton\('Start'/, 'review footer should render Start in the right footer slot');
assert.match(bulkFooterSource, /<div class="flex justify-end gap-3">[\s\S]*bulkUiState === 'downloading'[\s\S]*ActionButton\(canPause \? 'Pause' : 'Resume'/, 'downloading footer should render Pause/Resume in the right footer slot');
assert.doesNotMatch(batchSource, /isBulkReviewPhase[\s\S]{0,260}Pause all/, 'review footer should not show Pause all');
assert.doesNotMatch(batchSource, /isBulkReviewPhase[\s\S]{0,260}Resume all/, 'review footer should not show Resume all');
assert.doesNotMatch(batchSource, /isBulkReviewPhase[\s\S]{0,260}Cancel active/, 'review footer should not show Cancel active');
assert.match(batchSource, /archive\?\.warning/, 'bulk progress popup should surface cleanup warnings after completed folder finalization');
assert.doesNotMatch(batchSource, /summary\.activeCount === 0[\s\S]{0,240}Pause all/, 'inactive batch popup footer should not keep disabled pause controls visible');
assert.match(batchSource, /selectedBulkJobIds/, 'bulk review rows should track local checked include state');
assert.match(batchSource, /type="checkbox"/, 'bulk review rows should expose per-file include checkboxes');
assert.match(batchSource, /bulkUiState === 'failed'[\s\S]*BulkFailedRetryList/, 'failed bulk popup should render selectable retry member rows');
assert.match(batchSource, /function retryFailedBulkArchive[\s\S]*deleteJobs\(selection\.excludedJobIds,\s*true\)[\s\S]*retryBulkArchive\(archiveId\)/, 'failed bulk retry should delete unchecked members from disk before retrying the archive');
assert.match(batchSource, /Retry folder[\s\S]*!failedRetrySelection\.canRetry/, 'failed bulk retry should be disabled unless at least two members remain selected');
assert.doesNotMatch(batchSource, /bulkHasStarted\s*&&\s*rawBulkUiState === 'review'[\s\S]*\?\s*'downloading'/, 'bulk popup should not locally force review into downloading after Start');
assert.doesNotMatch(batchSource, /\bbulkHasStarted\b/, 'bulk popup should not keep local started override state');
assert.match(batchSource, /isUntouchedBulkReviewGate/, 'bulk popup should derive the first review gate from untouched pending jobs');
assert.match(batchSource, /bulkReviewStartSelection/, 'bulk Start should partition included and excluded review rows');
assert.match(batchSource, /deleteJobs\(selection\.excludedJobs\.map\(\(job\) => job\.id\), false\)/, 'bulk Start should remove unchecked review rows without deleting files from disk');
assert.match(batchSource, /bulkCancelConfirmPlan/, 'bulk Cancel confirmation should use the tested bulk cleanup plan');
assert.doesNotMatch(batchSource, /deleteJobs\(plan\.deleteJobIds,\s*true\)/, 'bulk Cancel confirmation should not delete popup batch members from disk');
assert.match(batchSource, /cancelJobs\(plan\.deleteJobIds,\s*\{\s*deleteFromDisk:\s*plan\.deleteFromDisk\s*\}\)/, 'bulk Cancel confirmation should ask the cancel command to delete visible batch files');
assert.match(batchSource, /closeOnSuccess:\s*plan\.closeOnSuccess/, 'successful bulk Cancel confirmation should close the popup after scheduling cancellation and disk cleanup');
assert.match(batchSource, /bulkUiState === 'canceled'[\s\S]*ActionButton\('Close'/, 'canceled bulk popup should expose only Close in the footer');
assert.match(batchSource, /canBulkCancel/, 'bulk downloading Cancel should remain available when popup jobs can be canceled');
assert.match(batchSource, /isBusy \|\| !canBulkCancel/, 'bulk downloading Cancel should disable only when no popup jobs can be canceled');
const reviewFooterBranch = bulkFooterSource.match(/bulkUiState === 'review'[\s\S]*?ActionButton\('Start'[\s\S]*?\{:else if bulkUiState === 'downloading'\}/)?.[0] ?? '';
assert.ok(reviewFooterBranch, 'bulk review footer branch should render the Start action before downloading controls');
assert.doesNotMatch(reviewFooterBranch, /Resume/, 'bulk review footer should say Start, not Resume');
assert.match(batchSource, /isConfirmingCancel/, 'bulk Cancel should use the same two-step confirmation transition as other progress popups');
assert.match(batchSource, /bulkUiState === 'finalizing'[\s\S]*BulkFinalizingStrip/, 'bulk finalizing state should render the adaptive no-action phase strip');
assert.doesNotMatch(batchSource, /bulkUiState === 'finalizing'[\s\S]{0,360}ActionButton/, 'bulk finalizing state should not render footer actions');
assert.doesNotMatch(batchSource, /ActionButton\(bulkOpenLabel\(completedArchive\)/, 'ready bulk popup should expose only Show, not a second Open action');
assert.match(backendPreviewSource, /width=640,height=480/, 'browser fallback batch progress popup should use the redesigned 640x480 size');
