import { isErrorResponse, toUserFacingMessage, type ExtensionIntegrationSettings, type HostToExtensionResponse, type PongPayload } from '@myapp/protocol';
import browser from './browser';
import {
  browserDownloadUrl,
  createBrowserDownloadHandoffMetadata,
  createBrowserDownloadBypassState,
  createAsyncFilenameInterceptionListener,
  detachBrowserDownloadForDesktopPrompt,
  discardBrowserDownload,
  firefoxWebRequestDownloadCandidate,
  markBrowserDownloadBypassUrl,
  restartBrowserDownload,
  restoreBrowserDownloadAfterPromptFallback,
  revokeBrowserDownloadBypassUrl,
  selectFilenameInterceptionApi,
  shouldBypassBrowserDownload,
  shouldBypassBrowserDownloadUrl,
  shouldHandleBrowserDownload,
  shouldDiscardBrowserDownloadAfterHandoff,
  shouldRestoreBrowserDownloadAfterPromptSwap,
  type BrowserDownloadFilenameInterceptionApi,
  type BrowserDownloadFilenameInterceptionCandidate,
  type BrowserDownloadFilenameSuggest,
  type BrowserDownloadFilenameSuggestion,
  type FirefoxWebRequestDownloadCandidate,
  type FirefoxWebRequestDownloadDetails,
} from './browserDownloads';
import {
  captureHandoffAuthHeaders,
  hasCapturedHandoffAuth,
  takeCapturedHandoffAuth,
  type HandoffAuthRequestDetails,
} from './handoffAuth';
import { buildContextMenuPayload, connectionForErrorCode, enqueueDownload, openApp, pingNativeHost, promptDownload, saveExtensionSettings } from './nativeMessaging';
import { getExtensionSettings, getPopupState, setExtensionSettings, setHostError, setLastResult, updatePopupState } from './state';
import type { PopupRequest, PopupStateResponse } from '../shared/messages';

const CONTEXT_MENU_ID = 'download-with-myapp';
const FIREFOX_FALLBACK_BYPASS_TTL_MS = 10_000;
const activeBrowserDownloadIds = new Set<number>();
const browserDownloadFallbackBypass = createBrowserDownloadBypassState();
let cachedExtensionSettings: ExtensionIntegrationSettings | null = null;

async function ensureContextMenu() {
  const settings = await getCachedExtensionSettings();
  await browser.contextMenus.removeAll();

  if (!settings.enabled || !settings.contextMenuEnabled) {
    return;
  }

  await browser.contextMenus.create({
    id: CONTEXT_MENU_ID,
    title: 'Download with Simple Download Manager',
    contexts: ['link'],
  });
}

async function refreshConnectionState() {
  const response = await pingNativeHost();
  if (isErrorResponse(response)) {
    const connection = connectionForErrorCode(response.code);
    const state = await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
    await updateBrowserBadge(state);
    return response;
  }

  const state = rememberStateSettings(await setLastResult('connected', response));
  await ensureContextMenu();
  await updateBrowserBadge(state);
  return response;
}

async function handleContextMenuClick(info: browser.contextMenus.OnClickData, tab?: browser.tabs.Tab) {
  const settings = await getCachedExtensionSettings();
  if (!settings.enabled || !settings.contextMenuEnabled) {
    return;
  }

  const payload = buildContextMenuPayload(info, tab);
  if (!payload) {
    await setHostError('INVALID_URL', 'The selected link did not include a URL.', 'error');
    return;
  }

  await updatePopupState({ isSubmitting: true });

  const response = await enqueueDownload(payload.url, payload.source);
  if (isErrorResponse(response)) {
    const connection = connectionForErrorCode(response.code);
    const state = await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
    await updateBrowserBadge(state);
    return;
  }

  const state = rememberStateSettings(await setLastResult('connected', response));
  await updateBrowserBadge(state);
}

