import assert from 'node:assert/strict';
import {
  getDeleteContextMenuLabel,
  getDeletePromptContent,
} from '../src/deletePrompts.ts';

const single = getDeletePromptContent(1);
assert.equal(single.title, 'Delete Download');
assert.equal(single.description, 'Remove this download from the list. Disk deletion requires explicit confirmation below.');
assert.equal(single.checkboxLabel, 'Delete file from disk');
assert.equal(single.confirmLabel, 'Delete');
assert.equal(single.contextMenuLabel, 'Delete');
assert.equal(single.selectedSummary, '1 download selected');

const multi = getDeletePromptContent(3);
assert.equal(multi.title, 'Delete 3 Downloads');
assert.equal(multi.description, 'Remove these downloads from the list. Disk deletion requires explicit confirmation below.');
assert.equal(multi.checkboxLabel, 'Delete selected files from disk');
assert.equal(multi.confirmLabel, 'Delete All');
assert.equal(multi.contextMenuLabel, 'Delete All');
assert.equal(multi.selectedSummary, '3 downloads selected');

assert.equal(getDeleteContextMenuLabel(0), 'Delete');
assert.equal(getDeleteContextMenuLabel(2), 'Delete All');
