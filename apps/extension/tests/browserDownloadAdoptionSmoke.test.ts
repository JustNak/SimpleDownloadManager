import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { completeBrowserDownloadAdoption } from '../src/background/browserDownloads.ts';

type QueueRow = {
  id: string;
  filename: string;
  state: 'completed';
  transferKind: 'browser_adopted';
};

const backgroundSource = readFileSync(
  join(process.cwd(), 'apps/extension/src/background/index.ts'),
  'utf8',
);

assert.match(
  backgroundSource,
  /downloads\?\.\onChanged\.addListener[\s\S]*handleBrowserDownloadChanged/,
  'browser download completion changes should be wired into the adoption handler',
);
assert.match(
  backgroundSource,
  /handleBrowserDownloadChanged[\s\S]*completeBrowserDownloadAdoption/,
  'the onChanged handler should run the completed browser adoption flow',
);

const adoptions = new Map([
  [42, {
    url: 'https://files.example.test/report.pdf',
    suggestedFilename: 'report.pdf',
    totalBytes: 12,
    incognito: false,
    pageUrl: 'https://files.example.test/',
    pageTitle: 'Files',
    reason: 'browser_download' as const,
  }],
]);
const completedRows: QueueRow[] = [];
let adoptedPayload: unknown;
let successResponse: unknown;

const handled = await completeBrowserDownloadAdoption(
  { id: 42, state: { current: 'complete' } },
  adoptions,
  {
    async searchDownloads(query) {
      assert.deepEqual(query, { id: 42 });
      return [{
        state: 'complete',
        exists: true,
        filename: 'C:\\Users\\Alice\\Downloads\\report.pdf',
        totalBytes: 12,
        mime: 'application/pdf',
      }];
    },
    browserDownloadSource(item) {
      return {
        entryPoint: 'browser_download',
        browser: 'chrome',
        extensionVersion: 'smoke-test',
        pageUrl: item.pageUrl,
        pageTitle: item.pageTitle,
        incognito: item.incognito,
      };
    },
    async adoptBrowserDownload(payload) {
      adoptedPayload = payload;
      completedRows.push({
        id: 'job_1',
        filename: payload.suggestedFilename ?? 'download.bin',
        state: 'completed',
        transferKind: 'browser_adopted',
      });
      return { ok: true, type: 'queued', payload: { jobId: 'job_1', status: 'queued' } };
    },
    isErrorResponse(response) {
      return !(response as { ok?: boolean }).ok;
    },
    onError() {
      assert.fail('completed browser adoption should not report a host error');
    },
    onSuccess(response) {
      successResponse = response;
    },
  },
);

assert.equal(handled, true);
assert.equal(adoptions.has(42), false, 'completed adoption should be consumed exactly once');
assert.deepEqual(adoptedPayload, {
  url: 'https://files.example.test/report.pdf',
  source: {
    entryPoint: 'browser_download',
    browser: 'chrome',
    extensionVersion: 'smoke-test',
    pageUrl: 'https://files.example.test/',
    pageTitle: 'Files',
    incognito: false,
  },
  localPath: 'C:\\Users\\Alice\\Downloads\\report.pdf',
  suggestedFilename: 'report.pdf',
  totalBytes: 12,
  mimeType: 'application/pdf',
});
assert.deepEqual(successResponse, { ok: true, type: 'queued', payload: { jobId: 'job_1', status: 'queued' } });
assert.deepEqual(completedRows, [{
  id: 'job_1',
  filename: 'report.pdf',
  state: 'completed',
  transferKind: 'browser_adopted',
}]);
