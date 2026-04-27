import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/QueueView.tsx', import.meta.url), 'utf8');

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
  /grid min-h-\[42px\][\s\S]*px-3 py-1 text-left text-sm/,
  'queue rows should use the slimmer density requested from the download table comment',
);

assert.doesNotMatch(
  source,
  /grid min-h-\[50px\]/,
  'queue rows should not keep the previous taller default height',
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
