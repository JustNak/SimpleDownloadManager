import { isErrorResponse, toUserFacingMessage, type ExtensionIntegrationSettings, type HostToExtensionResponse, type PongPayload } from '@myapp/protocol';
import browser from './browser';
import {
  browserDownloadUrl,
  classifyBrowserDownloadIntent,
  completeBrowserDownloadAdoption,
  createAsyncFilenameInterceptionListener,
  firefoxWebRequestDownloadCandidate,
  selectFilenameInterceptionApi,
  shouldAllowBrowserDownloadBySettings,
  type BrowserDownloadFilenameInterceptionApi,
  type BrowserDownloadFilenameInterceptionCandidate,
  type BrowserDownloadFilenameSuggest,
  type BrowserDownloadFilenameSuggestion,
  type BrowserDownloadAdoption,
  type BrowserDownloadDelta,
  type BrowserDownloadIntentDecision,
  type FirefoxWebRequestDownloadCandidate,
  type FirefoxWebRequestDownloadDetails,
} from './browserDownloads';
import {
  adoptBrowserDownload,
  buildContextMenuPayload,
  connectionForErrorCode,
  enqueueDownload,
  openApp,
  pingNativeHost,
  saveExtensionSettings,
} from './nativeMessaging';
import { getExtensionSettings, getPopupState, setExtensionSettings, setHostError, setLastResult, updatePopupState } from './state';
import type { PopupRequest, PopupStateResponse } from '../shared/messages';
import { normalizeAccentColor } from '../shared/appearance';

const CONTEXT_MENU_ID = 'download-with-myapp';
const APPEARANCE_SYNC_ALARM_NAME = 'appearance-sync';
const BROWSER_ADOPTION_PENDING_TTL_MS = 5 * 60 * 1000;
const browserDownloadAdoptions = new Map<number, BrowserDownloadAdoption>();
const pendingBrowserDownloadAdoptions = new Map<string, PendingBrowserDownloadAdoption>();
let cachedExtensionSettings: ExtensionIntegrationSettings | null = null;
let cachedExtensionSettingsPromise: Promise<ExtensionIntegrationSettings> | null = null;
let nativeHostPingPromise: Promise<HostToExtensionResponse> | null = null;
let refreshConnectionStatePromise: Promise<HostToExtensionResponse> | null = null;

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
  refreshConnectionStatePromise ??= refreshConnectionStateNow().finally(() => {
    refreshConnectionStatePromise = null;
  });

  return refreshConnectionStatePromise;
}

async function refreshConnectionStateNow() {
  const response = await pingNativeHostCoalesced();
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

async function pingNativeHostCoalesced(): Promise<HostToExtensionResponse> {
  nativeHostPingPromise ??= pingNativeHost().finally(() => {
    nativeHostPingPromise = null;
  });

  return nativeHostPingPromise;
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
  const url = browserDownloadUrl(item);
  if (url && consumePendingBrowserDownloadAdoption(item, url)) {
    return;
  }

  if (shouldSkipBrowserDownloadInterception(item)) {
    return;
  }

  if (!url) {
    return;
  }

  let settings = await getCachedExtensionSettings();
  if (!shouldAllowBrowserDownloadBySettings(item, settings)) {
    return;
  }

  const decision = classifyBrowserDownloadIntent({ ...item, url }, settings.capturedFileExtensions);
  logDownloadCaptureDecision('chromium-downloads', item, settings, decision);
  if (decision.action !== 'capture') {
    return;
  }
  trackBrowserDownloadForAdoption(url, item, 'browser_download');
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
  if (!shouldAllowBrowserDownloadBySettings(item, settings)) {
    suggestBrowserDownload(item, suggest);
    return;
  }

  const decision = classifyBrowserDownloadIntent({ ...item, url }, settings.capturedFileExtensions);
  logDownloadCaptureDecision('chromium-downloads', item, settings, decision);
  if (decision.action !== 'capture') {
    suggestBrowserDownload(item, suggest);
    return;
  }
  trackBrowserDownloadForAdoption(url, item, 'browser_download');
  suggestBrowserDownload(item, suggest);
}

browser.runtime.onInstalled.addListener(() => {
  void ensureAppearanceSyncAlarm();
  void refreshConnectionState();
});

browser.runtime.onStartup.addListener(() => {
  void ensureAppearanceSyncAlarm();
  void refreshConnectionState();
});

browser.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name !== APPEARANCE_SYNC_ALARM_NAME) {
    return;
  }

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
browser.downloads?.onChanged.addListener((delta) => {
  void handleBrowserDownloadChanged(delta as BrowserDownloadDelta);
});

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

