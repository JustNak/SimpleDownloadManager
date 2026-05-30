import assert from 'node:assert/strict';
import { DEFAULT_CAPTURED_FILE_EXTENSIONS, type ExtensionIntegrationSettings } from '@myapp/protocol';
import {
  browserDownloadUrl,
  classifyBrowserDownloadIntent,
  createAsyncFilenameInterceptionListener,
  selectFilenameInterceptionApi,
  shouldHandleBrowserDownload,
} from '../src/background/browserDownloads.ts';

async function main() {
  const defaultSettings: ExtensionIntegrationSettings = {
    enabled: true,
    downloadHandoffMode: 'ask',
    listenPort: 1420,
    contextMenuEnabled: true,
    showProgressAfterHandoff: true,
    showBadgeStatus: true,
    excludedHosts: [],
    ignoredFileExtensions: [],
    capturedFileExtensions: [...DEFAULT_CAPTURED_FILE_EXTENSIONS],
    downloadCaptureDebugLogging: false,
  };

  assert.equal(
    browserDownloadUrl({
      url: 'https://example.com/redirect',
      finalUrl: 'https://cdn.example.com/file.zip',
    }),
    'https://cdn.example.com/file.zip',
    'finalUrl should be preferred when the browser exposes it',
  );

  assert.equal(
    shouldHandleBrowserDownload({ url: 'https://example.com/file.zip' }, { ...defaultSettings, enabled: false }),
    false,
    'disabled integration should not capture browser downloads',
  );
  assert.equal(
    shouldHandleBrowserDownload(
      { url: 'https://example.com/file.zip' },
      { ...defaultSettings, downloadHandoffMode: 'off' },
    ),
    false,
    'off handoff mode should not capture browser downloads',
  );
  assert.equal(
    shouldHandleBrowserDownload(
      { url: 'https://downloads.example.com/file.zip' },
      { ...defaultSettings, excludedHosts: ['example.com'] },
    ),
    false,
    'excluded hosts should include subdomains',
  );
  assert.equal(
    shouldHandleBrowserDownload(
      { url: 'https://downloads.example.com/file.zip' },
      { ...defaultSettings, excludedHosts: ['*.example.com'] },
    ),
    false,
    'wildcard host excludes should match subdomains',
  );
  assert.equal(
    shouldHandleBrowserDownload(
      { url: 'https://example.com/file.zip' },
      { ...defaultSettings, excludedHosts: ['*.example.com'] },
    ),
    true,
    'subdomain wildcard excludes should not match the root host',
  );
  assert.equal(
    shouldHandleBrowserDownload(
      {
        url: 'https://downloads.example.com/packages/archive.pkgx?token=abc',
        filename: 'C:\\Users\\Me\\Downloads\\archive.pkgx',
      },
      { ...defaultSettings, capturedFileExtensions: ['pkgx'] },
    ),
    true,
    'user captured extensions should allow custom file types to be adopted',
  );
  assert.equal(
    shouldHandleBrowserDownload(
      { url: 'https://example.com/file.zip', filename: 'file.zip' },
      { ...defaultSettings, capturedFileExtensions: defaultSettings.capturedFileExtensions.filter((extension) => extension !== 'zip') },
    ),
    false,
    'removing a built-in extension from captured extensions should stop strong filename capture',
  );
  assert.equal(
    shouldHandleBrowserDownload(
      {
        url: 'https://addons.mozilla.org/firefox/downloads/file/4780131/e2f3c242819942eeb738-0.3.2.xpi',
        filename: 'e2f3c242819942eeb738-0.3.2.xpi',
      },
      defaultSettings,
    ),
    false,
    'Firefox .xpi packages should stay with the browser',
  );

  for (const item of [
    {
      url: 'https://www.youtube.com/youtubei/v1/verify?prettyPrint=false',
      filename: 'verify',
      mime: 'application/octet-stream',
      totalBytes: 0,
    },
    {
      url: 'https://www.youtube.com/verify_session',
      filename: 'json.txt',
      mime: 'application/octet-stream',
      totalBytes: 0,
    },
    {
      url: 'https://example.com/api/v1/session',
      filename: 'session',
      mime: 'application/json',
      totalBytes: 64,
    },
  ]) {
    assert.equal(
      shouldHandleBrowserDownload(item, defaultSettings),
      false,
      `${item.url} should be ignored as app traffic`,
    );
  }

  assert.deepEqual(
    classifyBrowserDownloadIntent({
      url: 'https://music.youtube.com/verify_session',
      filename: 'C:\\Users\\Me\\Downloads\\json.txt',
      mime: 'application/octet-stream',
      totalBytes: 0,
      fileSize: 0,
    }),
    { action: 'ignore', reason: 'app_traffic_probe' },
    'classifier should explain high-confidence app traffic probes',
  );
  assert.deepEqual(
    classifyBrowserDownloadIntent({
      url: 'https://files.example.com/export',
      contentDisposition: 'attachment; filename="export.zip"',
      mime: 'application/octet-stream',
    }),
    { action: 'capture', reason: 'attachment_disposition' },
    'classifier should explain explicit attachment captures',
  );
  assert.deepEqual(
    classifyBrowserDownloadIntent({
      url: 'https://files.example.com/download?id=opaque',
      filename: 'download',
      mime: 'application/zip',
      totalBytes: 8 * 1024 * 1024,
      resourceType: 'main_frame',
    }, defaultSettings.capturedFileExtensions),
    { action: 'capture', reason: 'download_mime' },
    'large top-level responses with a known download MIME type should be captured even when the URL is opaque',
  );
  assert.deepEqual(
    classifyBrowserDownloadIntent({
      url: 'https://www.youtube.com/api/timedtext?v=RZpz24nP1P0&ei=xUE',
      filename: 't.txt',
      contentDisposition: 'attachment; filename="t.txt"',
      mime: 'text/plain',
      totalBytes: 7680,
    }, defaultSettings.capturedFileExtensions),
    { action: 'ignore', reason: 'app_traffic_payload' },
    'attachment responses without a captured extension should not bypass the captured extension list',
  );
  assert.deepEqual(
    classifyBrowserDownloadIntent({
      url: 'https://api.example.test/v1/player/archive.zip',
      filename: 'archive.zip',
      contentDisposition: 'attachment; filename="archive.zip"',
      mime: 'application/octet-stream',
      totalBytes: 1048576,
      resourceType: 'xmlhttprequest',
    }, ['zip']),
    { action: 'ignore', reason: 'app_traffic_payload' },
    'page-internal API endpoints should not be captured',
  );

  const suggestionCalls: Array<{ filename?: string; conflictAction?: 'uniquify' | 'overwrite' | 'prompt' } | undefined> = [];
  const handledIds: number[] = [];
  const listener = createAsyncFilenameInterceptionListener(
    async (item: { id: number }, suggest) => {
      handledIds.push(item.id);
      suggest({ filename: 'CymaticsHubSetup.exe', conflictAction: 'uniquify' });
      suggest({ filename: 'duplicate.exe', conflictAction: 'uniquify' });
    },
  );
  const returned = listener({ id: 7 }, (suggestion) => {
    suggestionCalls.push(suggestion);
  });

  assert.equal(returned, true, 'async filename listeners must keep the browser suggest callback open');
  await Promise.resolve();
  assert.deepEqual(handledIds, [7]);
  assert.deepEqual(
    suggestionCalls,
    [{ filename: 'CymaticsHubSetup.exe', conflictAction: 'uniquify' }],
    'filename listeners should only release the browser callback once',
  );

  const rawFilenameApi = {
    onDeterminingFilename: {
      addListener() {
        // marker only
      },
    },
  };
  assert.equal(
    selectFilenameInterceptionApi(undefined, rawFilenameApi),
    rawFilenameApi,
    'raw Chrome filename interception should be preferred when the polyfill does not expose the event',
  );
}

void main();
