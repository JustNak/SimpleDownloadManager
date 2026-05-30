import { isErrorResponse, toUserFacingMessage, type DownloadRequestMetadata, type ExtensionIntegrationSettings, type HandoffAuth, type HostToExtensionResponse, type PongPayload } from '@myapp/protocol';
import browser from './browser';
import {
  browserDownloadUrl,
  browserDownloadFilenameSuggestion,
  classifyBrowserDownloadIntent,
  createAsyncFilenameInterceptionListener,
  firefoxWebRequestDownloadCandidate,
  selectFilenameInterceptionApi,
  shouldAllowBrowserDownloadBySettings,
  type BrowserDownloadFilenameInterceptionApi,
  type BrowserDownloadFilenameInterceptionCandidate,
  type BrowserDownloadFilenameSuggest,
  type BrowserDownloadIntentDecision,
  type FirefoxWebRequestDownloadCandidate,
  type FirefoxWebRequestDownloadDetails,
  type FirefoxWebRequestHeader,
} from './browserDownloads';
import {
  buildContextMenuPayload,
  connectionForErrorCode,
  detectBrowser,
  enqueueDownload,
  openApp,
  pingNativeHost,
  promptDownload,
  saveExtensionSettings,
} from './nativeMessaging';
import { getExtensionSettings, getPopupState, setExtensionSettings, setHostError, setLastResult, updatePopupState } from './state';
import type { PopupRequest, PopupStateResponse } from '../shared/messages';
import { normalizeAccentColor } from '../shared/appearance';

const CONTEXT_MENU_ID = 'download-with-myapp';
const APPEARANCE_SYNC_ALARM_NAME = 'appearance-sync';
let cachedExtensionSettings: ExtensionIntegrationSettings | null = null;
let cachedExtensionSettingsPromise: Promise<ExtensionIntegrationSettings> | null = null;
let nativeHostPingPromise: Promise<HostToExtensionResponse> | null = null;
let refreshConnectionStatePromise: Promise<HostToExtensionResponse> | null = null;
const handoffAuthByRequestId = new Map<string, BrowserHandoffAuthSnapshot>();
const handoffAuthByUrl = new Map<string, BrowserHandoffAuthSnapshot[]>();
const redirectOriginalByRequestId = new Map<string, BrowserRedirectSnapshot>();
const redirectOriginalByUrl = new Map<string, BrowserRedirectSnapshot[]>();
const HANDOFF_AUTH_SNAPSHOT_TTL_MS = 30_000;
const REDIRECT_ORIGINAL_SNAPSHOT_TTL_MS = 5 * 60 * 1000;
const MAX_HANDOFF_AUTH_SNAPSHOTS_PER_URL = 5;
const MAX_REDIRECT_ORIGINAL_SNAPSHOTS_PER_URL = 5;
const ALLOWED_HANDOFF_AUTH_HEADERS = new Set([
  'cookie',
  'authorization',
  'referer',
  'origin',
  'user-agent',
  'accept',
  'accept-language',
]);

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

  if (await cancelBrowserDownload(item)) {
    void handOffCapturedBrowserDownload(url, item, settings);
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

  if (await cancelBrowserDownload(item)) {
    void handOffCapturedBrowserDownload(url, item, settings);
  }
  suggest();
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
registerBrowserHandoffAuthHeaderCapture();
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

async function handOffCapturedBrowserDownload(
  url: string,
  item: BrowserDownloadHandoffItem,
  settings: ExtensionIntegrationSettings,
): Promise<void> {
  await updatePopupState({ isSubmitting: true });
  const handoffUrl = resolveOriginalBrowserDownloadUrl(url, item) ?? url;
  const source = browserDownloadSource(item);
  const metadata: DownloadRequestMetadata = {
    suggestedFilename: basenameOnly(item.filename) ?? basenameFromUrl(url),
    totalBytes: normalizedDownloadSize(item.totalBytes),
    handoffAuth: resolveBrowserHandoffAuth(handoffUrl, item),
  };
  const response = settings.downloadHandoffMode === 'auto'
    ? await enqueueDownload(handoffUrl, source, metadata)
    : await promptDownload(handoffUrl, source, metadata);

  if (isErrorResponse(response)) {
    await recordHostError(response);
    return;
  }

  const state = rememberStateSettings(await setLastResult('connected', response));
  await updateBrowserBadge(state);
}

async function recordHostError(
  response: Extract<HostToExtensionResponse, { ok: false }>,
): Promise<void> {
  const connection = connectionForErrorCode(response.code);
  const state = await setHostError(
    response.code,
    toUserFacingMessage(response.code, response.message),
    connection,
  );
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

    void handOffCapturedBrowserDownload(candidate.url, candidate, settings);
    return { cancel: true };
  } catch {
    return {};
  }
}

