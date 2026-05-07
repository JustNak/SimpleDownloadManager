import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/AddDownloadModal.svelte', import.meta.url), 'utf8');

assert.match(source, /bg-background\/60 p-4 backdrop-blur-\[1px\]/, 'modal backdrop should keep the Svelte translucent background and blur');
assert.match(source, /w-full max-w-xl[\s\S]*rounded-md[\s\S]*bg-card[\s\S]*animate-in fade-in zoom-in-95/, 'modal shell should keep the Svelte width, radius, card surface, and entrance animation');
assert.match(source, /New Download/, 'modal title should match the Svelte capitalization');
assert.match(source, /Add a file, torrent, or bulk links\./, 'modal subtitle should match the Svelte copy');
assert.match(source, /Close new download/, 'close button should keep the Svelte accessible label');
assert.match(source, /grid grid-cols-3 rounded-md border border-border bg-background p-1/, 'download mode tabs should use the compact Svelte segmented control');
for (const label of ['File', 'Torrent', 'Bulk']) {
  assert.match(source, new RegExp(`label: '${label}'`), `download mode label ${label} should match Svelte`);
}
assert.doesNotMatch(source, /label: 'Multi'|mode === 'multi'|multiUrls|multi-download-urls/, 'multi-download mode should not be exposed in the add-download modal');
assert.match(source, /browseTorrentFile/, 'torrent mode should wire the Import button to the native torrent import picker');
assert.match(source, /PackagePlus[\s\S]*Import/, 'torrent mode should render a compact Import button with an icon');
assert.match(source, /event\.target === event\.currentTarget/, 'the modal should only close when the backdrop itself is clicked');
assert.match(source, /event\.key === 'Escape'/, 'the modal should close when Escape is pressed');
assert.match(source, /document\.addEventListener\('keydown', closeOnEscape\)/, 'the modal should register a document Escape listener while open');
assert.match(source, /document\.removeEventListener\('keydown', closeOnEscape\)/, 'the modal should remove its Escape listener on unmount');
assert.match(source, /progressPopupIntentForSubmission/, 'added downloads should still return the popup intent used by the app shell');
assert.match(source, /mode === 'torrent'[\s\S]*transferKind: 'torrent'/, 'torrent submissions should keep the torrent transfer kind');
assert.match(source, /defaultBulkArchiveNameForUrls/, 'bulk archive names should be suggested from pasted multipart links');
assert.match(source, /bulkOutputKind/, 'bulk modal should track whether combine output is an archive or folder');
assert.match(source, /Archive[\s\S]*Folder/, 'bulk modal should offer Archive and Folder output choices');
assert.match(source, /setBulkOutputKind/, 'bulk modal should normalize the output name when switching Archive and Folder');
assert.match(source, /File Combine/, 'bulk combine label should use the concise File Combine wording');
assert.match(source, /<span class="min-w-0 whitespace-nowrap">File Combine<\/span>/, 'bulk combine label should stay on one line in the compact label column');
assert.doesNotMatch(source, /Combine downloads/, 'bulk combine should not use the old Combine downloads label');
assert.match(source, /title="Save as one archive or folder\."/, 'bulk combine helper copy should live in a hover tooltip instead of visible helper text');
assert.doesNotMatch(source, /<span class="mt-1 block text-xs leading-5 text-muted-foreground">Save as one archive or folder\.<\/span>/, 'bulk combine should not render a visible description under the label');
assert.match(source, /md:grid-cols-\[minmax\(0,132px\)_minmax\(0,1fr\)\]/, 'bulk combine row should keep a compact File Combine label column and wide controls column');
assert.match(source, /<input class="h-9 min-w-0 flex-1[\s\S]*aria-label="Bulk output name"[\s\S]*<div class="grid w-full shrink-0 grid-cols-2 rounded-md border border-border bg-card p-0\.5 sm:w-\[136px\] sm:rounded-l-none">/, 'bulk output name input should be the flexible first segment with compact integrated Archive and Folder controls after it');
assert.doesNotMatch(source, /md:grid-cols-\[170px_minmax\(0,1fr\)\]/, 'bulk output controls should not reserve a fixed grid column before the name input');
assert.match(source, /archiveNameTouched/, 'manual bulk archive names should not be overwritten by later pasted links');
assert.match(source, /addJobs\(urls, trimmedArchiveName, \{ resolveHosterLinks: true, startPaused: true, bulkOutputKind \}\)/, 'bulk submissions should resolve hoster links, pass output kind, and wait for explicit popup Start');
assert.match(source, /Resolving links\.\.\./, 'bulk submissions should show resolver-specific wait feedback while hoster links are being prepared');
assert.match(source, /readyLabel/, 'footer should use the Svelte ready-label wording instead of generic item copy');
