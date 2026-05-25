import assert from 'node:assert/strict';
import {
  createEnqueueDownloadRequest,
  type ExtensionIntegrationSettings,
  type RequestSource,
} from '@myapp/protocol';
import {
  buildHandoffAuthForUrl,
  captureHandoffAuthHeaders,
  filterHandoffAuthHeaders,
  hasCapturedBrowserSessionHeaders,
  hasCapturedHandoffAuth,
  resolveBrowserHandoffAuth,
  resolveBrowserHandoffAuthWithCookieFallback,
  takeCapturedHandoffAuth,
} from '../src/background/handoffAuth.ts';

const source: RequestSource = {
  entryPoint: 'browser_download',
  browser: 'chrome',
  extensionVersion: '0.3.45',
};

const settings: ExtensionIntegrationSettings = {
  enabled: true,
  downloadHandoffMode: 'ask',
  listenPort: 1420,
  contextMenuEnabled: true,
  showProgressAfterHandoff: true,
  showBadgeStatus: true,
  excludedHosts: [],
  ignoredFileExtensions: [],
  authenticatedHandoffEnabled: true,
  protectedDownloadAuthScope: 'legacy_global',
  authenticatedHandoffHosts: [],
};

const allowlistSettings: ExtensionIntegrationSettings = {
  ...settings,
  protectedDownloadAuthScope: 'allowlist',
  authenticatedHandoffHosts: ['chatgpt.com', '*.example.com'],
};

const defaultAllowlistSettings: ExtensionIntegrationSettings = {
  ...settings,
  protectedDownloadAuthScope: 'allowlist',
  authenticatedHandoffHosts: ['gofile.io'],
};

const disabledSettings: ExtensionIntegrationSettings = {
  ...settings,
  authenticatedHandoffEnabled: false,
  protectedDownloadAuthScope: 'off',
};

assert.deepEqual(
  filterHandoffAuthHeaders([
    { name: 'Cookie', value: 'session=abc' },
    { name: 'Authorization', value: 'Bearer secret' },
    { name: 'Range', value: 'bytes=0-' },
    { name: 'Accept-Encoding', value: 'gzip' },
    { name: 'Sec-Fetch-Site', value: 'same-origin' },
    { name: 'X-Unsafe', value: 'nope' },
  ]),
  [
    { name: 'Cookie', value: 'session=abc' },
    { name: 'Authorization', value: 'Bearer secret' },
    { name: 'Sec-Fetch-Site', value: 'same-origin' },
  ],
  'auth handoff should forward only the bounded browser-session header allowlist',
);

assert.deepEqual(
  buildHandoffAuthForUrl(
    'https://chatgpt.com/backend-api/estuary/content?id=file_123',
    [{ name: 'Cookie', value: 'oai-did=1' }],
    allowlistSettings,
  ),
  { headers: [{ name: 'Cookie', value: 'oai-did=1' }] },
  'legacy allowlist settings should forward browser session headers for configured hosts',
);

assert.deepEqual(
  buildHandoffAuthForUrl(
    'https://download-cdn.other.test/file.pdf',
    [{ name: 'Cookie', value: 'session=abc' }],
    allowlistSettings,
  ),
  { headers: [{ name: 'Cookie', value: 'session=abc' }] },
  'legacy allowlist settings should now forward browser session headers for any protected browser download host',
);

assert.deepEqual(
  buildHandoffAuthForUrl(
    'https://file-ap-sgp-3.gofile.io/download/web/file.rar',
    [{ name: 'Cookie', value: 'accountToken=secret' }],
    defaultAllowlistSettings,
  ),
  { headers: [{ name: 'Cookie', value: 'accountToken=secret' }] },
  'protected downloads should forward bounded session headers without requiring a built-in host allowlist',
);

assert.deepEqual(
  buildHandoffAuthForUrl(
    'https://canvadocs-sin.instructure.com/v2/documents/fzhs8D-eL9dX',
    [
      { name: 'Cookie', value: 'canvas_session=abc' },
      { name: 'Referer', value: 'https://canvas.instructure.com/courses/1/files/2' },
    ],
    defaultAllowlistSettings,
  ),
  {
    headers: [
      { name: 'Cookie', value: 'canvas_session=abc' },
      { name: 'Referer', value: 'https://canvas.instructure.com/courses/1/files/2' },
    ],
  },
  'protected downloads should cover canvadocs PDF hosts without requiring a built-in host allowlist',
);