async function handleBrowserDownloadCreated(item: browser.downloads.DownloadItem) {
  if (shouldSkipBrowserDownloadInterception(item)) {
    return;
  }

  const url = browserDownloadUrl(item);
  if (!url) {
    return;
  }

  let settings = await getCachedExtensionSettings();
  if (!shouldHandleBrowserDownload(item, settings)) {
    return;
  }

  activeBrowserDownloadIds.add(item.id);

  try {
    await browser.downloads.cancel(item.id).catch(() => undefined);

    const pingResponse = await pingNativeHost();
    if (isErrorResponse(pingResponse)) {
      await recordHostError(pingResponse);
      await restoreBrowserDownloadFallback(item);
      return;
    }

    rememberStateSettings(await setLastResult('connected', pingResponse));
    settings = rememberSettings(getSyncedSettings(pingResponse, settings));
    if (!shouldHandleBrowserDownload(item, settings)) {
      await restoreBrowserDownloadFallback(item);
      return;
    }

    const response = await handOffBrowserDownload(url, item, settings);

    if (isErrorResponse(response)) {
      await recordHostError(response);
      await restoreBrowserDownloadFallback(item);
      return;
    }

    if (shouldDiscardBrowserDownloadAfterHandoff(response)) {
      await discardBrowserDownload(browser.downloads, item.id);
      const state = rememberStateSettings(await setLastResult('connected', response));
      await updateBrowserBadge(state);
      return;
    }

    const state = rememberStateSettings(await setLastResult('connected', response));
    await updateBrowserBadge(state);
    await restoreBrowserDownloadFallback(item);
  } catch (error) {
    const state = await setHostError(
      'HOST_NOT_AVAILABLE',
      error instanceof Error ? error.message : 'Could not hand the browser download to the desktop app.',
      'error',
    );
    await updateBrowserBadge(state);
    await restoreBrowserDownloadFallback(item);
  } finally {
    activeBrowserDownloadIds.delete(item.id);
  }
}

async function handleBrowserDownloadDeterminingFilename(
  item: browser.downloads.DownloadItem,
  suggest: BrowserDownloadFilenameSuggest,
) {
  if (shouldSkipBrowserDownloadInterception(item)) {
    suggestBrowserDownload(item, suggest);
    return;
  }

  const url = browserDownloadUrl(item);
  if (!url) {
    suggestBrowserDownload(item, suggest);
    return;
  }

  let settings = await getCachedExtensionSettings();
  if (!shouldHandleBrowserDownload(item, settings)) {
    suggestBrowserDownload(item, suggest);
    return;
  }

  activeBrowserDownloadIds.add(item.id);
  const releaseFilename = () => {
    suggestBrowserDownload(item, suggest);
  };

  try {
    await detachBrowserDownloadForDesktopPrompt(browser.downloads, item.id, releaseFilename);

    const pingResponse = await pingNativeHost();
    if (isErrorResponse(pingResponse)) {
      await recordHostError(pingResponse);
      return;
    }

    const pingState = rememberStateSettings(await setLastResult('connected', pingResponse));
    settings = rememberSettings(getSyncedSettings(pingResponse, settings));
    if (!shouldHandleBrowserDownload(item, settings)) {
      await updateBrowserBadge(pingState);
      return;
    }

    const response = await handOffBrowserDownload(url, item, settings);

    if (isErrorResponse(response)) {
      await recordHostError(response);
      return;
    }

    const state = rememberStateSettings(await setLastResult('connected', response));
    await updateBrowserBadge(state);
    if (shouldRestoreBrowserDownloadAfterPromptSwap(response)) {
      await restoreBrowserDownloadFallback(item);
    }
  } catch (error) {
    const state = await setHostError(
      'HOST_NOT_AVAILABLE',
      error instanceof Error ? error.message : 'Could not hand the browser download to the desktop app.',
      'error',
    );
    await updateBrowserBadge(state);
  } finally {
    activeBrowserDownloadIds.delete(item.id);
  }
}

browser.runtime.onInstalled.addListener(() => {
  void refreshConnectionState();
});

