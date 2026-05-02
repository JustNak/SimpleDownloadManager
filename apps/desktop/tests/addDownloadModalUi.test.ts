import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/AddDownloadModal.svelte', import.meta.url), 'utf8');

assert.match(source, /bg-background\/60 p-4 backdrop-blur-\[1px\]/, 'modal backdrop should keep the React translucent background and blur');
assert.match(source, /w-full max-w-xl[\s\S]*rounded-md[\s\S]*bg-card[\s\S]*animate-in fade-in zoom-in-95/, 'modal shell should keep the React width, radius, card surface, and entrance animation');
assert.match(source, /New Download/, 'modal title should match the React capitalization');
assert.match(source, /Add a file, torrent, link list, or bulk archive\./, 'modal subtitle should match the React copy');
assert.match(source, /Close new download/, 'close button should keep the React accessible label');
assert.match(source, /grid grid-cols-4 rounded-md border border-border bg-background p-1/, 'download mode tabs should use the compact React segmented control');
for (const label of ['File', 'Torrent', 'Multi', 'Bulk']) {
  assert.match(source, new RegExp(`label: '${label}'`), `download mode label ${label} should match React`);
}
assert.match(source, /browseTorrentFile/, 'torrent mode should wire the Import button to the native torrent import picker');
assert.match(source, /PackagePlus[\s\S]*Import/, 'torrent mode should render a compact Import button with an icon');
assert.match(source, /event\.target === event\.currentTarget/, 'the modal should only close when the backdrop itself is clicked');
assert.match(source, /event\.key === 'Escape'/, 'the modal should close when Escape is pressed');
assert.match(source, /document\.addEventListener\('keydown', closeOnEscape\)/, 'the modal should register a document Escape listener while open');
assert.match(source, /document\.removeEventListener\('keydown', closeOnEscape\)/, 'the modal should remove its Escape listener on unmount');
assert.match(source, /progressPopupIntentForSubmission/, 'added downloads should still return the popup intent used by the app shell');
assert.match(source, /mode === 'torrent'[\s\S]*transferKind: 'torrent'/, 'torrent submissions should keep the torrent transfer kind');
assert.match(source, /readyLabel/, 'footer should use the React ready-label wording instead of generic item copy');
