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
