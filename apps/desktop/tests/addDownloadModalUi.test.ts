import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/AddDownloadModal.svelte', import.meta.url), 'utf8');

assert.match(source, /bg-background\/60 p-4 backdrop-blur-\[1px\]/, 'modal backdrop should keep the Svelte translucent background and blur');
assert.match(source, /w-full max-w-xl[\s\S]*rounded-md[\s\S]*bg-card[\s\S]*animate-in fade-in zoom-in-95/, 'modal shell should keep the Svelte width, radius, card surface, and entrance animation');
assert.match(source, /New Download/, 'modal title should match the Svelte capitalization');
assert.match(source, /Add a file, torrent, link list, or bulk archive\./, 'modal subtitle should match the Svelte copy');
assert.match(source, /Close new download/, 'close button should keep the Svelte accessible label');
assert.match(source, /grid grid-cols-4 rounded-md border border-border bg-background p-1/, 'download mode tabs should use the compact Svelte segmented control');
for (const label of ['File', 'Torrent', 'Multi', 'Bulk']) {
  assert.match(source, new RegExp(`label: '${label}'`), `download mode label ${label} should match Svelte`);
}
assert.match(source, /browseTorrentFile/, 'torrent mode should wire the Import button to the native torrent import picker');
assert.match(source, /PackagePlus[\s\S]*Import/, 'torrent mode should render a compact Import button with an icon');
assert.match(source, /event\.target === event\.currentTarget/, 'the modal should only close when the backdrop itself is clicked');
assert.match(source, /event\.key === 'Escape'/, 'the modal should close when Escape is pressed');
assert.match(source, /document\.addEventListener\('keydown', closeOnEscape\)/, 'the modal should register a document Escape listener while open');
assert.match(source, /document\.removeEventListener\('keydown', closeOnEscape\)/, 'the modal should remove its Escape listener on unmount');
assert.match(source, /progressPopupIntentForSubmission/, 'added downloads should still return the popup intent used by the app shell');
assert.match(source, /mode === 'torrent'[\s\S]*transferKind: 'torrent'/, 'torrent submissions should keep the torrent transfer kind');
assert.match(source, /mode === 'multi'\)[\s\S]*addJobs\(urls\)/, 'multi submissions should keep plain batch queueing');
assert.match(source, /defaultBulkArchiveNameForUrls/, 'bulk archive names should be suggested from pasted multipart links');
assert.match(source, /bulkOutputKind/, 'bulk modal should track whether combine output is an archive or folder');
assert.match(source, /Archive[\s\S]*Folder/, 'bulk modal should offer Archive and Folder output choices');
assert.match(source, /setBulkOutputKind/, 'bulk modal should normalize the output name when switching Archive and Folder');
assert.match(source, /archiveNameTouched/, 'manual bulk archive names should not be overwritten by later pasted links');
assert.match(source, /addJobs\(urls, trimmedArchiveName, \{ resolveHosterLinks: true, startPaused: true, bulkOutputKind \}\)/, 'bulk submissions should resolve hoster links, pass output kind, and wait for explicit popup Start');
assert.match(source, /readyLabel/, 'footer should use the Svelte ready-label wording instead of generic item copy');
