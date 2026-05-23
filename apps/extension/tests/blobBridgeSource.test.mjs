import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const backgroundSource = readFileSync('apps/extension/src/background/index.ts', 'utf8');
const buildSource = readFileSync('apps/extension/scripts/build.mjs', 'utf8');

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
