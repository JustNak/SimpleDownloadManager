import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/AddDownloadModal.tsx', import.meta.url), 'utf8');

assert.match(
  source,
  /function TorrentFileIcon/,
  'torrent import should use a dedicated torrent-file SVG icon',
);

assert.match(
  source,
  /browseTorrentFile/,
  'torrent mode should wire the Import button to the native torrent import picker',
);

assert.match(
  source,
  /'Import'/,
  'torrent mode should render a compact Import button',
);

assert.match(
  source,
  /handleBackdropMouseDown/,
  'the modal should have explicit backdrop click handling',
);

assert.match(
  source,
  /event\.target === event\.currentTarget/,
  'the modal should only close when the backdrop itself is clicked',
);
