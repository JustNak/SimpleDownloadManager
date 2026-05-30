import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createEnqueueDownloadRequest, createPromptDownloadRequest } from '@myapp/protocol';

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

const enqueueTorrentByUrl = createEnqueueDownloadRequest(
  'https://example.com/releases/example.torrent',
  source,
  'request_torrent_url',
  { transferKind: 'torrent' },
);
assert.equal(enqueueTorrentByUrl.ok, true, 'enqueue handoff should accept explicit torrent metadata for .torrent URLs');
if (enqueueTorrentByUrl.ok) {
  assert.equal(
    enqueueTorrentByUrl.value.payload.transferKind,
    'torrent',
    'enqueue handoff should preserve explicit torrent transfer kind for desktop routing',
  );
}

const enqueueTorrentByFilename = createEnqueueDownloadRequest(
  'https://example.com/download?id=123',
  source,
  'request_torrent_filename',
  {
    suggestedFilename: 'linux.iso.torrent',
    transferKind: 'torrent',
  },
);
assert.equal(enqueueTorrentByFilename.ok, true, 'enqueue handoff should accept torrent filename metadata');
if (enqueueTorrentByFilename.ok) {
  assert.equal(
    enqueueTorrentByFilename.value.payload.transferKind,
    'torrent',
    'enqueue handoff should preserve torrent metadata when only the suggested filename exposes .torrent',
  );
}

const promptTorrentMetadata = createPromptDownloadRequest(
  'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example',
  source,
  { transferKind: 'torrent' },
  'request_prompt_torrent',
);
assert.equal(promptTorrentMetadata.ok, true, 'prompt payloads should remain backward-compatible with torrent metadata');
if (promptTorrentMetadata.ok) {
  assert.equal(
    promptTorrentMetadata.value.payload.transferKind,
    'torrent',
    'prompt handoff should preserve transfer kind so desktop can bypass the download prompt for old clients',
  );
}

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

const enqueueWithHandoffAuth = createEnqueueDownloadRequest(
  'https://canvas.example.edu/files/569/download?download_frd=1&verifier=abc',
  { ...source, entryPoint: 'browser_download' },
  'request_handoff_auth',
  {
    suggestedFilename: 'lecture.pdf',
    handoffAuth: {
      headers: [
        { name: 'Cookie', value: 'canvas_session=abc' },
        { name: 'Referer', value: 'https://canvas.example.edu/courses/1/files' },
      ],
    },
  },
);
assert.equal(enqueueWithHandoffAuth.ok, true, 'browser download handoff should accept captured request headers');
if (enqueueWithHandoffAuth.ok) {
  assert.deepEqual(
    enqueueWithHandoffAuth.value.payload.handoffAuth,
    {
      headers: [
        { name: 'Cookie', value: 'canvas_session=abc' },
        { name: 'Referer', value: 'https://canvas.example.edu/courses/1/files' },
      ],
    },
    'enqueue handoff should preserve bounded browser request headers for desktop validation',
  );
}

const backgroundSource = readFileSync(new URL('../src/background/index.ts', import.meta.url), 'utf8');
assert.doesNotMatch(
  backgroundSource,
  /browserDownloadTransferKind\(item\)[\s\S]*promptDownload/,
  'rebuilt automatic browser capture should not route .torrent browser downloads through the old ask-mode URL replay path',
);
assert.match(
  backgroundSource,
  /handOffCapturedBrowserDownload\(/,
  'automatic browser downloads, including .torrent files, should use the prompt-first SDM handoff path',
);
