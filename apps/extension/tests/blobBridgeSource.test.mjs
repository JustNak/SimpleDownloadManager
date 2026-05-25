import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const backgroundSource = readFileSync('apps/extension/src/background/index.ts', 'utf8');
const buildSource = readFileSync('apps/extension/scripts/build.mjs', 'utf8');
const interceptorSource = readFileSync('apps/extension/src/content/blobDownloadInterceptor.ts', 'utf8');
const pageHookSource = readFileSync('apps/extension/src/content/blobDownloadPageHook.ts', 'utf8');

assert.match(
  buildSource,
  /content_scripts[\s\S]*blobDownloadInterceptor\.js/,
  'extension manifests should load the blob download content script on normal pages',
);

assert.match(
  backgroundSource,
  /browser\.runtime\.onConnect\.addListener[\s\S]*browser_blob_download/,
  'background should accept blob stream ports from the content script',
);

assert.match(
  backgroundSource,
  /browser\.runtime\.connectNative\(HOST_NAME\)/,
  'background should relay blob streams over a long-lived native messaging port',
);

assert.match(
  pageHookSource,
  /if \(!anchor\.hasAttribute\('download'\)\) \{[\s\S]*?return null;[\s\S]*?if \(isStreamHref\(href\)\)/,
  'page hook should only treat blob/data URLs as page-managed downloads when the page explicitly uses a download anchor',
);

assert.match(
  pageHookSource,
  /getAttribute\('download'\)/,
  'page hook should intercept explicit download anchors before the browser starts them',
);

assert.match(
  interceptorSource,
  /page_download_intent/,
  'content script should forward explicit HTTP page download intents to the background',
);

assert.match(
  backgroundSource,
  /case 'page_download_intent'/,
  'background should handle page-managed HTTP download intents through native handoff',
);
