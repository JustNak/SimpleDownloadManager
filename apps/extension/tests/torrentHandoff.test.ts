import assert from 'node:assert/strict';
import { createEnqueueDownloadRequest, createPromptDownloadRequest } from '@myapp/protocol';
import { shouldHandOffTorrentBrowserDownload } from '../src/background/torrentHandoff.ts';

const source = {
  entryPoint: 'context_menu' as const,
  browser: 'chrome' as const,
  extensionVersion: '0.3.0',
};

assert.equal(
  createEnqueueDownloadRequest(
    'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example',
    source,
    'request_1',
  ).ok,
  true,
  'context menu handoff should accept magnet links',
);

assert.equal(
  createPromptDownloadRequest(
    'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example',
    source,
    {},
    'request_2',
  ).ok,
  true,
  'prompt handoff should accept magnet links',
);

const enqueueWithMetadata = createEnqueueDownloadRequest(
  'https://example.com/download?id=123',
  source,
  'request_metadata',
  {
    suggestedFilename: 'nested/report%20final.pdf',
    totalBytes: 1024.75,
  },
);
assert.equal(enqueueWithMetadata.ok, true, 'enqueue handoff should accept browser download metadata');
if (enqueueWithMetadata.ok) {
  assert.equal(
    enqueueWithMetadata.value.payload.suggestedFilename,
    'nested/report%20final.pdf',
    'enqueue handoff should preserve a bounded suggested filename for desktop duplicate checks',
  );
  assert.equal(
    enqueueWithMetadata.value.payload.totalBytes,
    1024,
    'enqueue handoff should normalize positive total bytes like prompt handoff',
  );
}

assert.equal(
  shouldHandOffTorrentBrowserDownload({
    url: 'https://example.com/releases/example.torrent',
    filename: 'example.torrent',
  }),
  true,
  '.torrent browser downloads should be handed off',
);

assert.equal(
  shouldHandOffTorrentBrowserDownload({
    url: 'https://example.com/releases/example.zip',
    filename: 'example.zip',
  }),
  false,
  'non-torrent browser downloads should use normal HTTP rules',
);