assert.deepEqual(
  buildHandoffAuthForUrl(
    'https://cdn.inst-fs-sin-prod.inscloudgate.net/f5a017c8-f85a-4dca/file.pptx',
    [
      { name: 'Cookie', value: 'canvas_session=abc' },
      { name: 'Referer', value: 'https://canvas.instructure.com/courses/1/files/2' },
    ],
    defaultAllowlistSettings,
  ),
  {
    headers: [
      { name: 'Cookie', value: 'canvas_session=abc' },
      { name: 'Referer', value: 'https://canvas.instructure.com/courses/1/files/2' },
    ],
  },
  'protected downloads should cover Canvas inscloudgate CDN hosts without requiring a built-in host allowlist',
);

assert.deepEqual(
  buildHandoffAuthForUrl(
    'https://download-cdn.other.test/file.pdf',
    [{ name: 'Cookie', value: 'session=abc' }],
    settings,
  ),
  { headers: [{ name: 'Cookie', value: 'session=abc' }] },
  'legacy global protected-download scope should preserve existing authenticated handoff behavior',
);

assert.equal(
  buildHandoffAuthForUrl(
    'https://download-cdn.other.test/file.pdf',
    [{ name: 'Cookie', value: 'session=abc' }],
    { ...settings, excludedHosts: ['download-cdn.other.test'] },
  ),
  undefined,
  'excluded sites should suppress protected-download browser session headers',
);

assert.equal(
  buildHandoffAuthForUrl(
    'https://chatgpt.com/backend-api/estuary/content?id=file_123',
    [{ name: 'Cookie', value: 'oai-did=1' }],
    disabledSettings,
  ),
  undefined,
  'disabled protected downloads must not attach browser auth',
);

captureHandoffAuthHeaders(
  {
    requestId: 'disabled-session-marker',
    url: 'https://chatgpt.com/file.zip',
    method: 'GET',
    incognito: false,
    requestHeaders: [{ name: 'Cookie', value: 'session=abc' }],
  },
  disabledSettings,
  900,
);
assert.equal(
  hasCapturedBrowserSessionHeaders(
    {
      requestId: 'disabled-session-marker',
      url: 'https://chatgpt.com/file.zip',
      incognito: false,
    },
    950,
  ),
  true,
  'browser session markers should be retained even when Protected Downloads is disabled',
);
assert.equal(
  hasCapturedHandoffAuth(
    {
      requestId: 'disabled-session-marker',
      url: 'https://chatgpt.com/file.zip',
      incognito: false,
    },
    950,
  ),
  false,
  'disabled Protected Downloads should not retain replayable handoff auth',
);

captureHandoffAuthHeaders(
  {
    requestId: 'accept-only-marker',
    url: 'https://chatgpt.com/public.zip',
    method: 'GET',
    incognito: false,
    requestHeaders: [{ name: 'Accept', value: 'application/zip' }],
  },
  allowlistSettings,
  975,
);
assert.equal(
  hasCapturedBrowserSessionHeaders(
    {
      requestId: 'accept-only-marker',
      url: 'https://chatgpt.com/public.zip',
      incognito: false,
    },
    1_000,
  ),
  false,
  'generic browser headers should not mark a download as session-protected',
);

captureHandoffAuthHeaders(
  {
    requestId: 'broad-cdn-capture',
    url: 'https://download-cdn.other.test/file.zip',
    method: 'GET',
    incognito: false,
    requestHeaders: [{ name: 'Cookie', value: 'session=abc' }],
  },
  allowlistSettings,
  1_000,
);
assert.equal(
  hasCapturedHandoffAuth(
    {
      requestId: 'broad-cdn-capture',
      url: 'https://download-cdn.other.test/file.zip',
      incognito: false,
    },
    1_200,
  ),
  true,
  'capture should retain browser session headers for any host when Protected Downloads is enabled',
);
assert.deepEqual(
  takeCapturedHandoffAuth(
    {
      requestId: 'broad-cdn-capture',
      url: 'https://download-cdn.other.test/file.zip',
      incognito: false,
    },
    allowlistSettings,
    1_225,
  ),
  { headers: [{ name: 'Cookie', value: 'session=abc' }] },
  'broad protected-download captures should be consumable for arbitrary CDN hosts',
);

