import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backgroundSource = await readFile(path.join(repoRoot, 'apps/extension/src/background/index.ts'), 'utf8');

const handlerStart = backgroundSource.indexOf('async function handleBrowserDownloadDeterminingFilename');
const handlerEnd = backgroundSource.indexOf('browser.runtime.onInstalled', handlerStart);
const handoffStart = backgroundSource.indexOf('async function handOffBrowserDownload');
const handoffEnd = backgroundSource.indexOf('async function recordHostError', handoffStart);

assert.notEqual(handlerStart, -1, 'filename interception handler should exist');
assert.notEqual(handlerEnd, -1, 'filename interception handler should be sliceable');
assert.notEqual(handoffStart, -1, 'browser download handoff helper should exist');
assert.notEqual(handoffEnd, -1, 'browser download handoff helper should be sliceable');

const handlerSource = backgroundSource.slice(handlerStart, handlerEnd);
const handoffSource = backgroundSource.slice(handoffStart, handoffEnd);
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
  /if \(isErrorResponse\(pingResponse\)\) \{[\s\S]*?await recordHostError\(pingResponse\);[\s\S]*?return;/,
  'native-host ping failures after detachment should record an error and keep the browser download blocked',
);
assert.doesNotMatch(
  handlerSource,
  /if \(isErrorResponse\(pingResponse\)\) \{(?:(?!return;)[\s\S])*restoreBrowserDownloadFallback\(item\)/,
  'native-host ping failures must not restore the browser download automatically',
);
assert.match(
  handlerSource,
  /if \(!shouldHandleBrowserDownload\(item, settings\)\) \{[\s\S]*?await updateBrowserBadge\(pingState\);[\s\S]*?return;/,
  'settings synced from the desktop app after detachment should not leak the captured browser download',
);
assert.doesNotMatch(
  handlerSource,
  /if \(!shouldHandleBrowserDownload\(item, settings\)\) \{(?:(?!return;)[\s\S])*restoreBrowserDownloadFallback\(item\)/,
  'post-capture settings changes must not restore the browser download automatically',
);
assert.match(
  handlerSource,
  /if \(handoffResolution\.action === 'record_error'\) \{[\s\S]*?await recordHostError\(handoffResolution\.response\);[\s\S]*?return;/,
  'native handoff errors after detachment should record the error and keep the browser download blocked',
);
assert.doesNotMatch(
  handlerSource,
  /handoffResolution\.action === 'record_error'(?:(?!return;)[\s\S])*restoreBrowserDownloadFallback\(item\)/,
  'native handoff errors must not restore the browser download automatically',
);
assert.match(
  handoffSource,
  /const authResolution = await resolveBrowserHandoffAuthWithCookieFallback\(handoffDetails, settings, \{[\s\S]*?cookieLookup: getCookieLookup\(\),[\s\S]*?userAgent: browserUserAgent\(\),[\s\S]*?\}\);/,
  'browser download handoffs should resolve captured auth and Firefox cookie fallback before queuing',
);
assert.doesNotMatch(
  handoffSource,
  /authResolution\.status === 'protected_auth_required'[\s\S]*?PROTECTED_DOWNLOAD_AUTH_REQUIRED/,
  'Chrome-style browser download handoffs should let the desktop access probe decide when auth cannot be attached',
);
assert.match(
  handoffSource,
  /createBrowserDownloadHandoffMetadata\(item, authResolution\.handoffAuth\)/,
  'Chrome-style browser download handoffs should attach captured auth only when available',
);