void ensureAppearanceSyncAlarm();

async function ensureAppearanceSyncAlarm(): Promise<void> {
  try {
    await browser.alarms.create(APPEARANCE_SYNC_ALARM_NAME, {
      delayInMinutes: 15,
      periodInMinutes: 15,
    });
  } catch {
    // Alarm setup failure should not block handoff or popup behavior.
  }
}

function shouldSkipBrowserDownloadInterception(item: browser.downloads.DownloadItem): boolean {
  return !browserDownloadUrl(item);
}

function browserDownloadSource(item: BrowserDownloadHandoffItem) {
  return {
    entryPoint: 'browser_download' as const,
    extensionVersion: browser.runtime.getManifest().version,
    incognito: item.incognito,
    pageUrl: item.pageUrl,
    pageTitle: item.pageTitle,
    referrer: item.referrer,
  };
}

async function recordHostError(response: Extract<HostToExtensionResponse, { ok: false }>): Promise<void> {
  const connection = connectionForErrorCode(response.code);
  const state = await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
  await updateBrowserBadge(state);
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

  cachedExtensionSettingsPromise ??= getExtensionSettings().then(rememberSettings).finally(() => {
    cachedExtensionSettingsPromise = null;
  });

  return cachedExtensionSettingsPromise;
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
  await badgeApi.setBadgeBackgroundColor({
    color: attention > 0 ? '#d97706' : normalizeAccentColor(state.appearanceSettings?.accentColor),
  });
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
      types: ['main_frame', 'sub_frame', 'xmlhttprequest', 'object', 'other'],
    },
    ['blocking', 'responseHeaders'],
  );
}

async function handleFirefoxWebRequestHeadersReceived(
  details: FirefoxWebRequestDownloadDetails,
): Promise<{ cancel?: boolean }> {
  try {
    const settings = await getCachedExtensionSettings();
    const candidate = firefoxWebRequestDownloadCandidate(details, settings);
    if (!candidate) {
      return {};
    }
    logDownloadCaptureDecision('firefox-webrequest', candidate, settings, {
      action: 'capture',
      reason: candidate.reason,
    });

    markBrowserDownloadUrlForAdoption(candidate);
    return {};
  } catch {
    return {};
  }
}

async function handleBrowserDownloadChanged(delta: BrowserDownloadDelta): Promise<void> {
  await completeBrowserDownloadAdoption(delta, browserDownloadAdoptions, {
    searchDownloads: (query) => browser.downloads.search(query),
    browserDownloadSource,
    adoptBrowserDownload,
    isErrorResponse,
    onError: (response) => recordHostError(response as Extract<HostToExtensionResponse, { ok: false }>),
    onSuccess: async (response) => {
      const state = rememberStateSettings(await setLastResult('connected', response));
      await updateBrowserBadge(state);
    },
  });
}

