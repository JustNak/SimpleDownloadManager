import assert from 'node:assert/strict';
import type { ExtensionIntegrationSettings } from '@myapp/protocol';
import {
  blobDownloadFilename,
  createBrowserBlobBeginRequest,
  createBrowserBlobChunkRequest,
  isBlobDownloadHref,
  shouldHandleBlobDownload,
} from '../src/background/blobDownloads.ts';

const defaultSettings: ExtensionIntegrationSettings = {
  enabled: true,
  downloadHandoffMode: 'ask',
  listenPort: 1420,
  contextMenuEnabled: true,
  showProgressAfterHandoff: true,
  showBadgeStatus: true,
  excludedHosts: [],
  ignoredFileExtensions: [],
  authenticatedHandoffEnabled: true,
  protectedDownloadAuthScope: 'allowlist',
  authenticatedHandoffHosts: ['gofile.io'],
};

assert.equal(isBlobDownloadHref('blob:https://web.telegram.org/8f2a'), true);
assert.equal(isBlobDownloadHref('https://web.telegram.org/file.zip'), false);

assert.equal(
  shouldHandleBlobDownload(
    { blobUrl: 'blob:https://web.telegram.org/8f2a', pageUrl: 'https://web.telegram.org/k/', filename: 'video.mp4' },
    defaultSettings,
  ),
  true,
  'Telegram blob downloads should be captured when the site is not explicitly excluded',
);

assert.equal(
  shouldHandleBlobDownload(
    { blobUrl: 'blob:https://web.telegram.org/8f2a', pageUrl: 'https://web.telegram.org/k/', filename: 'video.mp4' },
    { ...defaultSettings, excludedHosts: ['web.telegram.org'] },
  ),
  false,
  'custom excluded hosts should still bypass blob capture',
);

assert.equal(
  shouldHandleBlobDownload(
    { blobUrl: 'blob:https://web.telegram.org/8f2a', pageUrl: 'https://web.telegram.org/k/', filename: 'video.mp4' },
    { ...defaultSettings, ignoredFileExtensions: ['mp4'] },
  ),
  false,
  'ignored file extensions should bypass blob capture',
);

assert.equal(blobDownloadFilename('C:\\Users\\Me\\Downloads\\clip.webm', 'video/mp4'), 'clip.webm');
assert.equal(blobDownloadFilename('', 'video/mp4'), 'download.mp4');
assert.equal(blobDownloadFilename('', 'application/octet-stream'), 'download.bin');

assert.deepEqual(
  createBrowserBlobBeginRequest({
    streamId: 'blob-stream-1',
    filename: 'clip.webm',
    totalBytes: 4096,
    mimeType: 'video/webm',
    source: {
      entryPoint: 'browser_download',
      browser: 'chrome',
      extensionVersion: '1.0.1',
      pageUrl: 'https://web.telegram.org/k/',
      pageTitle: 'Telegram',
      incognito: false,
    },
  }).payload,
  {
    streamId: 'blob-stream-1',
    suggestedFilename: 'clip.webm',
    totalBytes: 4096,
    mimeType: 'video/webm',
    source: {
      entryPoint: 'browser_download',
      browser: 'chrome',
      extensionVersion: '1.0.1',
      pageUrl: 'https://web.telegram.org/k/',
      pageTitle: 'Telegram',
      referrer: undefined,
      incognito: false,
    },
  },
  'blob begin requests should carry filename, size, MIME, and browser source metadata',
);

assert.deepEqual(
  createBrowserBlobChunkRequest('blob-stream-1', 1024, new Uint8Array([0, 1, 2, 255])).payload,
  {
    streamId: 'blob-stream-1',
    offset: 1024,
    data: 'AAEC/w==',
  },
  'blob chunk requests should base64 encode binary chunks for native messaging',
);
