import assert from 'node:assert/strict';
import { browserDownloadFilenameSuggestion } from '../src/background/browserDownloads.ts';

assert.deepEqual(
  browserDownloadFilenameSuggestion({
    filename: 'C:\\Users\\Alice\\Downloads\\archive.zip',
    url: 'https://downloads.example.com/archive.zip',
  }),
  { filename: 'archive.zip', conflictAction: 'uniquify' },
  'Chromium filename suggestions should use a basename, not an absolute local path',
);

assert.deepEqual(
  browserDownloadFilenameSuggestion({
    url: 'https://downloads.example.com/releases/setup.exe?token=abc',
  }),
  { filename: 'setup.exe', conflictAction: 'uniquify' },
  'Chromium filename suggestions should fall back to the URL basename',
);

assert.equal(
  browserDownloadFilenameSuggestion({
    url: 'https://downloads.example.com/',
  }),
  undefined,
  'downloads without a basename should release the browser default suggestion',
);