function trackBrowserDownloadForAdoption(
  url: string,
  item: BrowserDownloadHandoffItem & { id?: number; mime?: string },
  reason: BrowserDownloadAdoption['reason'],
): void {
  if (typeof item.id !== 'number' || item.id < 0) {
    return;
  }

  browserDownloadAdoptions.set(item.id, {
    url,
    suggestedFilename: basenameOnly(item.filename) ?? basenameFromUrl(url),
    totalBytes: normalizedDownloadSize(item.totalBytes),
    incognito: item.incognito,
    pageUrl: item.pageUrl,
    pageTitle: item.pageTitle,
    referrer: item.referrer,
    reason,
  });
}

function markBrowserDownloadUrlForAdoption(candidate: FirefoxWebRequestDownloadCandidate): void {
  prunePendingBrowserDownloadAdoptions();
  pendingBrowserDownloadAdoptions.set(adoptionUrlKey(candidate.url), {
    url: candidate.url,
    suggestedFilename: candidate.filename,
    totalBytes: normalizedDownloadSize(candidate.totalBytes),
    incognito: candidate.incognito,
    expiresAt: Date.now() + BROWSER_ADOPTION_PENDING_TTL_MS,
    reason: 'browser_session',
  });
}

function consumePendingBrowserDownloadAdoption(item: browser.downloads.DownloadItem, url: string): boolean {
  prunePendingBrowserDownloadAdoptions();
  const finalUrl = (item as browser.downloads.DownloadItem & { finalUrl?: string }).finalUrl;
  const keys = [url, finalUrl].filter((value): value is string => Boolean(value)).map(adoptionUrlKey);
  const key = keys.find((candidateKey) => pendingBrowserDownloadAdoptions.has(candidateKey));
  if (!key) {
    return false;
  }

  const pending = pendingBrowserDownloadAdoptions.get(key);
  pendingBrowserDownloadAdoptions.delete(key);
  if (!pending) {
    return false;
  }

  trackBrowserDownloadForAdoption(url, { ...item, ...pending }, pending.reason);
  return true;
}

function prunePendingBrowserDownloadAdoptions(now = Date.now()): void {
  for (const [key, pending] of pendingBrowserDownloadAdoptions) {
    if (pending.expiresAt < now) {
      pendingBrowserDownloadAdoptions.delete(key);
    }
  }
}

function adoptionUrlKey(url: string): string {
  try {
    const parsed = new URL(url);
    parsed.hash = '';
    return parsed.href;
  } catch {
    return url;
  }
}

function normalizedDownloadSize(value: number | undefined): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.floor(value) : undefined;
}

function logDownloadCaptureDecision(
  browserPath: 'chromium-downloads' | 'firefox-webrequest',
  item: { url?: string; finalUrl?: string; filename?: string; mime?: string; totalBytes?: number },
  settings: ExtensionIntegrationSettings,
  decision: BrowserDownloadIntentDecision,
): void {
  if (!settings.downloadCaptureDebugLogging) {
    return;
  }

  console.debug('download-capture-policy', {
    browserPath,
    urlHost: hostFromUrl(browserDownloadUrl(item) ?? item.url),
    basename: basenameOnly(item.filename) ?? basenameFromUrl(browserDownloadUrl(item) ?? item.url ?? ''),
    mime: item.mime,
    totalBytes: item.totalBytes,
    decision: decision.action,
    reason: decision.reason,
  });
}

function hostFromUrl(url: string | undefined): string | undefined {
  if (!url) {
    return undefined;
  }

  try {
    return new URL(url).host;
  } catch {
    return undefined;
  }
}

function getFirefoxWebRequestApi(): FirefoxWebRequestApi | null {
  const runtimeBrowser = browser as typeof browser & {
    webRequest?: FirefoxWebRequestApi;
  };

  return runtimeBrowser.webRequest?.onHeadersReceived ? runtimeBrowser.webRequest : null;
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
  filename?: string;
  totalBytes?: number;
  incognito?: boolean;
  pageUrl?: string;
  pageTitle?: string;
  referrer?: string;
};

type PendingBrowserDownloadAdoption = BrowserDownloadAdoption & {
  expiresAt: number;
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
