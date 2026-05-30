import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backgroundSource = await readFile(path.join(repoRoot, 'apps/extension/src/background/index.ts'), 'utf8');

const handlerStart = backgroundSource.indexOf('async function handleFirefoxWebRequestHeadersReceived');
const handlerEnd = backgroundSource.indexOf('function normalizedDownloadSize', handlerStart);

assert.notEqual(handlerStart, -1, 'Firefox webRequest headers handler should exist');
assert.notEqual(handlerEnd, -1, 'Firefox webRequest handler should be sliceable');
assert.notEqual(
  backgroundSource.indexOf('function registerBrowserHandoffAuthHeaderCapture'),
  -1,
  'automatic capture should collect bounded browser request headers for memory-only SDM handoff',
);

const handlerSource = backgroundSource.slice(handlerStart, handlerEnd);
const passThroughIndex = handlerSource.indexOf('shouldKeepBrowserSessionDownloadInBrowser(candidate, settings)');
const cancelIndex = handlerSource.indexOf('return { cancel: true };');
const handoffIndex = handlerSource.indexOf('void handOffCapturedBrowserDownload(');

assert.notEqual(cancelIndex, -1, 'Firefox webRequest interception should cancel classified browser downloads before SDM handoff');
assert.equal(
  passThroughIndex,
  -1,
  'Firefox should not bypass classified downloads merely because the browser sent cookies',
);
assert.notEqual(
  handoffIndex,
  -1,
  'Firefox classified downloads should be handed to SDM prompt/auto flow',
);
assert.match(
  backgroundSource,
  /captureHandoffAuthHeaders[\s\S]*handoffAuthFromRequestHeaders[\s\S]*resolveBrowserHandoffAuth/,
  'Firefox automatic capture should pair the classified download with the browser-sent request headers',
);
assert.doesNotMatch(
  backgroundSource,
  /shouldKeepBrowserSessionDownloadInBrowser|markFirefoxWebRequestBypass|markBrowserDownloadUrlForAdoption/,
  'Firefox automatic capture should avoid browser-session adoption and the old protected-download fallback stack',
);
