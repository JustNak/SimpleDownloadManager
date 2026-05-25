import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backgroundSource = await readFile(path.join(repoRoot, 'apps/extension/src/background/index.ts'), 'utf8');

const handlerStart = backgroundSource.indexOf('async function handleFirefoxWebRequestHeadersReceived');
const handlerEnd = backgroundSource.indexOf('function markFirefoxWebRequestBypass', handlerStart);
const captureStart = backgroundSource.indexOf('function registerHandoffAuthHeaderCapture');
const captureEnd = backgroundSource.indexOf('async function handleFirefoxWebRequestHeadersReceived', captureStart);

assert.notEqual(handlerStart, -1, 'Firefox webRequest headers handler should exist');
assert.notEqual(handlerEnd, -1, 'Firefox webRequest handler should be sliceable');
assert.notEqual(captureStart, -1, 'protected-download header capture registration should exist');
assert.notEqual(captureEnd, -1, 'protected-download header capture registration should be sliceable');

const handlerSource = backgroundSource.slice(handlerStart, handlerEnd);
const captureSource = backgroundSource.slice(captureStart, captureEnd);
const markerCheckIndex = handlerSource.indexOf('hasCapturedBrowserSessionHeaders');
const cancelIndex = handlerSource.indexOf('return { cancel: true };');

assert.notEqual(
  markerCheckIndex,
  -1,
  'Firefox protected downloads should check memory-only browser session markers before canceling',
);
assert.doesNotMatch(
  handlerSource.slice(markerCheckIndex, cancelIndex),
  /return \{\};/,
  'Firefox protected downloads should not leak the original browser request after they are classified as downloads',
);
assert.match(
  handlerSource,
  /const browserFallback:[\s\S]*\?[\s\r\n]*'unavailable'[\s\S]*:[\s\r\n]*candidate\.browserFallback \?\? 'replay'/,
  'Firefox protected handoffs should mark browser replay fallback as unavailable',
);
assert.match(
  handlerSource,
  /void handleFirefoxWebRequestDownload\(\{ \.\.\.candidate, browserFallback \}, settings\);[\s\S]*return \{ cancel: true \};/,
  'Firefox protected handoffs should cancel the browser request and let the strict handoff policy handle failures',
);
assert.match(
  captureSource,
  /types: \['main_frame', 'sub_frame', 'xmlhttprequest', 'object', 'other'\]/,
  'protected-download header capture should include object requests used by embedded Canvas/canvadocs PDFs',
);
assert.match(
  captureSource,
  /captureHandoffAuthHeaders\(details, cachedExtensionSettings \?\? defaultExtensionSettings\);[\s\S]*getCachedExtensionSettings\(\)\.then\(\(settings\) => \{[\s\S]*captureHandoffAuthHeaders\(details, settings\);/,
  'startup protected-download header capture should re-run with loaded user settings before Firefox cancels classified downloads',
);