function normalizedDownloadSize(value: number | undefined): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.floor(value) : undefined;
}

function registerBrowserHandoffAuthHeaderCapture(): void {
  const webRequest = getFirefoxWebRequestApi();
  if (!webRequest) {
    return;
  }

  webRequest.onSendHeaders?.addListener(
    captureHandoffAuthHeaders,
    {
      urls: ['http://*/*', 'https://*/*'],
      types: ['main_frame', 'sub_frame', 'xmlhttprequest', 'object', 'other'],
    },
    handoffAuthHeaderExtraInfoSpec(),
  );

  webRequest.onBeforeRedirect?.addListener(
    captureBrowserDownloadRedirect,
    {
      urls: ['http://*/*', 'https://*/*'],
      types: ['main_frame', 'sub_frame', 'xmlhttprequest', 'object', 'other'],
    },
  );
}

function captureHandoffAuthHeaders(details: FirefoxWebRequestSendHeadersDetails): void {
  const url = browserDownloadUrl({ url: details.url });
  if (!url || !details.requestHeaders?.length) {
    return;
  }

  const handoffAuth = handoffAuthFromRequestHeaders(details.requestHeaders);
  if (!handoffAuth) {
    return;
  }

  const snapshot: BrowserHandoffAuthSnapshot = {
    url,
    createdAt: Date.now(),
    handoffAuth,
  };

  if (details.requestId) {
    handoffAuthByRequestId.set(details.requestId, handoffAuthByRequestId.get(details.requestId) ?? snapshot);
  }

  const snapshots = handoffAuthByUrl.get(url) ?? [];
  snapshots.unshift(snapshot);
  handoffAuthByUrl.set(url, snapshots.slice(0, MAX_HANDOFF_AUTH_SNAPSHOTS_PER_URL));
  pruneHandoffAuthSnapshots();
}

function captureBrowserDownloadRedirect(details: FirefoxWebRequestRedirectDetails): void {
  const redirectUrl = browserDownloadUrl({ url: details.redirectUrl });
  const currentUrl = browserDownloadUrl({ url: details.url });
  if (!redirectUrl || !currentUrl) {
    return;
  }

  const existing = details.requestId ? redirectOriginalByRequestId.get(details.requestId) : undefined;
  const originalUrl = existing?.originalUrl ?? currentUrl;
  if (!isPreferredOriginalDownloadUrl(originalUrl)) {
    return;
  }

  const snapshot: BrowserRedirectSnapshot = {
    originalUrl,
    redirectedUrl: redirectUrl,
    createdAt: Date.now(),
  };

  if (details.requestId) {
    redirectOriginalByRequestId.set(details.requestId, snapshot);
  }

  const snapshots = redirectOriginalByUrl.get(redirectUrl) ?? [];
  snapshots.unshift(snapshot);
  redirectOriginalByUrl.set(redirectUrl, snapshots.slice(0, MAX_REDIRECT_ORIGINAL_SNAPSHOTS_PER_URL));
  pruneRedirectOriginalSnapshots();
}