browser.runtime.onStartup.addListener(() => {
  void refreshConnectionState();
});

browser.contextMenus.onClicked.addListener((info: browser.contextMenus.OnClickData, tab?: browser.tabs.Tab) => {
  if (info.menuItemId !== CONTEXT_MENU_ID) {
    return;
  }

  void handleContextMenuClick(info, tab);
});

const filenameInterceptionApi = getFilenameInterceptionApi();
if (filenameInterceptionApi) {
  filenameInterceptionApi.onDeterminingFilename.addListener(
    createAsyncFilenameInterceptionListener(handleBrowserDownloadDeterminingFilename),
  );
} else {
  browser.downloads?.onCreated.addListener((item) => {
    void handleBrowserDownloadCreated(item);
  });
  registerFirefoxWebRequestInterception();
}

browser.runtime.onMessage.addListener(async (message: PopupRequest) => {
  switch (message.type) {
    case 'popup_ping':
      return refreshConnectionState();
    case 'popup_get_state':
      return getPopupState();
    case 'popup_open_options': {
      await openOptionsPage();
      return getPopupState();
    }
    case 'extension_settings_update': {
      const cachedSettings = rememberSettings(await setExtensionSettings(message.settings));
      await ensureContextMenu();
      const response = await saveExtensionSettings(cachedSettings);
      if (isErrorResponse(response)) {
        const connection = connectionForErrorCode(response.code);
        const state = await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
        await updateBrowserBadge(state);
        return state;
      }

      const state = rememberStateSettings(await setLastResult('connected', response));
      await ensureContextMenu();
      await updateBrowserBadge(state);
      return state;
    }
    case 'popup_open_app':
    case 'popup_open_settings': {
      await updatePopupState({ isSubmitting: true });
      const response = await openApp();
      if (isErrorResponse(response)) {
        const connection = connectionForErrorCode(response.code);
        const state = await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
        await updateBrowserBadge(state);
        return response;
      }

      const state = rememberStateSettings(await setLastResult('connected', response));
      await updateBrowserBadge(state);
      return response;
    }
    case 'popup_enqueue': {
      await updatePopupState({ isSubmitting: true });
      const response = await enqueueDownload(message.url, {
        entryPoint: 'popup',
        extensionVersion: browser.runtime.getManifest().version,
      });

      if (isErrorResponse(response)) {
        const connection = connectionForErrorCode(response.code);
        const state = await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
        await updateBrowserBadge(state);
        return response;
      }

      const state = rememberStateSettings(await setLastResult('connected', response));
      await updateBrowserBadge(state);
      return response;
    }
    default:
      return getPopupState();
  }
});

void refreshConnectionState();
registerHandoffAuthHeaderCapture();

function shouldSkipBrowserDownloadInterception(item: browser.downloads.DownloadItem): boolean {
  return shouldBypassBrowserDownload(item, browserDownloadFallbackBypass)
    || activeBrowserDownloadIds.has(item.id)
    || !browserDownloadUrl(item);
}

async function handOffBrowserDownload(
  url: string,
  item: BrowserDownloadHandoffItem,
  settings: ExtensionIntegrationSettings,
): Promise<HostToExtensionResponse> {
  const source = {
    entryPoint: 'browser_download' as const,
    extensionVersion: browser.runtime.getManifest().version,
    incognito: item.incognito,
  };
  const handoffDetails = {
    requestId: 'requestId' in item ? item.requestId : undefined,
    url,
    incognito: item.incognito,
  };
  if (!settings.authenticatedHandoffEnabled && hasCapturedHandoffAuth(handoffDetails)) {
    return {
      ok: false,
      requestId: 'protected_downloads_disabled',
      type: 'rejected',
      code: 'PROTECTED_DOWNLOAD_AUTH_REQUIRED',
      message: 'This site requires your browser session. Enable Protected Downloads or let the browser handle this download.',
    };
  }

  const handoffAuth = takeCapturedHandoffAuth(handoffDetails, settings);

  const metadata = createBrowserDownloadHandoffMetadata(item, handoffAuth);

  if (settings.downloadHandoffMode === 'auto') {
    return enqueueDownload(url, source, metadata);
  }

  return promptDownload(url, source, metadata);
}

