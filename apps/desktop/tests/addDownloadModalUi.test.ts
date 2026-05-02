import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/AddDownloadModal.svelte', import.meta.url), 'utf8');

assert.match(source, /browseTorrentFile/, 'torrent mode should wire the Import button to the native torrent import picker');
assert.match(source, /PackagePlus[\s\S]*Import/, 'torrent mode should render a compact Import button with an icon');
assert.match(source, /event\.target === event\.currentTarget/, 'the modal should only close when the backdrop itself is clicked');
assert.match(source, /event\.key === 'Escape'/, 'the modal should close when Escape is pressed');
assert.match(source, /document\.addEventListener\('keydown', closeOnEscape\)/, 'the modal should register a document Escape listener while open');
assert.match(source, /document\.removeEventListener\('keydown', closeOnEscape\)/, 'the modal should remove its Escape listener on unmount');
assert.match(source, /progressPopupIntentForSubmission/, 'added downloads should still return the popup intent used by the app shell');
assert.match(source, /mode === 'torrent'[\s\S]*transferKind: 'torrent'/, 'torrent submissions should keep the torrent transfer kind');
