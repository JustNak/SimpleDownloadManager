import assert from 'node:assert/strict';
import { DEFAULT_CAPTURED_FILE_EXTENSIONS, type ExtensionIntegrationSettings } from '@myapp/protocol';
import {
  firefoxWebRequestDownloadCandidate,
  type FirefoxWebRequestDownloadDetails,
} from '../src/background/browserDownloads.ts';

const defaultSettings: ExtensionIntegrationSettings = {
  enabled: true,
  downloadHandoffMode: 'ask',
  listenPort: 1420,
  contextMenuEnabled: true,
  showProgressAfterHandoff: true,
  showBadgeStatus: true,
  excludedHosts: [],
  ignoredFileExtensions: [],
  capturedFileExtensions: [...DEFAULT_CAPTURED_FILE_EXTENSIONS],
  downloadCaptureDebugLogging: false,
};

function details(update: Partial<FirefoxWebRequestDownloadDetails>): FirefoxWebRequestDownloadDetails {
  return {
    url: 'https://downloads.example.com/movie.zip',
    method: 'GET',
    type: 'main_frame',
    responseHeaders: [
      {
        name: 'Content-Disposition',
        value: 'attachment; filename="movie.zip"',
      },
      {
        name: 'Content-Length',
        value: '1024',
      },
    ],
    ...update,
  };
}

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(details({}), defaultSettings),
  {
    url: 'https://downloads.example.com/movie.zip',
    filename: 'movie.zip',
    totalBytes: 1024,
    reason: 'attachment_disposition',
    incognito: false,
  },
  'Firefox attachment responses should be intercepted before the browser Save As dialog',
);

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(details({ cookieStoreId: 'firefox-container-1' }), defaultSettings),
  {
    url: 'https://downloads.example.com/movie.zip',
    filename: 'movie.zip',
    totalBytes: 1024,
    reason: 'attachment_disposition',
    incognito: false,
    cookieStoreId: 'firefox-container-1',
  },
  'Firefox webRequest candidates should preserve cookieStoreId for container-aware cookie fallback',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      responseHeaders: [{ name: 'Content-Type', value: 'text/html' }],
    }),
    defaultSettings,
  ),
  null,
  'normal HTML navigation should not be intercepted',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://search.brave.com/search?q=What%20is%20the%20date%3F%20%21g&source=web',
      statusCode: 307,
      responseHeaders: [
        { name: 'Content-Type', value: 'application/octet-stream' },
        { name: 'Location', value: 'https://www.google.com/search?q=What%20is%20the%20date%3F' },
      ],
    }),
    defaultSettings,
  ),
  null,
  'Brave bang redirects with binary content type should stay as browser navigation',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://downloads.example.com/redirect',
      statusLine: 'HTTP/2 302 Found',
      responseHeaders: [
        { name: 'Content-Disposition', value: 'attachment; filename="redirect.zip"' },
        { name: 'Location', value: 'https://downloads.example.com/movie.zip' },
      ],
    }),
    defaultSettings,
  ),
  null,
  'Firefox should not intercept redirect responses even when they look like attachments',
);

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(details({ method: 'POST' }), defaultSettings),
  {
    url: 'https://downloads.example.com/movie.zip',
    filename: 'movie.zip',
    totalBytes: 1024,
    reason: 'attachment_disposition',
    incognito: false,
  },
  'POST downloads with strong attachment evidence should still be classified for SDM handoff',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(details({ type: 'image' }), defaultSettings),
  null,
  'page resources should not be intercepted as downloads',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(details({}), {
    ...defaultSettings,
    excludedHosts: ['example.com'],
  }),
  null,
  'excluded hosts should bypass Firefox webRequest interception',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(details({}), {
    ...defaultSettings,
    excludedHosts: ['*.example.com'],
  }),
  null,
  'wildcard excluded hosts should bypass Firefox webRequest interception',
);

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(details({}), {
    ...defaultSettings,
    ignoredFileExtensions: ['zip'],
  }),
  {
    url: 'https://downloads.example.com/movie.zip',
    filename: 'movie.zip',
    totalBytes: 1024,
    reason: 'attachment_disposition',
    incognito: false,
  },
  'legacy ignored extensions should no longer suppress Firefox webRequest capture',
);

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://downloads.example.com/movie.zip?token=abc',
      responseHeaders: [{ name: 'Content-Type', value: 'application/zip' }],
      incognito: true,
    }),
    defaultSettings,
  ),
  {
    url: 'https://downloads.example.com/movie.zip?token=abc',
    filename: 'movie.zip',
    totalBytes: undefined,
    reason: 'strong_filename',
    incognito: true,
  },
  'known downloadable MIME types should use the URL basename when no attachment filename is present',
);

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://downloads.example.com/download?id=opaque',
      type: 'main_frame',
      responseHeaders: [
        { name: 'Content-Type', value: 'application/zip' },
        { name: 'Content-Length', value: String(8 * 1024 * 1024) },
      ],
    }),
    defaultSettings,
  ),
  {
    url: 'https://downloads.example.com/download?id=opaque',
    filename: 'download',
    totalBytes: 8 * 1024 * 1024,
    reason: 'download_mime',
    incognito: false,
  },
  'Firefox should catch large top-level opaque responses with known download MIME types',
);

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://downloads.example.com/report',
      responseHeaders: [
        {
          name: 'Content-Disposition',
          value: "attachment; filename*=UTF-8'en-us'report%20final.zip",
        },
        {
          name: 'Content-Length',
          value: '2048',
        },
      ],
    }),
    defaultSettings,
  ),
  {
    url: 'https://downloads.example.com/report',
    filename: 'report final.zip',
    totalBytes: 2048,
    reason: 'attachment_disposition',
    incognito: false,
  },
  'Firefox attachment filenames should decode RFC 5987 filename* values with a language tag',
);

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://downloads.example.com/installer.dmg',
      responseHeaders: [{ name: 'Content-Type', value: 'application/x-apple-diskimage' }],
    }),
    defaultSettings,
  ),
  {
    url: 'https://downloads.example.com/installer.dmg',
    filename: 'installer.dmg',
    totalBytes: undefined,
    reason: 'strong_filename',
    incognito: false,
  },
  'Firefox should intercept common disk-image installer downloads even without Content-Disposition',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://downloads.example.com/archive',
      responseHeaders: [
        { name: 'Content-Disposition', value: 'attachment; filename="archive.custom"' },
        { name: 'Content-Type', value: 'application/x-custom-download' },
      ],
    }),
    defaultSettings,
  ),
  null,
  'Firefox attachment responses should not bypass the configured captured extension list',
);

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://canvas.instructure.com/files/569/download?download_frd=1&verifier=c6Hd',
      type: 'xmlhttprequest',
      responseHeaders: [
        { name: 'Content-Disposition', value: 'attachment; filename="lecture.pdf"' },
        { name: 'Content-Length', value: '4096' },
      ],
    }),
    defaultSettings,
  ),
  {
    url: 'https://canvas.instructure.com/files/569/download?download_frd=1&verifier=c6Hd',
    filename: 'lecture.pdf',
    totalBytes: 4096,
    reason: 'explicit_download_url',
    incognito: false,
  },
  'Firefox should intercept Canvas/Instructure attachment downloads delivered through XHR requests',
);

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://canvas.school.edu/files/569/download?download_frd=1&verifier=c6Hd',
      type: 'other',
      responseHeaders: [{ name: 'Content-Type', value: 'text/html' }],
    }),
    defaultSettings,
  ),
  {
    url: 'https://canvas.school.edu/files/569/download?download_frd=1&verifier=c6Hd',
    filename: 'download',
    totalBytes: undefined,
    reason: 'explicit_download_url',
    incognito: false,
  },
  'Firefox should classify explicit Canvas download URLs even on custom Canvas domains',
);

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://downloads.example.com/course/report-final.docx?token=abc',
      type: 'object',
      responseHeaders: [],
    }),
    defaultSettings,
  ),
  {
    url: 'https://downloads.example.com/course/report-final.docx?token=abc',
    filename: 'report-final.docx',
    totalBytes: undefined,
    reason: 'strong_filename',
    incognito: false,
  },
  'Firefox should classify strong filename-extension download URLs beyond frame navigation',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://downloads.example.com/course/report-final.docx?token=abc',
      type: 'object',
      responseHeaders: [],
    }),
    {
      ...defaultSettings,
      capturedFileExtensions: defaultSettings.capturedFileExtensions.filter((extension) => extension !== 'docx'),
    },
  ),
  null,
  'Firefox should stop classifying removed captured extensions as strong filename downloads',
);

