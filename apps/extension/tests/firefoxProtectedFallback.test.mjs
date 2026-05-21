import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backgroundSource = await readFile(path.join(repoRoot, 'apps/extension/src/background/index.ts'), 'utf8');

const handlerStart = backgroundSource.indexOf('async function handleFirefoxWebRequestHeadersReceived');
const handlerEnd = backgroundSource.indexOf('function markFirefoxWebRequestBypass', handlerStart);

assert.notEqual(handlerStart, -1, 'Firefox webRequest headers handler should exist');
assert.notEqual(handlerEnd, -1, 'Firefox webRequest handler should be sliceable');

const handlerSource = backgroundSource.slice(handlerStart, handlerEnd);
const markerCheckIndex = handlerSource.indexOf('hasCapturedBrowserSessionHeaders');
const authCheckIndex = handlerSource.indexOf('hasCapturedHandoffAuth');
const preserveIndex = handlerSource.indexOf('return {};', markerCheckIndex);
const cancelIndex = handlerSource.indexOf('return { cancel: true };');

assert.notEqual(
  markerCheckIndex,
  -1,
  'Firefox protected downloads should check memory-only browser session markers before canceling',
);
assert.notEqual(
  authCheckIndex,
  -1,
  'Firefox protected downloads should require exact captured auth before canceling',
);
assert.ok(
  preserveIndex > markerCheckIndex && preserveIndex < cancelIndex,
  'Firefox should preserve the original browser request when protected auth cannot be replayed',
);
assert.match(
  handlerSource,
  /const browserFallback:[\s\S]*\?[\s\r\n]*'unavailable'[\s\S]*:[\s\r\n]*'replay'/,
  'Firefox protected handoffs should mark browser replay fallback as unavailable',
);
