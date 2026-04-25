import assert from 'node:assert/strict';
import {
  batchUrlTextAreaClassName,
  batchUrlTextAreaWrap,
  downloadSubmitLabel,
  ensureTrailingEditableLine,
  parseDownloadUrlLines,
} from '../src/downloadInput.ts';

const longSignedUrl = 'https://store-044.wnam.tb-cdn.io/zip/067d34b2-b6b8-4324-b795-3b45544d9dfb?token=ea24bba1-eba0-4a5d-92cd-bbe07d59b864';

assert.deepEqual(
  parseDownloadUrlLines(longSignedUrl),
  [longSignedUrl],
  'a long signed URL should remain one queue item unless it contains a real newline',
);

assert.deepEqual(
  parseDownloadUrlLines(`${longSignedUrl}\nhttps://example.com/second.zip`),
  [longSignedUrl, 'https://example.com/second.zip'],
  'only physical newlines should create separate queue items',
);

assert.equal(
  ensureTrailingEditableLine(''),
  '',
  'empty batch URL inputs should stay empty so the placeholder remains visible',
);

assert.equal(
  ensureTrailingEditableLine(longSignedUrl),
  `${longSignedUrl}\n`,
  'a pasted URL should get a real editable blank line after it',
);

assert.deepEqual(
  parseDownloadUrlLines(ensureTrailingEditableLine(`${longSignedUrl}\nhttps://example.com/second.zip`)),
  [longSignedUrl, 'https://example.com/second.zip'],
  'the trailing editable blank line should not create an extra download item',
);

assert.equal(batchUrlTextAreaWrap, 'off', 'batch URL inputs should not soft-wrap long links');
assert.ok(batchUrlTextAreaClassName.includes('whitespace-pre'), 'batch URL inputs should preserve physical lines');
assert.ok(batchUrlTextAreaClassName.includes('overflow-x-auto'), 'batch URL inputs should scroll horizontally');

assert.equal(downloadSubmitLabel('single', 1, true), 'Start Download');
assert.equal(downloadSubmitLabel('multi', 1, true), 'Queue 1 Download');
assert.equal(downloadSubmitLabel('multi', 2, true), 'Queue 2 Downloads');
assert.equal(downloadSubmitLabel('bulk', 1, true), 'Queue 1 Download and Combine');
assert.equal(downloadSubmitLabel('bulk', 2, false), 'Queue 2 Downloads');
