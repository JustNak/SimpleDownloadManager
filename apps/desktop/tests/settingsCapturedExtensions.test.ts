import assert from 'node:assert/strict';
import { DEFAULT_CAPTURED_FILE_EXTENSIONS } from '../src/defaultSettings.ts';
import {
  addCapturedExtensions,
  filterCapturedExtensions,
  formatCapturedExtensionsSummary,
  normalizeCapturedExtensionInput,
  parseCapturedExtensionInput,
  removeCapturedExtension,
} from '../src/settingsCapturedExtensions.ts';

assert.equal(normalizeCapturedExtensionInput('.ZIP'), 'zip');
assert.equal(normalizeCapturedExtensionInput('7zip'), '7z');
assert.equal(normalizeCapturedExtensionInput('invalid/path'), '');
assert.equal(normalizeCapturedExtensionInput('bad value'), '');

assert.deepEqual(
  parseCapturedExtensionInput('zip, rar .EXE 7zip zip'),
  ['zip', 'rar', 'exe', '7z'],
  'captured extension input should normalize aliases, dots, case, and duplicates',
);

const added = addCapturedExtensions(['zip'], ['rar', '.ZIP', '7zip', 'invalid/path']);
assert.deepEqual(added.extensions, ['zip', 'rar', '7z']);
assert.deepEqual(added.addedExtensions, ['rar', '7z']);
assert.deepEqual(added.duplicateExtensions, ['zip']);

assert.deepEqual(removeCapturedExtension(added.extensions, '.rar'), ['zip', '7z']);
assert.deepEqual(filterCapturedExtensions(['zip', 'rar', 'docx'], 'do'), ['docx']);
assert.equal(formatCapturedExtensionsSummary([]), 'No captured extensions');
assert.equal(formatCapturedExtensionsSummary(['zip']), '1 captured extension');
assert.equal(formatCapturedExtensionsSummary(['zip', 'rar']), '2 captured extensions');
assert.equal(
  formatCapturedExtensionsSummary([...DEFAULT_CAPTURED_FILE_EXTENSIONS]),
  `${DEFAULT_CAPTURED_FILE_EXTENSIONS.length} default extensions`,
);