captureHandoffAuthHeaders(
  {
    requestId: 'chrome-protected-cdn',
    url: 'https://download-cdn.other.test/file.rar',
    method: 'GET',
    incognito: false,
    requestHeaders: [{ name: 'Cookie', value: 'accountToken=secret' }],
  },
  allowlistSettings,
  1_300,
);
assert.deepEqual(
  resolveBrowserHandoffAuth(
    {
      requestId: 'chrome-protected-cdn',
      url: 'https://download-cdn.other.test/file.rar',
      incognito: false,
    },
    allowlistSettings,
    1_350,
  ),
  { status: 'ready', handoffAuth: { headers: [{ name: 'Cookie', value: 'accountToken=secret' }] } },
  'Chrome browser handoffs should attach captured session headers for arbitrary CDN hosts',
);

captureHandoffAuthHeaders(
  {
    requestId: 'chrome-protected-allowed',
    url: 'https://chatgpt.com/file.zip',
    method: 'GET',
    incognito: false,
    requestHeaders: [{ name: 'Cookie', value: 'session=abc' }],
  },
  allowlistSettings,
  1_400,
);
assert.deepEqual(
  resolveBrowserHandoffAuth(
    {
      requestId: 'chrome-protected-allowed',
      url: 'https://chatgpt.com/file.zip',
      incognito: false,
    },
    allowlistSettings,
    1_450,
  ),
  { status: 'ready', handoffAuth: { headers: [{ name: 'Cookie', value: 'session=abc' }] } },
  'Chrome browser handoffs should keep allowed memory-only auth when it was captured',
);

captureHandoffAuthHeaders(
  {
    requestId: 'firefox-canvadocs-object',
    url: 'https://canvadocs-sin.instructure.com/v2/documents/fzhs8D-eL9dX',
    method: 'GET',
    incognito: false,
    requestHeaders: [
      { name: 'Cookie', value: 'canvas_session=abc' },
      { name: 'Referer', value: 'https://canvas.instructure.com/courses/1/files/2' },
    ],
  },
  defaultAllowlistSettings,
  1_475,
);
assert.deepEqual(
  resolveBrowserHandoffAuth(
    {
      requestId: 'firefox-canvadocs-object',
      url: 'https://canvadocs-sin.instructure.com/v2/documents/fzhs8D-eL9dX',
      incognito: false,
    },
    defaultAllowlistSettings,
    1_490,
  ),
  {
    status: 'ready',
    handoffAuth: {
      headers: [
        { name: 'Cookie', value: 'canvas_session=abc' },
        { name: 'Referer', value: 'https://canvas.instructure.com/courses/1/files/2' },
      ],
    },
  },
  'Firefox canvadocs handoffs should attach captured same-request browser session headers',
);

assert.deepEqual(
  resolveBrowserHandoffAuth(
    {
      requestId: 'chrome-public-download',
      url: 'https://public.example.org/file.zip',
      incognito: false,
    },
    allowlistSettings,
    1_500,
  ),
  { status: 'ready' },
  'public browser downloads without session headers should still hand off normally',
);

captureHandoffAuthHeaders(
  {
    requestId: 'request-1',
    url: 'https://chatgpt.com/file.zip',
    method: 'GET',
    incognito: false,
    requestHeaders: [{ name: 'Cookie', value: 'session=abc' }],
  },
  allowlistSettings,
  1_000,
);
assert.equal(
  hasCapturedHandoffAuth(
    {
      requestId: 'request-1',
      url: 'https://chatgpt.com/file.zip',
      incognito: false,
    },
    1_200,
  ),
  true,
  'allowlisted captured auth should be detectable without consuming it',
);
assert.equal(
  takeCapturedHandoffAuth(
    {
      requestId: 'request-1',
      url: 'https://chatgpt.com/file.zip',
      incognito: false,
    },
    disabledSettings,
    1_225,
  ),
  undefined,
  'disabled protected downloads should clear captured auth instead of retaining session headers',
);
assert.equal(
  takeCapturedHandoffAuth(
    {
      requestId: 'request-1',
      url: 'https://chatgpt.com/file.zip',
      incognito: false,
    },
    allowlistSettings,
    1_250,
  ),
  undefined,
  'captured headers cleared while disabled should not become available after re-enabling',
);

