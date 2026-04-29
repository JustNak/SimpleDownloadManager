import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/QueueView.tsx', import.meta.url), 'utf8');
const appSource = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');

assert.doesNotMatch(
  source,
  /<SortableColumnHeader column="size"[^>]*align="right"/,
  'the Size sort header should align with size values instead of being pushed toward Actions',
);

assert.match(
  source,
  /<SortableColumnHeader column="size" sortMode=\{sortMode\} onSortChange=\{onSortChange\}>/,
  'the Size header should remain sortable while using the default column alignment',
);

assert.match(
  source,
  /queueRowSizeClass\(queueRowSize\)/,
  'queue rows should read their density from the saved queue row-size setting',
);

assert.match(
  source,
  /case 'medium':[\s\S]*return 'min-h-\[42px\] py-1 text-sm'/,
  'medium queue row size should preserve the current default density',
);

assert.match(
  source,
  /case 'compact':[\s\S]*min-h-\[28px\] py-0 text-xs[\s\S]*case 'small':[\s\S]*min-h-\[34px\] py-0\.5 text-xs[\s\S]*case 'damn':[\s\S]*min-h-\[68px\] py-2\.5 text-base/,
  'queue row sizing should make every density affect whole-row height, from compact through DAMN',
);

assert.match(
  source,
  /rowSize=\{queueRowSize\}[\s\S]*activityState=\{fileBadgeActivityState/,
  'queue row file badges should scale with the selected row-size setting',
);

assert.match(
  source,
  /<InlineNameProgress[\s\S]*rowSize=\{queueRowSize\}/,
  'queue row name and progress content should scale with the selected row-size setting',
);

assert.match(
  source,
  /function inlineNameDensity[\s\S]*case 'compact':[\s\S]*container: 'px-2 py-0'[\s\S]*metaText: 'mt-0 text-\[10px\] leading-3'/,
  'compact rows should reduce internal name/progress padding and text leading, not only the file icon',
);

assert.match(
  source,
  /function fileBadgeDensity[\s\S]*case 'compact':[\s\S]*h-5 w-5[\s\S]*case 'damn':[\s\S]*h-12 w-12/,
  'file badge density should continue scaling with the row-size options',
);

assert.match(
  source,
  /selectedJob && showDetailsOnClick \? \(/,
  'selected-download details should render only when click-to-show details is enabled',
);

assert.match(
  source,
  /const DETAILS_MIN_HEIGHT = 104;/,
  'the selected-download details pane should support a slimmer minimum height',
);

assert.match(
  source,
  /const DETAILS_DEFAULT_HEIGHT = 128;/,
  'the selected-download details pane should open in a compact default height',
);

assert.match(
  source,
  /const compact = height <= DETAILS_DEFAULT_HEIGHT \+ 8;/,
  'the selected-download details pane should use compact layout at its default height',
);

assert.match(
  source,
  /shouldOpenJobFileOnDoubleClick\(job, event\.button\)[\s\S]*onOpen\(job\.id\);/,
  'queue row double-click should open the file instead of revealing the folder',
);

assert.match(
  source,
  /function toggleSingleJobSelection\(jobId: string\)[\s\S]*isOnlySelectedJob[\s\S]*clearJobSelection\(\);[\s\S]*selectSingleJob\(jobId\);/,
  'single row clicks should toggle off when the clicked row is the only active selection',
);

assert.match(
  source,
  /onClick=\{\(\) => \{[\s\S]*toggleSingleJobSelection\(job\.id\);[\s\S]*setContextMenu\(null\);/,
  'queue row click should use the toggle selection behavior',
);

assert.match(
  source,
  /event\.key === 'Enter' \|\| event\.key === ' '[\s\S]*toggleSingleJobSelection\(job\.id\);/,
  'keyboard row activation should match click toggling behavior',
);

assert.doesNotMatch(
  source,
  /shouldRevealJobDirectoryOnDoubleClick/,
  'queue row double-click should no longer use the reveal-folder helper',
);

assert.match(
  source,
  /label="Open Folder" onClick=\{\(\) => onReveal\(job\.id\)\}/,
  'the context menu Open Folder action should still reveal the file location',
);

assert.match(
  source,
  /grid min-w-\[1080px\] grid-flow-col auto-cols-\[minmax\(260px,1fr\)\] grid-rows-2 gap-x-3 gap-y-2/,
  'compact details should use self-contained scrolling detail cells with enough width for each value',
);

assert.match(
  source,
  /function CompactDetailItem[\s\S]*className="min-w-0 px-1 py-1"/,
  'compact detail items should be unframed rather than card-like boxes',
);

assert.doesNotMatch(
  source,
  /className="min-w-0 rounded-sm border border-border\/70 bg-background\/35 px-2\.5 py-1\.5"/,
  'compact detail items should not keep the previous card-style wrapper',
);

assert.match(
  source,
  /title=\{value\}[\s\S]*\{value\}/,
  'compact detail values should keep the full value available as hover text while truncating visually',
);

assert.doesNotMatch(
  source,
  /h-\[3px\] rounded-full/,
  'inline row progress should not use the previous thin hairline strip',
);

assert.match(
  source,
  /absolute \$\{density\.progressInset\} left-0 z-0 rounded-\[inherit\] blur-md/,
  'inline row progress should use a density-aware blurred background wash behind the row text',
);

assert.match(
  source,
  /activityState=\{fileBadgeActivityState\(job, recentlyCompletedJobIds\.has\(job\.id\)\)\}/,
  'queue rows should pass the computed file badge activity state into FileBadge',
);

assert.match(
  source,
  /activityState = 'none'[\s\S]*LoaderCircle[\s\S]*animate-spin[\s\S]*Check/,
  'FileBadge should support buffering spinner and completed check overlays',
);

assert.match(
  source,
  /const COMPLETED_BADGE_DURATION_MS = 1200;/,
  'completed file badge overlay should use the requested 1200ms duration',
);

assert.match(
  source,
  /<SortableColumnHeader column="date"[\s\S]*className=\{torrentColumnAlignClass\(isTorrentTable\)\}/,
  'torrent table Date header should be centered with the Date cells',
);

assert.match(
  source,
  /<div title=\{isTorrentTable \? 'Seed upload speed' : undefined\} className=\{torrentColumnAlignClass\(isTorrentTable\)\}>/,
  'torrent table Seed header should be centered with the Seed cells',
);

assert.match(
  source,
  /className=\{queueDateCellClass\(isTorrentTable\)\}/,
  'torrent table Date cells should share a centered alignment class',
);

assert.match(
  source,
  /className=\{queueMetricCellClass\(isTorrentTable\)\}/,
  'torrent table Seed metric cells should share a centered alignment class',
);

assert.doesNotMatch(
  source,
  /function formatTorrentSeedMetric[\s\S]*return `Up \$\{formatBytes\(job\.torrent\.uploadedBytes\)\}`;/,
  'torrent table Seed cells should omit the Up prefix and show only the uploaded byte value',
);

assert.equal(
  (source.match(/label="Show Popup"/g) ?? []).length,
  2,
  'Show Popup should be available in both row action menus and right-click context menus',
);

assert.equal(
  (source.match(/canShowProgressPopup\(job\) \? \(/g) ?? []).length,
  2,
  'both Show Popup menu entries should be gated to active download states',
);

assert.match(
  source,
  /onShowPopup: \(id: string\) => void;/,
  'QueueView should accept an explicit Show Popup callback',
);

assert.match(
  appSource,
  /async function handleShowPopup\(id: string\)[\s\S]*await openProgressWindow\(id\)/,
  'App should wire Show Popup through the existing progress popup opener',
);

assert.match(
  appSource,
  /onShowPopup=\{handleShowPopup\}/,
  'QueueView should receive the Show Popup handler from App',
);
