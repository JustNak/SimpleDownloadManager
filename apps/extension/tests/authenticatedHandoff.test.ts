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
  takeCapturedHandoffAuth,
} from '../src/background/handoffAuth.ts';

const source: RequestSource = {
  entryPoint: 'browser_download',
  browser: 'chrome',
  extensionVersion: '0.3.43',
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
  authenticatedHandoffHosts: ['chatgpt.com', 'download*.example.com'],
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
  'allowlisted ChatGPT file URLs should receive captured browser auth',
);

assert.equal(
  buildHandoffAuthForUrl(
    'https://example.org/file.pdf',
    [{ name: 'Cookie', value: 'session=abc' }],
    settings,
  ),
  undefined,
  'non-allowlisted hosts must not receive browser auth',
);

assert.equal(
  buildHandoffAuthForUrl(
    'https://chatgpt.com/backend-api/estuary/content?id=file_123',
    [{ name: 'Cookie', value: 'oai-did=1' }],
    { ...settings, authenticatedHandoffEnabled: false },
  ),
  undefined,
  'disabled authenticated handoff must not attach browser auth',
);

captureHandoffAuthHeaders(
  {
    requestId: 'request-1',
    url: 'https://download-cdn.example.com/file.zip',
    method: 'GET',
    incognito: false,
    requestHeaders: [{ name: 'Cookie', value: 'session=abc' }],
  },
  settings,
  1_000,
);
assert.deepEqual(
  takeCapturedHandoffAuth(
    {
      requestId: 'request-1',
      url: 'https://download-cdn.example.com/file.zip',
      incognito: false,
    },
    settings,
    1_250,
  ),
  { headers: [{ name: 'Cookie', value: 'session=abc' }] },
  'recent captured headers should be consumed by request id for a matching allowlisted host',
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
