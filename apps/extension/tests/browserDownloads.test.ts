import assert from 'node:assert/strict';
import type { ExtensionIntegrationSettings } from '@myapp/protocol';
import {
  browserDownloadUrl,
  cancelBrowserDownloadForDesktopPrompt,
  createBrowserDownloadBypassState,
  createAsyncFilenameInterceptionListener,
  detachBrowserDownloadForDesktopPrompt,
  discardBrowserDownloadBeforeFilenameRelease,
  discardBrowserDownload,
  restartBrowserDownload,
  restoreBrowserDownloadAfterPromptFallback,
  selectFilenameInterceptionApi,
  shouldBypassBrowserDownload,
  shouldDiscardBrowserDownloadAfterHandoff,
  shouldHandleBrowserDownload,
  shouldRestoreBrowserDownloadAfterPromptSwap,
  shouldRestoreBrowserDownloadAfterFailedProtectedHandoff,
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
    authenticatedHandoffEnabled: false,
    authenticatedHandoffHosts: [],
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
      { url: 'https://download-cdn.example.com/file.zip' },
      { ...defaultSettings, excludedHosts: ['download*.example.com'] },
    ),
    false,
    'wildcards should match within host labels',
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
      requestId: 'request_dismissed',
      type: 'accepted',
      payload: {
        appState: 'running',
        status: 'dismissed',
      },
    }),
    true,
    'prompt-dismissed handoffs should cancel outright and discard the browser download',
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
  assert.equal(
    shouldRestoreBrowserDownloadAfterFailedProtectedHandoff({
      ok: false,
      requestId: 'request_protected',
      type: 'rejected',
      code: 'PROTECTED_DOWNLOAD_AUTH_REQUIRED',
      message: 'This site requires your browser session.',
    }),
    true,
    'protected-download auth failures should leave the browser fallback path active',
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

  const promptCaptureOrder: string[] = [];
  await cancelBrowserDownloadForDesktopPrompt(
    {
      async cancel(downloadId: number) {
        promptCaptureOrder.push(`cancel:${downloadId}`);
      },
    },
    101,
  );
  assert.deepEqual(
    promptCaptureOrder,
    ['cancel:101'],
    'filename-interception handoffs should cancel the browser item before waiting on the desktop prompt',
  );

  const detachBeforePromptOrder: string[] = [];
  await detachBrowserDownloadForDesktopPrompt(
    {
      async cancel(downloadId: number) {
        detachBeforePromptOrder.push(`cancel:${downloadId}`);
      },
      async search(query: { id: number }) {
        detachBeforePromptOrder.push(`search:${query.id}`);
        return [];
      },
      async erase(query: { id: number }) {
        detachBeforePromptOrder.push(`erase:${query.id}`);
      },
    },
    102,
    () => {
      detachBeforePromptOrder.push('suggest');
    },
  );
  detachBeforePromptOrder.push('desktop-prompt-wait');
  assert.deepEqual(
    detachBeforePromptOrder,
    ['cancel:102', 'suggest', 'search:102', 'erase:102', 'desktop-prompt-wait'],
    'prompt capture should detach the browser download before waiting for the desktop prompt',
  );

  const detachRetryOrder: string[] = [];
  await detachBrowserDownloadForDesktopPrompt(
    {
      async cancel(downloadId: number) {
        detachRetryOrder.push(`cancel:${downloadId}`);
        throw new Error('not in progress');
      },
      async search(query: { id: number }) {
        detachRetryOrder.push(`search:${query.id}`);
        return [];
      },
      async erase(query: { id: number }) {
        detachRetryOrder.push(`erase:${query.id}`);
      },
    },
    103,
    () => {
      detachRetryOrder.push('suggest');
    },
  );
  assert.deepEqual(
    detachRetryOrder,
    ['cancel:103', 'suggest', 'cancel:103', 'search:103', 'erase:103'],
    'prompt capture should retry cancel after filename release when Chrome rejects pre-release cancellation',
  );

  assert.equal(
    shouldRestoreBrowserDownloadAfterPromptSwap({
      ok: true,
      requestId: 'request_prompt_canceled',
      type: 'accepted',
      payload: {
        appState: 'running',
        status: 'canceled',
      },
    }),
    true,
    'Swap should restore the browser download',
  );
  assert.equal(
    shouldRestoreBrowserDownloadAfterPromptSwap({
      ok: true,
      requestId: 'request_prompt_dismissed_by_cancel_button',
      type: 'accepted',
      payload: {
        appState: 'running',
        status: 'dismissed',
      },
    }),
    false,
    'Cancel should fully cancel the browser download instead of restoring it',
  );
  for (const status of ['queued', 'duplicate_existing_job', 'dismissed'] as const) {
    assert.equal(
      shouldRestoreBrowserDownloadAfterPromptSwap({
        ok: true,
        requestId: `request_prompt_${status}`,
        type: 'accepted',
        payload: {
          appState: 'running',
          status,
        },
      }),
      false,
      `${status} prompt results should not restore the browser download after detachment`,
    );
  }

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

  const promptFallbackBypass = createBrowserDownloadBypassState();
  const promptFallbackOrder: string[] = [];
  let promptFallbackRestartedWith: unknown;
  const promptFallbackId = await restoreBrowserDownloadAfterPromptFallback(
    {
      async cancel(downloadId: number) {
        promptFallbackOrder.push(`cancel:${downloadId}`);
      },
      async search(query: { id: number }) {
        promptFallbackOrder.push(`search:${query.id}`);
        return [];
      },
      async erase(query: { id: number }) {
        promptFallbackOrder.push(`erase:${query.id}`);
      },
      async download(options) {
        promptFallbackRestartedWith = options;
        promptFallbackOrder.push(`download:${options.url}`);
        return 777;
      },
    },
    {
      id: 111,
      url: 'https://example.com/prompt',
      finalUrl: 'https://cdn.example.com/prompt.zip',
      filename: 'C:\\Users\\Downloads\\prompt.zip',
    },
    promptFallbackBypass,
    () => {
      promptFallbackOrder.push('suggest');
    },
  );

  assert.equal(promptFallbackId, 777);
  assert.deepEqual(
    promptFallbackOrder,
    ['suggest', 'cancel:111', 'search:111', 'erase:111', 'download:https://cdn.example.com/prompt.zip'],
    'prompt fallback should release the filename callback, discard the captured item, and restart through the bypass path',
  );
  assert.deepEqual(promptFallbackRestartedWith, {
    url: 'https://cdn.example.com/prompt.zip',
    filename: 'prompt.zip',
    conflictAction: 'uniquify',
    saveAs: false,
  });

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