function resolveOriginalBrowserDownloadUrl(
  url: string,
  item: { requestId?: string; url?: string; finalUrl?: string },
): string | undefined {
  pruneRedirectOriginalSnapshots();

  if (item.requestId) {
    const snapshot = redirectOriginalByRequestId.get(item.requestId);
    if (snapshot && !redirectOriginalSnapshotExpired(snapshot)) {
      return snapshot.originalUrl;
    }
  }

  const candidates = [url, item.finalUrl, item.url].filter((value): value is string => Boolean(value));
  for (const candidate of candidates) {
    const snapshots = redirectOriginalByUrl.get(candidate) ?? [];
    const snapshot = snapshots.find((entry) => !redirectOriginalSnapshotExpired(entry));
    if (snapshot) {
      return snapshot.originalUrl;
    }
  }

  return undefined;
}

function resolveBrowserHandoffAuth(
  url: string,
  item: { requestId?: string; url?: string; finalUrl?: string },
): HandoffAuth | undefined {
  pruneHandoffAuthSnapshots();

  if (item.requestId) {
    const snapshot = handoffAuthByRequestId.get(item.requestId);
    if (snapshot && snapshot.url === url && !handoffAuthSnapshotExpired(snapshot)) {
      return snapshot.handoffAuth;
    }
  }

  const candidates = handoffAuthByUrl.get(url) ?? [];
  return candidates.find((snapshot) => !handoffAuthSnapshotExpired(snapshot))?.handoffAuth;
}

function handoffAuthFromRequestHeaders(headers: FirefoxWebRequestHeader[]): HandoffAuth | undefined {
  const filtered = headers
    .filter((header) => header.value && isAllowedHandoffAuthHeader(header.name))
    .map((header) => ({
      name: canonicalHandoffAuthHeaderName(header.name),
      value: header.value ?? '',
    }))
    .filter((header) => header.value);

  return filtered.length ? { headers: filtered } : undefined;
}

function isAllowedHandoffAuthHeader(name: string): boolean {
  const lower = name.trim().toLowerCase();
  return ALLOWED_HANDOFF_AUTH_HEADERS.has(lower)
    || lower.startsWith('sec-fetch-')
    || lower.startsWith('sec-ch-ua');
}

function canonicalHandoffAuthHeaderName(name: string): string {
  return name.trim().toLowerCase().replace(/(^|-)([a-z])/g, (_, prefix: string, letter: string) => (
    `${prefix}${letter.toUpperCase()}`
  ));
}

function handoffAuthHeaderExtraInfoSpec(): string[] {
  const spec = ['requestHeaders'];
  if (!detectBrowser().includes('firefox')) {
    spec.push('extraHeaders');
  }
  return spec;
}

function pruneHandoffAuthSnapshots(): void {
  const now = Date.now();
  for (const [requestId, snapshot] of handoffAuthByRequestId) {
    if (now - snapshot.createdAt > HANDOFF_AUTH_SNAPSHOT_TTL_MS) {
      handoffAuthByRequestId.delete(requestId);
    }
  }

  for (const [url, snapshots] of handoffAuthByUrl) {
    const active = snapshots.filter((snapshot) => now - snapshot.createdAt <= HANDOFF_AUTH_SNAPSHOT_TTL_MS);
    if (active.length) {
      handoffAuthByUrl.set(url, active);
    } else {
      handoffAuthByUrl.delete(url);
    }
  }
}

function handoffAuthSnapshotExpired(snapshot: BrowserHandoffAuthSnapshot): boolean {
  return Date.now() - snapshot.createdAt > HANDOFF_AUTH_SNAPSHOT_TTL_MS;
}