captureHandoffAuthHeaders(
  {
    requestId: 'stale-request',
    url: 'https://chatgpt.com/backend-api/estuary/content?id=file_old',
    method: 'GET',
    requestHeaders: [{ name: 'Cookie', value: 'old=1' }],
  },
  allowlistSettings,
  10_000,
);
assert.equal(
  takeCapturedHandoffAuth(
    {
      requestId: 'stale-request',
      url: 'https://chatgpt.com/backend-api/estuary/content?id=file_old',
    },
    allowlistSettings,
    45_001,
  ),
  undefined,
  'expired captured auth should not be forwarded',
);

captureHandoffAuthHeaders(
  {
    requestId: 'url-fallback-source',
    url: 'https://chatgpt.com/backend-api/estuary/content?id=file_456',
    method: 'GET',
    requestHeaders: [{ name: 'Cookie', value: 'oai-did=2' }],
  },
  allowlistSettings,
  20_000,
);
assert.deepEqual(
  takeCapturedHandoffAuth(
    {
      url: 'https://chatgpt.com/backend-api/estuary/content?id=file_456',
    },
    allowlistSettings,
    20_500,
  ),
  { headers: [{ name: 'Cookie', value: 'oai-did=2' }] },
  'URL fallback should attach captured auth when the browser download lacks a request id',
);

captureHandoffAuthHeaders(
  {
    requestId: 'ambiguous-a',
    url: 'https://download-cdn.example.com/ambiguous.zip',
    method: 'GET',
    incognito: false,
    requestHeaders: [{ name: 'Cookie', value: 'a=1' }],
  },
  allowlistSettings,
  30_000,
);
captureHandoffAuthHeaders(
  {
    requestId: 'ambiguous-b',
    url: 'https://download-cdn.example.com/ambiguous.zip',
    method: 'GET',
    incognito: false,
    requestHeaders: [{ name: 'Cookie', value: 'b=1' }],
  },
  allowlistSettings,
  30_100,
);
assert.equal(
  takeCapturedHandoffAuth(
    {
      url: 'https://download-cdn.example.com/ambiguous.zip',
      incognito: false,
    },
    allowlistSettings,
    30_200,
  ),
  undefined,
  'URL fallback should refuse ambiguous fresh captures for the same URL',
);
assert.deepEqual(
  takeCapturedHandoffAuth(
    {
      requestId: 'ambiguous-b',
      url: 'https://download-cdn.example.com/ambiguous.zip',
      incognito: false,
    },
    allowlistSettings,
    30_250,
  ),
  { headers: [{ name: 'Cookie', value: 'b=1' }] },
  'request id matching should still consume a specific capture when URL fallback is ambiguous',
);

for (let index = 0; index < 70; index += 1) {
  captureHandoffAuthHeaders(
    {
      requestId: `eviction-${index}`,
      url: `https://download-cdn.example.com/eviction-${index}.zip`,
      method: 'GET',
      requestHeaders: [{ name: 'Cookie', value: `session=${index}` }],
    },
    allowlistSettings,
    40_000 + index,
  );
}
assert.equal(
  takeCapturedHandoffAuth(
    {
      requestId: 'eviction-0',
      url: 'https://download-cdn.example.com/eviction-0.zip',
    },
    allowlistSettings,
    40_100,
  ),
  undefined,
  'old captured auth entries should be evicted by a global memory cap',
);
assert.deepEqual(
  takeCapturedHandoffAuth(
    {
      requestId: 'eviction-69',
      url: 'https://download-cdn.example.com/eviction-69.zip',
    },
    allowlistSettings,
    40_100,
  ),
  { headers: [{ name: 'Cookie', value: 'session=69' }] },
  'newer captured auth entries should remain available after global eviction',
);

