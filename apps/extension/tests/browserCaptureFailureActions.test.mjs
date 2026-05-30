import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const backgroundSource = readFileSync(new URL('../src/background/index.ts', import.meta.url), 'utf8');
const stateSource = readFileSync(new URL('../src/background/state.ts', import.meta.url), 'utf8');
const popupSource = readFileSync(new URL('../src/popup/index.ts', import.meta.url), 'utf8');
const popupHtml = readFileSync(new URL('../src/popup/index.html', import.meta.url), 'utf8');
const messagesSource = readFileSync(new URL('../src/shared/messages.ts', import.meta.url), 'utf8');

assert.doesNotMatch(
  messagesSource,
  /pendingBrowserCapture|BrowserCaptureRetry|popup_retry_browser_capture|popup_cancel_browser_capture/,
  'popup state/messages should not expose failed browser-capture retry UI',
);

assert.doesNotMatch(
  stateSource,
  /pendingBrowserCapture|BrowserCaptureRetry|clearPendingBrowserCapture/,
  'background state should not persist failed capture retry metadata for the popup',
);

assert.doesNotMatch(
  backgroundSource,
  /recordHostError\(response, capture\)|retryPendingBrowserCapture|popup_retry_browser_capture|popup_cancel_browser_capture/,
  'background should not keep popup retry/cancel handling for failed captures',
);

assert.doesNotMatch(
  popupHtml,
  /browser-capture-error|retry-browser-capture-button|cancel-browser-capture-button|Download failed/,
  'popup should not render a browser capture failure panel',
);

assert.doesNotMatch(
  popupSource,
  /pendingBrowserCapture|browserCaptureError|retryBrowserCaptureButton|cancelBrowserCaptureButton|popup_retry_browser_capture|popup_cancel_browser_capture/,
  'popup script should not render failed capture state or send retry/cancel messages',
);
