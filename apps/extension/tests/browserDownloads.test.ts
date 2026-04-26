import assert from 'node:assert/strict';
import type { ExtensionIntegrationSettings } from '@myapp/protocol';
import {
  browserDownloadUrl,
  createBrowserDownloadBypassState,
  createAsyncFilenameInterceptionListener,
  discardBrowserDownloadBeforeFilenameRelease,
  discardBrowserDownload,
  restartBrowserDownload,
  selectFilenameInterceptionApi,
  shouldBypassBrowserDownload,
  shouldDiscardBrowserDownloadAfterHandoff,
  shouldHandleBrowserDownload,
} from '../src/background/browserDownloads.ts';

const calls: string[] = [];

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
      { url: 'https://example.com/file.zip' },
      { ...defaultSettings, ignoredFileExtensions: ['zip'] },
    ),
    false,
    'ignored extensions should not capture normal HTTP downloads',
  );
  assert.equal(
    shouldHandleBrowserDownload(
      { url: 'https://example.com/file.torrent', filename: 'file.torrent' },
      { ...defaultSettings, ignoredFileExtensions: ['torrent'] },
    ),
    true,
    '.torrent downloads should still be handed off as torrent jobs',
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
    'Firefox .xpi packages should stay with the browser because AMO downloads can require browser session context',
  );

  await discardBrowserDownload(
    {
      async cancel(downloadId: number) {
        calls.push(`cancel:${downloadId}`);
      },
      async search(query: { id: number }) {
        calls.push(`search:${query.id}`);
        return [
          {
            id: query.id,
            state: 'complete',
            exists: true,
          },
        ];
      },
      async removeFile(downloadId: number) {
        calls.push(`removeFile:${downloadId}`);
      },
      async erase(query: { id: number }) {
        calls.push(`erase:${query.id}`);
      },
    },
    42,
  );

  assert.deepEqual(calls, ['cancel:42', 'search:42', 'removeFile:42', 'erase:42']);
  assert.equal(
    shouldDiscardBrowserDownloadAfterHandoff({
      ok: true,
      requestId: 'request_1',
      type: 'accepted',
      payload: {
        appState: 'running',
        status: 'canceled',
      },
    }),
    false,
    'prompt-canceled handoffs should return to the browser download',
  );
  assert.equal(
    shouldDiscardBrowserDownloadAfterHandoff({
      ok: true,
      requestId: 'request_queued',
      type: 'accepted',
      payload: {
        appState: 'running',
        status: 'queued',
      },
    }),
    true,
    'queued handoffs should block the browser download',
  );
  assert.equal(
    shouldDiscardBrowserDownloadAfterHandoff({
      ok: true,
      requestId: 'request_duplicate',
      type: 'accepted',
      payload: {
        appState: 'running',
        status: 'duplicate_existing_job',
      },
    }),
    true,
    'duplicate handoffs should block the browser download',
  );
  assert.equal(
    shouldDiscardBrowserDownloadAfterHandoff({
      ok: false,
      requestId: 'request_2',
      type: 'app_unreachable',
      code: 'APP_UNREACHABLE',
      message: 'app did not respond',
    }),
    false,
    'failed extension handoffs should be passed back to the browser',
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

  const releaseOrder: string[] = [];
  await discardBrowserDownloadBeforeFilenameRelease(
    {
      async cancel(downloadId: number) {
        releaseOrder.push(`cancel:${downloadId}`);
      },
      async search(query: { id: number }) {
        releaseOrder.push(`search:${query.id}`);
        return [];
      },
      async erase(query: { id: number }) {
        releaseOrder.push(`erase:${query.id}`);
      },
    },
    99,
    () => {
      releaseOrder.push('suggest');
    },
  );
  assert.deepEqual(
    releaseOrder,
    ['cancel:99', 'suggest', 'search:99', 'erase:99'],
    'accepted handoffs should cancel before releasing filename determination to prevent Save As leakage',
  );

  const fallbackReleaseOrder: string[] = [];
  await discardBrowserDownloadBeforeFilenameRelease(
    {
      async cancel(downloadId: number) {
        fallbackReleaseOrder.push(`cancel:${downloadId}`);
        throw new Error('not in progress');
      },
      async search(query: { id: number }) {
        fallbackReleaseOrder.push(`search:${query.id}`);
        return [];
      },
      async erase(query: { id: number }) {
        fallbackReleaseOrder.push(`erase:${query.id}`);
      },
    },
    100,
    () => {
      fallbackReleaseOrder.push('suggest');
    },
  );
  assert.deepEqual(
    fallbackReleaseOrder,
    ['cancel:100', 'suggest', 'cancel:100', 'search:100', 'erase:100'],
    'accepted handoffs should retry cancel after release if Chrome rejects pre-release cancellation',
  );

  let restartedWith: unknown;
  const bypass = createBrowserDownloadBypassState();
  const restartedId = await restartBrowserDownload(
    {
      async download(options) {
        restartedWith = options;
        assert.equal(
          shouldBypassBrowserDownload({ id: 500, url: options.url }, bypass),
          true,
          'fallback restart should bypass one URL event that races before the id is known',
        );
        return 501;
      },
    },
    {
      id: 100,
      url: 'https://example.com/download?id=1',
      finalUrl: 'https://cdn.example.com/File%20Name.zip',
      filename: 'C:\\Users\\Downloads\\File Name.zip',
    },
    bypass,
  );

  assert.equal(restartedId, 501);
  assert.deepEqual(restartedWith, {
    url: 'https://cdn.example.com/File%20Name.zip',
    filename: 'File Name.zip',
    conflictAction: 'uniquify',
    saveAs: false,
  });
  assert.equal(
    shouldBypassBrowserDownload({ id: 501, url: 'https://cdn.example.com/File%20Name.zip' }, bypass),
    true,
    'fallback restart should bypass interception by returned download id',
  );
  assert.equal(
    shouldBypassBrowserDownload({ id: 502, url: 'https://cdn.example.com/File%20Name.zip' }, bypass),
    false,
    'fallback bypass should be one-shot',
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
