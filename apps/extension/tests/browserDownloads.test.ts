import assert from 'node:assert/strict';
import {
  createAsyncFilenameInterceptionListener,
  discardBrowserDownloadBeforeFilenameRelease,
  discardBrowserDownload,
  selectFilenameInterceptionApi,
  shouldDiscardBrowserDownloadAfterHandoff,
} from '../src/background/browserDownloads.ts';

const calls: string[] = [];

async function main() {
  await discardBrowserDownload(
    {
      async cancel(downloadId: number) {
        calls.push(`cancel:${downloadId}`);
      },
      async erase(query: { id: number }) {
        calls.push(`erase:${query.id}`);
      },
    },
    42,
  );

  assert.deepEqual(calls, ['cancel:42', 'erase:42']);
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
    },
  );
  const returned = listener({ id: 7 }, (suggestion) => {
    suggestionCalls.push(suggestion);
  });

  assert.equal(returned, true, 'async filename listeners must keep the browser suggest callback open');
  await Promise.resolve();
  assert.deepEqual(handledIds, [7]);
  assert.deepEqual(suggestionCalls, [{ filename: 'CymaticsHubSetup.exe', conflictAction: 'uniquify' }]);

  const releaseOrder: string[] = [];
  await discardBrowserDownloadBeforeFilenameRelease(
    {
      async cancel(downloadId: number) {
        releaseOrder.push(`cancel:${downloadId}`);
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
    ['cancel:99', 'suggest', 'erase:99'],
    'accepted handoffs should cancel before releasing filename determination to prevent Save As leakage',
  );

  const fallbackReleaseOrder: string[] = [];
  await discardBrowserDownloadBeforeFilenameRelease(
    {
      async cancel(downloadId: number) {
        fallbackReleaseOrder.push(`cancel:${downloadId}`);
        throw new Error('not in progress');
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
    ['cancel:100', 'suggest', 'cancel:100', 'erase:100'],
    'accepted handoffs should retry cancel after release if Chrome rejects pre-release cancellation',
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
