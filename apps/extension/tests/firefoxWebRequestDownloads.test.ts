import assert from 'node:assert/strict';
import type { ExtensionIntegrationSettings } from '@myapp/protocol';
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
    incognito: false,
  },
  'Firefox attachment responses should be intercepted before the browser Save As dialog',
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
  firefoxWebRequestDownloadCandidate(details({ method: 'POST' }), defaultSettings),
  null,
  'non-replayable POST downloads should stay with Firefox',
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

assert.equal(
  firefoxWebRequestDownloadCandidate(details({}), {
    ...defaultSettings,
    ignoredFileExtensions: ['zip'],
  }),
  null,
  'ignored file extensions should bypass Firefox webRequest interception',
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
    incognito: true,
  },
  'known downloadable MIME types should use the URL basename when no attachment filename is present',
);
