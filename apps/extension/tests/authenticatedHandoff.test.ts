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
  hasCapturedHandoffAuth,
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
  authenticatedHandoffHosts: [],
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
    settings,
  ),
  { headers: [{ name: 'Cookie', value: 'oai-did=1' }] },
  'protected downloads should not require a host allowlist to receive captured browser auth',
);

assert.deepEqual(
  buildHandoffAuthForUrl(
    'https://example.org/file.pdf',
    [{ name: 'Cookie', value: 'session=abc' }],
    settings,
  ),
  { headers: [{ name: 'Cookie', value: 'session=abc' }] },
  'exact browser-download request auth should be controlled by the protected-download toggle, not by hosts',
);

assert.equal(
  buildHandoffAuthForUrl(
    'https://chatgpt.com/backend-api/estuary/content?id=file_123',
    [{ name: 'Cookie', value: 'oai-did=1' }],
    { ...settings, authenticatedHandoffEnabled: false },
  ),
  undefined,
  'disabled protected downloads must not attach browser auth',
);

captureHandoffAuthHeaders(
  {
    requestId: 'request-1',
    url: 'https://download-cdn.example.com/file.zip',
    method: 'GET',
    incognito: false,
    requestHeaders: [{ name: 'Cookie', value: 'session=abc' }],
  },
  1_000,
);
assert.equal(
  hasCapturedHandoffAuth(
    {
      requestId: 'request-1',
      url: 'https://download-cdn.example.com/file.zip',
      incognito: false,
    },
    1_200,
  ),
  true,
  'captured auth should be detectable without consuming it',
);
assert.equal(
  takeCapturedHandoffAuth(
    {
      requestId: 'request-1',
      url: 'https://download-cdn.example.com/file.zip',
      incognito: false,
    },
    { ...settings, authenticatedHandoffEnabled: false },
    1_225,
  ),
  undefined,
  'disabled protected downloads should clear captured auth instead of retaining session headers',
);
assert.equal(
  takeCapturedHandoffAuth(
    {
      requestId: 'request-1',
      url: 'https://download-cdn.example.com/file.zip',
      incognito: false,
    },
    settings,
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
  10_000,
);
assert.equal(
  takeCapturedHandoffAuth(
    {
      requestId: 'stale-request',
      url: 'https://chatgpt.com/backend-api/estuary/content?id=file_old',
    },
    settings,
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
  20_000,
);
assert.deepEqual(
  takeCapturedHandoffAuth(
    {
      url: 'https://chatgpt.com/backend-api/estuary/content?id=file_456',
    },
    settings,
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
  30_100,
);
assert.equal(
  takeCapturedHandoffAuth(
    {
      url: 'https://download-cdn.example.com/ambiguous.zip',
      incognito: false,
    },
    settings,
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
    settings,
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
    40_000 + index,
  );
}
assert.equal(
  takeCapturedHandoffAuth(
    {
      requestId: 'eviction-0',
      url: 'https://download-cdn.example.com/eviction-0.zip',
    },
    settings,
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
    settings,
    40_100,
  ),
  { headers: [{ name: 'Cookie', value: 'session=69' }] },
  'newer captured auth entries should remain available after global eviction',
);

const request = createEnqueueDownloadRequest(
  'https://chatgpt.com/backend-api/estuary/content?id=file_123',
  source,
  'request-1',
  {
    headers: [
      { name: 'Cookie', value: 'oai-did=1' },
      { name: 'Range', value: 'bytes=0-' },
    ],
  },
);
assert.equal(request.ok, true);
if (request.ok) {
  assert.deepEqual(request.value.payload.handoffAuth, {
    headers: [{ name: 'Cookie', value: 'oai-did=1' }],
  });
}
