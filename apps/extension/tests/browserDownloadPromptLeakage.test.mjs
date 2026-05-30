import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const backgroundSource = await readFile(path.join(repoRoot, 'apps/extension/src/background/index.ts'), 'utf8');

const handlerStart = backgroundSource.indexOf('async function handleBrowserDownloadDeterminingFilename');
const handlerEnd = backgroundSource.indexOf('browser.runtime.onInstalled', handlerStart);
const preSaveHelperIndex = backgroundSource.indexOf('async function handOffBrowserDownloadBeforeBrowserSave');
const directBrowserHandoffIndex = backgroundSource.indexOf('async function handOffBrowserDownload');
const blobStreamIndex = backgroundSource.indexOf('async function streamBrowserDownloadToDesktop');

assert.notEqual(handlerStart, -1, 'filename interception handler should exist');
assert.notEqual(handlerEnd, -1, 'filename interception handler should be sliceable');
assert.equal(preSaveHelperIndex, -1, 'automatic browser capture should not keep the old pre-save desktop handoff helper');
assert.equal(directBrowserHandoffIndex, -1, 'automatic browser capture should not replay browser downloads directly through the desktop app');
assert.equal(blobStreamIndex, -1, 'automatic browser capture should not stream browser fetch blobs through native messaging');

const handlerSource = backgroundSource.slice(handlerStart, handlerEnd);
const detachIndex = handlerSource.indexOf('await detachBrowserDownloadForDesktopPrompt');
const pingIndex = handlerSource.indexOf('const pingResponse = await pingNativeHost');
const cancelIndex = handlerSource.indexOf('await cancelBrowserDownload(item)');
const handoffIndex = handlerSource.indexOf('void handOffCapturedBrowserDownload(');

assert.notEqual(cancelIndex, -1, 'filename interception should cancel the browser download before SDM handoff');
assert.notEqual(handoffIndex, -1, 'filename interception should hand the cancelled download to SDM prompt/auto flow');
assert.equal(
  detachIndex,
  -1,
  'filename interception should not use the old detach helper that releases the browser save dialog before app acceptance',
);
assert.equal(pingIndex, -1, 'filename interception should not ping before releasing the browser save');
assert.doesNotMatch(
  handlerSource,
  /discardBrowserDownloadBeforeFilenameRelease/,
  'detached prompt flow should not keep the filename callback open until after the desktop prompt resolves',
);
assert.doesNotMatch(
  handlerSource,
  /trackBrowserDownloadForAdoption\(url, item, 'browser_download'\)/,
  'ask/auto browser capture should not let the browser save first and adopt the completed file later',
);
assert.doesNotMatch(
  backgroundSource,
  /resolveBrowserHandoffAuthWithCookieFallback|createBrowserDownloadHandoffMetadata|classifyBrowserDownloadHandoffResolution/,
  'automatic browser capture should not depend on protected-header URL replay helpers',
);
