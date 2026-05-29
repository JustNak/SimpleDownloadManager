import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backgroundSource = await readFile(path.join(repoRoot, 'apps/extension/src/background/index.ts'), 'utf8');

const handlerStart = backgroundSource.indexOf('async function handleFirefoxWebRequestHeadersReceived');
const handlerEnd = backgroundSource.indexOf('async function handleBrowserDownloadChanged', handlerStart);

assert.notEqual(handlerStart, -1, 'Firefox webRequest headers handler should exist');
assert.notEqual(handlerEnd, -1, 'Firefox webRequest handler should be sliceable');
assert.equal(
  backgroundSource.indexOf('function registerHandoffAuthHeaderCapture'),
  -1,
  'rebuilt automatic capture should not collect browser auth headers for URL replay',
);

const handlerSource = backgroundSource.slice(handlerStart, handlerEnd);
const passThroughIndex = handlerSource.indexOf('shouldKeepBrowserSessionDownloadInBrowser(candidate, settings)');
const cancelIndex = handlerSource.indexOf('return { cancel: true };');
const adoptIndex = handlerSource.indexOf('markBrowserDownloadUrlForAdoption(candidate)');

assert.equal(cancelIndex, -1, 'Firefox webRequest interception should not cancel classified browser downloads');
assert.equal(
  passThroughIndex,
  -1,
  'Firefox should not bypass classified downloads merely because the browser sent cookies',
);
assert.notEqual(
  adoptIndex,
  -1,
  'Firefox classified downloads should be marked for completed-file adoption',
);
assert.match(
  handlerSource,
  /markBrowserDownloadUrlForAdoption\(candidate\);[\s\S]*return \{\};/,
  'Firefox classified downloads should let the original browser request finish before the app adopts the file',
);
assert.doesNotMatch(
  backgroundSource,
  /captureHandoffAuthHeaders|shouldKeepBrowserSessionDownloadInBrowser|markFirefoxWebRequestBypass/,
  'Firefox automatic capture should avoid the old protected-download replay fallback stack',
);
