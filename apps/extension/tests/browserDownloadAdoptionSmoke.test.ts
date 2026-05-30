import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const backgroundSource = readFileSync(
  join(process.cwd(), 'apps/extension/src/background/index.ts'),
  'utf8',
);

assert.doesNotMatch(
  backgroundSource,
  /trackBrowserDownloadForAdoption|markBrowserDownloadUrlForAdoption|fallbackCapturedDownloadToBrowser|completeBrowserDownloadAdoption/,
  'automatic browser capture should not use completed-file adoption or browser fallback',
);

assert.match(
  backgroundSource,
  /resolveOriginalBrowserDownloadUrl/,
  'redirected browser downloads should preserve the original source URL for SDM handoff',
);

assert.match(
  backgroundSource,
  /async function handleBrowserDownloadDeterminingFilename[\s\S]*await cancelBrowserDownload\(item\)[\s\S]*void handOffCapturedBrowserDownload\(url, item, settings\)[\s\S]*suggest\(\);/,
  'Chromium filename interception should cancel the browser item, start SDM handoff, then release the filename callback',
);

assert.match(
  backgroundSource,
  /async function handleFirefoxWebRequestHeadersReceived[\s\S]*void handOffCapturedBrowserDownload\(candidate\.url, candidate, settings\);[\s\S]*return \{ cancel: true \};/,
  'Firefox webRequest interception should cancel the browser request and hand the download to SDM',
);

assert.match(
  backgroundSource,
  /settings\.downloadHandoffMode === 'auto'[\s\S]*enqueueDownload\(handoffUrl, source, metadata\)[\s\S]*promptDownload\(handoffUrl, source, metadata\)/,
  'auto mode should enqueue directly while ask mode opens the existing desktop prompt',
);

assert.match(
  backgroundSource,
  /downloads\.removeFile\?\.\(item\.id\)[\s\S]*downloads\.erase\?\.\(\{ id: item\.id \}\)/,
  'browser cancellation should attempt best-effort cleanup of partial files and download history',
);
