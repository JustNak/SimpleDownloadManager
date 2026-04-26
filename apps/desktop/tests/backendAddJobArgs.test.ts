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

assert.deepEqual(
  buildAddJobCommandArgs('magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example'),
  {
    url: 'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example',
    expectedSha256: null,
    transferKind: 'torrent',
  },
  'addJob should mark magnet links as torrent transfers',
);

assert.deepEqual(
  buildAddJobCommandArgs('https://example.com/example.torrent'),
  {
    url: 'https://example.com/example.torrent',
    expectedSha256: null,
    transferKind: 'torrent',
  },
  'addJob should mark .torrent URLs as torrent transfers',
);

assert.deepEqual(
  buildAddJobCommandArgs('https://example.com/example.torrent', { transferKind: 'torrent' }),
  {
    url: 'https://example.com/example.torrent',
    expectedSha256: null,
    transferKind: 'torrent',
  },
  'addJob should pass explicit torrent transfer intent through command args',
);

assert.deepEqual(
  buildAddJobCommandArgs('C:\\Users\\You\\Downloads\\example.torrent', { transferKind: 'torrent' }),
  {
    url: 'C:\\Users\\You\\Downloads\\example.torrent',
    expectedSha256: null,
    transferKind: 'torrent',
  },
  'addJob should pass explicit local torrent file intent through command args',
);
