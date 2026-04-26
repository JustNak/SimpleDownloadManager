import assert from 'node:assert/strict';
import { buildAddJobCommandArgs } from '../src/backendCommandArgs.ts';

assert.deepEqual(
  buildAddJobCommandArgs('https://example.com/file.zip'),
  { url: 'https://example.com/file.zip', expectedSha256: null },
  'addJob should remain compatible when no checksum is supplied',
);

assert.deepEqual(
  buildAddJobCommandArgs(
    'https://example.com/file.zip',
    'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA',
  ),
  {
    url: 'https://example.com/file.zip',
    expectedSha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  },
  'addJob should pass normalized checksum command args when supplied',
);