assert.deepEqual(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://downloads.example.com/export/report.pkgx?token=abc',
      type: 'object',
      responseHeaders: [],
    }),
    { ...defaultSettings, capturedFileExtensions: ['pkgx'] },
  ),
  {
    url: 'https://downloads.example.com/export/report.pkgx?token=abc',
    filename: 'report.pkgx',
    totalBytes: undefined,
    reason: 'strong_filename',
    incognito: false,
  },
  'Firefox should classify user configured filename-extension download URLs',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://downloads.example.com/export/report.pkgx?token=abc',
      type: 'xmlhttprequest',
      responseHeaders: [],
    }),
    { ...defaultSettings, capturedFileExtensions: ['pkgx'] },
  ),
  null,
  'Firefox should not classify page-internal custom-extension XHRs without explicit download intent',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://canvas.instructure.com/api/v1/courses/1/files',
      type: 'xmlhttprequest',
      responseHeaders: [{ name: 'Content-Type', value: 'application/json' }],
    }),
    defaultSettings,
  ),
  null,
  'Firefox should not classify normal Canvas API JSON requests as downloads',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://music.youtube.com/youtubei/v1/player?prettyPrint=false',
      method: 'POST',
      type: 'xmlhttprequest',
      responseHeaders: [{ name: 'Content-Type', value: 'application/octet-stream' }],
    }),
    defaultSettings,
  ),
  null,
  'Firefox should not classify YouTube Music API JSON payloads as downloads even when they use a generic binary MIME type',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://music.youtube.com/verify_session',
      type: 'main_frame',
      responseHeaders: [
        { name: 'Content-Type', value: 'application/octet-stream' },
        { name: 'Content-Length', value: '0' },
      ],
    }),
    defaultSettings,
  ),
  null,
  'Firefox should not classify YouTube Music session verification payloads as downloads',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://music.youtube.com/verify_session',
      type: 'main_frame',
      responseHeaders: [
        { name: 'Content-Disposition', value: 'attachment; filename="json.txt"' },
        { name: 'Content-Type', value: 'application/octet-stream' },
        { name: 'Content-Length', value: '0' },
      ],
    }),
    defaultSettings,
  ),
  null,
  'Firefox should ignore zero-byte YouTube Music session attachments with generic JSON filenames',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://music.youtube.com/verify_session',
      type: 'main_frame',
      responseHeaders: [
        { name: 'Content-Disposition', value: 'attachment; filename="json.txt"' },
        { name: 'Content-Type', value: 'application/octet-stream' },
        { name: 'Content-Length', value: '0' },
      ],
    }),
    { ...defaultSettings, capturedFileExtensions: ['txt'] },
  ),
  null,
  'Firefox user captured extensions should not override high-confidence app traffic probes',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://music.youtube.com/verify_session',
      type: 'main_frame',
      responseHeaders: [
        { name: 'Content-Disposition', value: 'attachment; filename="json.txt"' },
      ],
    }),
    defaultSettings,
  ),
  null,
  'Firefox should ignore YouTube Music session attachments even when size and MIME metadata are missing',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://www.youtube.com/api/timedtext?v=RZpz24nP1P0&ei=xUE',
      type: 'xmlhttprequest',
      responseHeaders: [
        { name: 'Content-Disposition', value: 'attachment; filename="t.txt"' },
        { name: 'Content-Type', value: 'text/plain' },
        { name: 'Content-Length', value: '7680' },
      ],
    }),
    defaultSettings,
  ),
  null,
  'Firefox should not prompt for YouTube timedtext txt attachment responses',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://api.example.test/v1/player/archive.zip',
      type: 'xmlhttprequest',
      responseHeaders: [
        { name: 'Content-Disposition', value: 'attachment; filename="archive.zip"' },
        { name: 'Content-Type', value: 'application/octet-stream' },
        { name: 'Content-Length', value: '4096' },
      ],
    }),
    { ...defaultSettings, capturedFileExtensions: ['zip'] },
  ),
  null,
  'Firefox should not prompt for page-internal API payloads even with configured extension filenames',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://files.example.test/background/export.zip',
      type: 'xmlhttprequest',
      responseHeaders: [
        { name: 'Content-Disposition', value: 'attachment; filename="export.zip"' },
        { name: 'Content-Type', value: 'application/octet-stream' },
        { name: 'Content-Length', value: '1048576' },
      ],
    }),
    { ...defaultSettings, capturedFileExtensions: ['zip'] },
  ),
  null,
  'Firefox should require explicit download intent before intercepting page-internal requests',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://canvas.instructure.com/api/v1/courses/1/files',
      type: 'xmlhttprequest',
      responseHeaders: [{ name: 'Content-Type', value: 'application/x-protobuf' }],
    }),
    defaultSettings,
  ),
  null,
  'Firefox should not classify Canvas API protobuf requests as downloads',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://web.telegram.org/k/version',
      type: 'main_frame',
      responseHeaders: [
        { name: 'Content-Type', value: 'application/octet-stream' },
        { name: 'Content-Length', value: '9' },
      ],
    }),
    defaultSettings,
  ),
  null,
  'Firefox should not classify Telegram Web version probes as user downloads',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://app.example.com/api/files/123',
      method: 'POST',
      type: 'xmlhttprequest',
      responseHeaders: [
        { name: 'Content-Disposition', value: 'attachment; filename="export.zip"' },
        { name: 'Content-Type', value: 'application/octet-stream' },
      ],
    }),
    defaultSettings,
  ),
  null,
  'Firefox should not pre-cancel page-internal API responses based only on attachment headers',
);

assert.equal(
  firefoxWebRequestDownloadCandidate(
    details({
      url: 'https://cdn.example.com/signed',
      statusCode: 302,
      type: 'other',
      responseHeaders: [
        { name: 'Content-Type', value: 'application/octet-stream' },
        { name: 'Location', value: 'https://cdn.example.com/file.zip?Signature=abc&Expires=123' },
      ],
    }),
    defaultSettings,
  ),
  null,
  'Firefox should wait for the final signed redirect response before classifying a download',
);