function pruneRedirectOriginalSnapshots(): void {
  const now = Date.now();
  for (const [requestId, snapshot] of redirectOriginalByRequestId) {
    if (now - snapshot.createdAt > REDIRECT_ORIGINAL_SNAPSHOT_TTL_MS) {
      redirectOriginalByRequestId.delete(requestId);
    }
  }

  for (const [url, snapshots] of redirectOriginalByUrl) {
    const active = snapshots.filter((snapshot) => now - snapshot.createdAt <= REDIRECT_ORIGINAL_SNAPSHOT_TTL_MS);
    if (active.length) {
      redirectOriginalByUrl.set(url, active);
    } else {
      redirectOriginalByUrl.delete(url);
    }
  }
}

function redirectOriginalSnapshotExpired(snapshot: BrowserRedirectSnapshot): boolean {
  return Date.now() - snapshot.createdAt > REDIRECT_ORIGINAL_SNAPSHOT_TTL_MS;
}

function isPreferredOriginalDownloadUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    const pathname = parsed.pathname.toLowerCase();
    return parsed.searchParams.get('download_frd') === '1'
      && /(?:^|\/)(?:courses\/\d+\/)?files\/\d+\/download\/?$/.test(pathname);
  } catch {
    return false;
  }
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
  suggest(browserDownloadFilenameSuggestion(item));
}

async function cancelBrowserDownload(item: browser.downloads.DownloadItem): Promise<boolean> {
  if (typeof item.id !== 'number' || item.id < 0) {
    await recordHostError(browserCaptureError('INVALID_PAYLOAD', 'Browser download did not expose a cancellable id.'));
    return false;
  }

  const downloads = browser.downloads as typeof browser.downloads & {
    cancel?: (downloadId: number) => Promise<void>;
    removeFile?: (downloadId: number) => Promise<void>;
    erase?: (query: { id: number }) => Promise<unknown>;
  };

  if (!downloads.cancel) {
    await recordHostError(browserCaptureError('INTERNAL_ERROR', 'Browser download cancellation is not available.'));
    return false;
  }

  try {
    await downloads.cancel(item.id);
    await downloads.removeFile?.(item.id).catch(() => undefined);
    await downloads.erase?.({ id: item.id }).catch(() => undefined);
    return true;
  } catch (error) {
    const message = error instanceof Error ? error.message : 'Browser download cancellation failed.';
    await recordHostError(browserCaptureError('INTERNAL_ERROR', message));
    return false;
  }
}

function browserCaptureError(
  code: Extract<HostToExtensionResponse, { ok: false }>['code'],
  message: string,
): Extract<HostToExtensionResponse, { ok: false }> {
  return {
    ok: false,
    requestId: 'browser_capture_error',
    type: 'rejected',
    code,
    message,
  };
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
  requestId?: string;
  url?: string;
  finalUrl?: string;
};

type BrowserHandoffAuthSnapshot = {
  url: string;
  createdAt: number;
  handoffAuth: HandoffAuth;
};

type BrowserRedirectSnapshot = {
  originalUrl: string;
  redirectedUrl: string;
  createdAt: number;
};

type FirefoxWebRequestApi = {
  onSendHeaders?: {
    addListener(
      listener: (details: FirefoxWebRequestSendHeadersDetails) => void,
      filter: { urls: string[]; types: string[] },
      extraInfoSpec: string[],
    ): void;
  };
  onBeforeRedirect?: {
    addListener(
      listener: (details: FirefoxWebRequestRedirectDetails) => void,
      filter: { urls: string[]; types: string[] },
    ): void;
  };
  onHeadersReceived: {
    addListener(
      listener: (details: FirefoxWebRequestDownloadDetails) => Promise<{ cancel?: boolean }> | { cancel?: boolean },
      filter: { urls: string[]; types: string[] },
      extraInfoSpec: string[],
    ): void;
  };
};

type FirefoxWebRequestSendHeadersDetails = {
  requestId?: string;
  url: string;
  requestHeaders?: FirefoxWebRequestHeader[];
};

type FirefoxWebRequestRedirectDetails = {
  requestId?: string;
  url: string;
  redirectUrl: string;
};