async function recordHostError(response: Extract<HostToExtensionResponse, { ok: false }>): Promise<void> {
  const connection = connectionForErrorCode(response.code);
  const state = await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
  await updateBrowserBadge(state);
}

async function restoreBrowserDownloadFallback(
  item: browser.downloads.DownloadItem,
  releaseFilename?: () => void,
): Promise<void> {
  try {
    if (releaseFilename) {
      await restoreBrowserDownloadAfterPromptFallback(
        browser.downloads,
        item,
        browserDownloadFallbackBypass,
        releaseFilename,
      );
      return;
    }

    await discardBrowserDownload(browser.downloads, item.id);
    await restartBrowserDownload(browser.downloads, item, browserDownloadFallbackBypass);
  } catch (error) {
    const state = await setHostError(
      'DOWNLOAD_FAILED',
      error instanceof Error
        ? `Could not return the download to the browser: ${error.message}`
        : 'Could not return the download to the browser.',
      'error',
    );
    await updateBrowserBadge(state);
  }
}

async function handleFirefoxWebRequestDownload(
  candidate: FirefoxWebRequestDownloadCandidate,
  initialSettings: ExtensionIntegrationSettings,
): Promise<void> {
  let settings = rememberSettings(initialSettings);

  try {
    const pingResponse = await pingNativeHost();
    if (isErrorResponse(pingResponse)) {
      await recordHostError(pingResponse);
      await restoreFirefoxWebRequestFallback(candidate);
      return;
    }

    rememberStateSettings(await setLastResult('connected', pingResponse));
    settings = rememberSettings(getSyncedSettings(pingResponse, settings));
    if (!shouldHandleBrowserDownload({ url: candidate.url, filename: candidate.filename }, settings)) {
      await restoreFirefoxWebRequestFallback(candidate);
      return;
    }

    const response = await handOffBrowserDownload(candidate.url, candidate, settings);

    if (isErrorResponse(response)) {
      await recordHostError(response);
      await restoreFirefoxWebRequestFallback(candidate);
      return;
    }

    if (!shouldDiscardBrowserDownloadAfterHandoff(response)) {
      const state = rememberStateSettings(await setLastResult('connected', response));
      await updateBrowserBadge(state);
      await restoreFirefoxWebRequestFallback(candidate);
      return;
    }

    const state = rememberStateSettings(await setLastResult('connected', response));
    await updateBrowserBadge(state);
  } catch (error) {
    const state = await setHostError(
      'HOST_NOT_AVAILABLE',
      error instanceof Error ? error.message : 'Could not hand the Firefox download to the desktop app.',
      'error',
    );
    await updateBrowserBadge(state);
    await restoreFirefoxWebRequestFallback(candidate);
  }
}

async function restoreFirefoxWebRequestFallback(candidate: FirefoxWebRequestDownloadCandidate): Promise<void> {
  const releaseExtraBypass = markFirefoxWebRequestBypass(candidate.url);

  try {
    await restartBrowserDownload(
      browser.downloads,
      {
        id: -1,
        url: candidate.url,
        filename: candidate.filename,
      },
      browserDownloadFallbackBypass,
    );
  } catch (error) {
    releaseExtraBypass();
    const state = await setHostError(
      'DOWNLOAD_FAILED',
      error instanceof Error
        ? `Could not return the Firefox download to the browser: ${error.message}`
        : 'Could not return the Firefox download to the browser.',
      'error',
    );
    await updateBrowserBadge(state);
  }
}

function basenameFromUrl(url: string): string | undefined {
  try {
    const parsed = new URL(url);
    const pathname = decodeURIComponent(parsed.pathname);
    return basenameOnly(pathname);
  } catch {
    return undefined;
  }
}

