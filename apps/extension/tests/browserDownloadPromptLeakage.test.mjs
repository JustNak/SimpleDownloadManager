import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backgroundSource = await readFile(path.join(repoRoot, 'apps/extension/src/background/index.ts'), 'utf8');

const handlerStart = backgroundSource.indexOf('async function handleBrowserDownloadDeterminingFilename');
const handlerEnd = backgroundSource.indexOf('browser.runtime.onInstalled', handlerStart);

assert.notEqual(handlerStart, -1, 'filename interception handler should exist');
assert.notEqual(handlerEnd, -1, 'filename interception handler should be sliceable');

const handlerSource = backgroundSource.slice(handlerStart, handlerEnd);
const detachIndex = handlerSource.indexOf('await detachBrowserDownloadForDesktopPrompt');
const handoffIndex = handlerSource.indexOf('const response = await handOffBrowserDownload');

assert.notEqual(
  detachIndex,
  -1,
  'filename interception should detach the original browser download before the desktop prompt can wait',
);
assert.notEqual(handoffIndex, -1, 'filename interception should still hand off to the desktop app');
assert.ok(
  detachIndex < handoffIndex,
  'browser download detachment must happen before awaiting the desktop handoff response',
);
assert.match(
  handlerSource,
  /shouldRestoreBrowserDownloadAfterPromptSwap\(response\)/,
  'detached prompt flow should restore the browser download only for the Swap prompt result',
);
assert.doesNotMatch(
  handlerSource,
  /discardBrowserDownloadBeforeFilenameRelease/,
  'detached prompt flow should not keep the filename callback open until after the desktop prompt resolves',
);
