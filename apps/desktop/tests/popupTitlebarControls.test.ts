import assert from 'node:assert/strict';
import { popupTitlebarControls } from '../src/popupTitlebarControls.ts';

assert.deepEqual(
  popupTitlebarControls().map((control) => control.label),
  ['Minimize', 'Close'],
  'popup titlebars should expose minimize before close',
);