function basenameOnly(path: string | undefined): string | undefined {
  if (!path) return undefined;
  const normalized = path.replaceAll('\\', '/');
  return normalized.split('/').filter(Boolean).pop();
}

function getSyncedSettings(response: HostToExtensionResponse, fallback: ExtensionIntegrationSettings): ExtensionIntegrationSettings {
  if (!response.ok || response.type !== 'pong') {
    return fallback;
  }

  const payload = response.payload as PongPayload;
  return payload.extensionSettings ?? fallback;
}

async function getCachedExtensionSettings(): Promise<ExtensionIntegrationSettings> {
  if (cachedExtensionSettings) {
    return cachedExtensionSettings;
  }

  return rememberSettings(await getExtensionSettings());
}

function rememberSettings(settings: ExtensionIntegrationSettings): ExtensionIntegrationSettings {
  cachedExtensionSettings = settings;
  return settings;
}

function rememberStateSettings<TState extends { extensionSettings?: ExtensionIntegrationSettings }>(state: TState): TState {
  if (state.extensionSettings) {
    rememberSettings(state.extensionSettings);
  }
  return state;
}

async function updateBrowserBadge(state: PopupStateResponse) {
  const badgeApi = getBadgeApi();
  if (!badgeApi) {
    return;
  }

  if (!state.extensionSettings?.showBadgeStatus) {
    await badgeApi.setBadgeText({ text: '' });
    return;
  }

  if (state.connection !== 'connected') {
    await badgeApi.setBadgeText({ text: '!' });
    await badgeApi.setBadgeBackgroundColor({ color: '#dc2626' });
    return;
  }

  const attention = state.queueSummary?.attention ?? state.queueSummary?.failed ?? 0;
  const active = state.queueSummary?.active ?? 0;
  const text = attention > 0 ? String(attention) : active > 0 ? String(active) : '';
  await badgeApi.setBadgeText({ text });
  await badgeApi.setBadgeBackgroundColor({ color: attention > 0 ? '#d97706' : '#3b82f6' });
}

function getBadgeApi() {
  const runtimeBrowser = browser as typeof browser & {
    action?: {
      setBadgeText(details: { text: string }): Promise<void>;
      setBadgeBackgroundColor(details: { color: string }): Promise<void>;
    };
    browserAction?: {
      setBadgeText(details: { text: string }): Promise<void>;
      setBadgeBackgroundColor(details: { color: string }): Promise<void>;
    };
  };

  return runtimeBrowser.action ?? runtimeBrowser.browserAction;
}

function getFilenameInterceptionApi(): BrowserDownloadFilenameInterceptionApi<browser.downloads.DownloadItem> | null {
  const downloads = browser.downloads as BrowserDownloadFilenameInterceptionCandidate<browser.downloads.DownloadItem>;
  const rawChrome = globalThis as typeof globalThis & {
    chrome?: {
      downloads?: BrowserDownloadFilenameInterceptionCandidate<browser.downloads.DownloadItem>;
    };
  };

  return selectFilenameInterceptionApi(downloads, rawChrome.chrome?.downloads);
}

function registerFirefoxWebRequestInterception(): void {
  const webRequest = getFirefoxWebRequestApi();
  if (!webRequest) {
    return;
  }

  webRequest.onHeadersReceived.addListener(
    handleFirefoxWebRequestHeadersReceived,
    {
      urls: ['http://*/*', 'https://*/*'],
      types: ['main_frame', 'sub_frame'],
    },
    ['blocking', 'responseHeaders'],
  );
}

function registerHandoffAuthHeaderCapture(): void {
  const webRequest = getWebRequestApi();
  const headerEvent = webRequest?.onSendHeaders ?? webRequest?.onBeforeSendHeaders;
  if (!headerEvent) {
    return;
  }

  const listener = (details: HandoffAuthRequestDetails): void => {
    captureHandoffAuthHeaders(details);
  };
  const filter = {
    urls: ['http://*/*', 'https://*/*'],
    types: ['main_frame', 'sub_frame', 'xmlhttprequest', 'other'],
  };

  try {
    headerEvent.addListener(listener, filter, ['requestHeaders', 'extraHeaders']);
  } catch {
    headerEvent.addListener(listener, filter, ['requestHeaders']);
  }
}

