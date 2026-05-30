import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const backgroundSource = readFileSync(new URL('../src/background/index.ts', import.meta.url), 'utf8');

assert.match(
  backgroundSource,
  /captureBrowserDownloadRedirect/,
  'browser redirects should be tracked so signed CDN responses can be handed off by their source URL',
);
assert.match(
  backgroundSource,
  /resolveOriginalBrowserDownloadUrl/,
  'handoff should resolve the original browser download URL before sending to SDM',
);
assert.match(
  backgroundSource,
  /const handoffUrl = resolveOriginalBrowserDownloadUrl\(url, item\) \?\? url;[\s\S]*enqueueDownload\(handoffUrl, source, metadata\)[\s\S]*promptDownload\(handoffUrl, source, metadata\)/,
  'SDM handoff should use the original Instructure URL instead of the resolved CDN URL when a redirect source is known',
);
assert.doesNotMatch(
  backgroundSource,
  /fallbackCapturedDownloadToBrowser|shouldKeepProtectedBrowserDownloadInBrowser|trackProtectedBrowserDownloadForAdoption/,
  'Canvas/Instructure support should not depend on browser-owned fallback/adoption',
);
