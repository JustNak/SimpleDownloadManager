import assert from 'node:assert/strict';
import {
  discardBrowserDownload,
  shouldDiscardBrowserDownloadAfterHandoff,
} from '../src/background/browserDownloads';

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
    true,
    'prompt-canceled handoffs should still block the browser download',
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
}

void main();