async function handleFirefoxWebRequestHeadersReceived(
  details: FirefoxWebRequestDownloadDetails,
): Promise<{ cancel?: boolean }> {
  try {
    if (shouldBypassBrowserDownloadUrl(details.url, browserDownloadFallbackBypass)) {
      return {};
    }

    const settings = await getCachedExtensionSettings();
    const candidate = firefoxWebRequestDownloadCandidate(details, settings);
    if (!candidate) {
      return {};
    }

    void handleFirefoxWebRequestDownload(candidate, settings);
    return { cancel: true };
  } catch {
    return {};
  }
}

function markFirefoxWebRequestBypass(url: string): () => void {
  let released = false;
  markBrowserDownloadBypassUrl(browserDownloadFallbackBypass, url);

  const timeout = globalThis.setTimeout(() => {
    release();
  }, FIREFOX_FALLBACK_BYPASS_TTL_MS);

  function release() {
    if (released) {
      return;
    }

    released = true;
    globalThis.clearTimeout(timeout);
    revokeBrowserDownloadBypassUrl(browserDownloadFallbackBypass, url);
  }

  return release;
}

function getFirefoxWebRequestApi(): FirefoxWebRequestApi | null {
  const runtimeBrowser = browser as typeof browser & {
    webRequest?: FirefoxWebRequestApi;
  };

  return runtimeBrowser.webRequest?.onHeadersReceived ? runtimeBrowser.webRequest : null;
}

function getWebRequestApi(): BrowserWebRequestApi | null {
  const runtimeBrowser = browser as typeof browser & {
    webRequest?: BrowserWebRequestApi;
  };

  return runtimeBrowser.webRequest ?? null;
}

function suggestBrowserDownload(item: browser.downloads.DownloadItem, suggest: BrowserDownloadFilenameSuggest) {
  const filename = basenameOnly(item.filename) ?? basenameFromUrl(item.url);
  if (filename) {
    suggest({ filename, conflictAction: 'uniquify' } satisfies BrowserDownloadFilenameSuggestion);
    return;
  }

  suggest();
}

async function openOptionsPage() {
  const runtimeBrowser = browser as typeof browser & {
    runtime: typeof browser.runtime & {
      openOptionsPage?: () => Promise<void>;
      getURL: (path: string) => string;
    };
    tabs?: {
      create(details: { url: string }): Promise<unknown>;
    };
  };

  if (runtimeBrowser.runtime.openOptionsPage) {
    await runtimeBrowser.runtime.openOptionsPage();
    return;
  }

  await runtimeBrowser.tabs?.create({ url: runtimeBrowser.runtime.getURL('options.html') });
}

type BrowserDownloadHandoffItem = {
  requestId?: string;
  filename?: string;
  totalBytes?: number;
  incognito?: boolean;
};

type FirefoxWebRequestApi = {
  onHeadersReceived: {
    addListener(
      listener: (details: FirefoxWebRequestDownloadDetails) => Promise<{ cancel?: boolean }> | { cancel?: boolean },
      filter: { urls: string[]; types: string[] },
      extraInfoSpec: string[],
    ): void;
  };
};

type BrowserWebRequestApi = FirefoxWebRequestApi & {
  onSendHeaders?: {
    addListener(
      listener: (details: HandoffAuthRequestDetails) => void,
      filter: { urls: string[]; types?: string[] },
      extraInfoSpec?: string[],
    ): void;
  };
  onBeforeSendHeaders?: {
    addListener(
      listener: (details: HandoffAuthRequestDetails) => void,
      filter: { urls: string[]; types?: string[] },
      extraInfoSpec?: string[],
    ): void;
  };
};
