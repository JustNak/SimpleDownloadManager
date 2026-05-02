import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/QueueView.svelte', import.meta.url), 'utf8');
const appSource = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');

assert.match(source, /button class="text-left hover:text-foreground" onclick=\{\(\) => setSort\('size'\)\}>Size/, 'the Size header should remain sortable while using the default column alignment');
assert.match(source, /queueRowSizeClass\(queueRowSize\)/, 'queue rows should read their density from the saved queue row-size setting');
assert.match(source, /default:[\s\S]*return 'min-h-\[42px\] py-1 text-sm'/, 'medium queue row size should preserve the current default density');
assert.match(source, /case 'compact':[\s\S]*min-h-\[28px\] py-0 text-xs[\s\S]*case 'small':[\s\S]*min-h-\[34px\] py-0\.5 text-xs[\s\S]*case 'damn':[\s\S]*min-h-\[68px\] py-2\.5 text-base/, 'queue row sizing should make every density affect whole-row height, from compact through DAMN');
assert.match(source, /size=\{queueRowSize === 'damn' \? 'lg' : queueRowSize === 'compact' \? 'sm' : 'md'\}/, 'queue row file badges should scale with the selected row-size setting');
assert.match(source, /selectedJob && showDetailsOnClick/, 'selected-download details should render only when click-to-show details is enabled');
assert.match(source, /ondblclick=\{\(event\) => event\.button === 0 && onOpen\(job\.id\)\}/, 'queue row double-click should open the file instead of revealing the folder');
assert.match(source, /function selectRow\(job: DownloadJob/, 'queue row click should use the Svelte selection helper');
assert.match(source, /Open Folder[\s\S]*onReveal\(job\.id\)/, 'the context menu Open Folder action should still reveal the file location');
assert.match(source, /grid min-w-\[1080px\] grid-flow-col auto-cols-\[minmax\(260px,1fr\)\] grid-rows-2 gap-x-3 gap-y-2/, 'compact details should use self-contained scrolling detail cells with enough width for each value');
assert.match(source, /#snippet CompactDetailItem[\s\S]*class="min-w-0 px-1 py-1"/, 'compact detail items should be unframed rather than card-like boxes');
assert.doesNotMatch(source, /rounded-sm border border-border\/70 bg-background\/35 px-2\.5 py-1\.5/, 'compact detail items should not keep the previous card-style wrapper');
assert.match(source, /title=\{value\}>\{value\}/, 'compact detail values should keep the full value available as hover text while truncating visually');
assert.equal((source.match(/Show Popup/g) ?? []).length, 2, 'Show Popup should be available in both row action menus and right-click context menus');
assert.equal((source.match(/canShowProgressPopup\(job\)/g) ?? []).length, 2, 'both Show Popup menu entries should be gated to active download states');
assert.match(source, /onShowPopup: \(id: string\) => void;/, 'QueueView should accept an explicit Show Popup callback');
assert.match(appSource, /async function handleShowPopup\(id: string\)[\s\S]*await openProgressWindow\(id\)/, 'App should wire Show Popup through the existing progress popup opener');
assert.match(appSource, /onShowPopup=\{\(id\) => void handleShowPopup\(id\)\}/, 'QueueView should receive the Show Popup handler from App');