let cookieLookupCalls = 0;
const cookieFallback = await resolveBrowserHandoffAuthWithCookieFallback(
  {
    requestId: 'firefox-cookie-fallback',
    url: 'https://cdn.example.net/file.zip',
    incognito: false,
    cookieStoreId: 'firefox-container-1',
  },
  settings,
  {
    now: 50_000,
    userAgent: 'Mozilla/5.0 Test',
    async cookieLookup(details) {
      cookieLookupCalls += 1;
      assert.deepEqual(
        details,
        { url: 'https://cdn.example.net/file.zip', storeId: 'firefox-container-1' },
        'cookie fallback should query the matching URL and Firefox cookie store',
      );
      return [
        { name: 'sid', value: 'abc' },
        { name: 'pref', value: '1' },
      ];
    },
  },
);
assert.equal(cookieLookupCalls, 1);
assert.deepEqual(
  cookieFallback,
  {
    status: 'ready',
    handoffAuth: {
      headers: [
        { name: 'Cookie', value: 'sid=abc; pref=1' },
        { name: 'User-Agent', value: 'Mozilla/5.0 Test' },
      ],
    },
  },
  'Firefox cookie fallback should synthesize Cookie and User-Agent headers when webRequest omits Cookie',
);

captureHandoffAuthHeaders(
  {
    requestId: 'firefox-captured-cookie-preferred',
    url: 'https://cdn.example.net/preferred.zip',
    method: 'GET',
    incognito: false,
    requestHeaders: [{ name: 'Cookie', value: 'captured=1' }],
  },
  settings,
  51_000,
);
let capturedCookieLookupCalled = false;
assert.deepEqual(
  await resolveBrowserHandoffAuthWithCookieFallback(
    {
      requestId: 'firefox-captured-cookie-preferred',
      url: 'https://cdn.example.net/preferred.zip',
      incognito: false,
    },
    settings,
    {
      now: 51_100,
      userAgent: 'Mozilla/5.0 Test',
      async cookieLookup() {
        capturedCookieLookupCalled = true;
        return [{ name: 'sid', value: 'fallback' }];
      },
    },
  ),
  {
    status: 'ready',
    handoffAuth: {
      headers: [
        { name: 'Cookie', value: 'captured=1' },
        { name: 'User-Agent', value: 'Mozilla/5.0 Test' },
      ],
    },
  },
  'captured webRequest Cookie headers should be preferred over cookies API fallback',
);
assert.equal(capturedCookieLookupCalled, false);

assert.deepEqual(
  await resolveBrowserHandoffAuthWithCookieFallback(
    {
      requestId: 'firefox-cookie-rejected',
      url: 'https://cdn.example.net/rejected.zip',
      incognito: false,
    },
    settings,
    {
      now: 52_000,
      userAgent: 'Mozilla/5.0 Test',
      async cookieLookup() {
        throw new Error('cookies unavailable');
      },
    },
  ),
  { status: 'ready' },
  'cookie fallback should fail closed when the cookies API lookup rejects',
);

let excludedCookieLookupCalled = false;
assert.deepEqual(
  await resolveBrowserHandoffAuthWithCookieFallback(
    {
      requestId: 'firefox-cookie-excluded',
      url: 'https://excluded.example.net/file.zip',
      incognito: false,
    },
    { ...settings, excludedHosts: ['excluded.example.net'] },
    {
      now: 53_000,
      userAgent: 'Mozilla/5.0 Test',
      async cookieLookup() {
        excludedCookieLookupCalled = true;
        return [{ name: 'sid', value: 'abc' }];
      },
    },
  ),
  { status: 'ready' },
  'excluded sites should suppress Firefox cookie fallback auth',
);
assert.equal(excludedCookieLookupCalled, false);

const request = createEnqueueDownloadRequest(
  'https://chatgpt.com/backend-api/estuary/content?id=file_123',
  source,
  'request-1',
  {
    browserFallback: 'unavailable',
    handoffAuth: {
      headers: [
        { name: 'Cookie', value: 'oai-did=1' },
        { name: 'Range', value: 'bytes=0-' },
      ],
    },
  },
);
assert.equal(request.ok, true);
if (request.ok) {
  assert.deepEqual(request.value.payload.handoffAuth, {
    headers: [{ name: 'Cookie', value: 'oai-did=1' }],
  });
  assert.equal(
    request.value.payload.browserFallback,
    'unavailable',
    'browser fallback metadata should survive request validation',
  );
}
