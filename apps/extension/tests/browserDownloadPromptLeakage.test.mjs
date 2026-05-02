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
const pingIndex = handlerSource.indexOf('const pingResponse = await pingNativeHost');
const handoffIndex = handlerSource.indexOf('const response = await handOffBrowserDownload');

assert.notEqual(
  detachIndex,
  -1,
  'filename interception should detach the original browser download before the desktop prompt can wait',
);
assert.notEqual(handoffIndex, -1, 'filename interception should still hand off to the desktop app');
assert.notEqual(pingIndex, -1, 'filename interception should still check the native host');
assert.ok(
  detachIndex < pingIndex,
  'browser download detachment must happen before native host checks can wait or fail',
);
assert.ok(
  detachIndex < handoffIndex,
  'browser download detachment must happen before awaiting the desktop handoff response',
);
assert.match(
  handlerSource,
  /classifyBrowserDownloadHandoffResolution\(response\)/,
  'detached prompt flow should classify desktop handoff outcomes before deciding whether to restore',
);
assert.doesNotMatch(
  handlerSource,
  /discardBrowserDownloadBeforeFilenameRelease/,
  'detached prompt flow should not keep the filename callback open until after the desktop prompt resolves',
);

assert.match(
  handlerSource,
  /if \(isErrorResponse\(pingResponse\)\) \{[\s\S]*?await recordHostError\(pingResponse\);[\s\S]*?await restoreBrowserDownloadFallback\(item\);[\s\S]*?return;/,
  'native-host ping failures after detachment should restore the browser download',
);
assert.match(
  handlerSource,
  /if \(!shouldHandleBrowserDownload\(item, settings\)\) \{[\s\S]*?await updateBrowserBadge\(pingState\);[\s\S]*?await restoreBrowserDownloadFallback\(item\);[\s\S]*?return;/,
  'settings synced from the desktop app after detachment should restore when capture becomes disabled',
);
assert.match(
  handlerSource,
  /if \(handoffResolution\.action === 'record_error_and_restore'\) \{[\s\S]*?await recordHostError\(handoffResolution\.response\);[\s\S]*?await restoreBrowserDownloadFallback\(item\);[\s\S]*?return;/,
  'native handoff errors after detachment should record the error and restore the browser download',
);
