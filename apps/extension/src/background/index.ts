import { isErrorResponse, toUserFacingMessage, type ExtensionIntegrationSettings, type HostToExtensionResponse, type PongPayload } from '@myapp/protocol';
import browser from './browser';
import { buildContextMenuPayload, connectionForErrorCode, enqueueDownload, openApp, pingNativeHost, promptDownload, saveExtensionSettings } from './nativeMessaging';
import { getExtensionSettings, getPopupState, setExtensionSettings, setHostError, setLastResult, updatePopupState } from './state';
import type { PopupRequest, PopupStateResponse } from '../shared/messages';

const CONTEXT_MENU_ID = 'download-with-myapp';
const interceptedBrowserDownloadIds = new Set<number>();

async function ensureContextMenu() {
  const settings = await getExtensionSettings();
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

  const state = await setLastResult('connected', response);
  await ensureContextMenu();
  await updateBrowserBadge(state);
  return response;
}

async function handleContextMenuClick(info: browser.contextMenus.OnClickData, tab?: browser.tabs.Tab) {
  const settings = await getExtensionSettings();
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

  const state = await setLastResult('connected', response);
  await updateBrowserBadge(state);
}

async function handleBrowserDownloadCreated(item: browser.downloads.DownloadItem) {
  if (interceptedBrowserDownloadIds.has(item.id) || !isHttpUrl(item.url)) {
    return;
  }

  let settings = await getExtensionSettings();
  if (!shouldHandleBrowserDownload(item.url, settings, item.filename)) {
    return;
  }

  const pingResponse = await pingNativeHost();
  if (isErrorResponse(pingResponse)) {
    const connection = connectionForErrorCode(pingResponse.code);
    const state = await setHostError(pingResponse.code, toUserFacingMessage(pingResponse.code, pingResponse.message), connection);
    await updateBrowserBadge(state);
    return;
  }

  await setLastResult('connected', pingResponse);
  settings = getSyncedSettings(pingResponse, settings);
  if (!shouldHandleBrowserDownload(item.url, settings, item.filename)) {
    return;
  }

  interceptedBrowserDownloadIds.add(item.id);
  let paused = false;

  try {
    await browser.downloads.pause(item.id);
    paused = true;

    const response = settings.downloadHandoffMode === 'auto'
      ? await enqueueDownload(item.url, {
          entryPoint: 'browser_download',
          extensionVersion: browser.runtime.getManifest().version,
          incognito: item.incognito,
        })
      : await promptDownload(item.url, {
          entryPoint: 'browser_download',
          extensionVersion: browser.runtime.getManifest().version,
          incognito: item.incognito,
        }, {
          suggestedFilename: basenameOnly(item.filename),
          totalBytes: item.totalBytes > 0 ? item.totalBytes : undefined,
        });

    if (isErrorResponse(response)) {
      if (paused) {
        await browser.downloads.resume(item.id).catch(() => undefined);
      }
      const connection = connectionForErrorCode(response.code);
      const state = await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
      await updateBrowserBadge(state);
      return;
    }

    if (isCanceledHandoff(response)) {
      if (paused) {
        await browser.downloads.resume(item.id).catch(() => undefined);
      }
      const state = await setLastResult('connected', response);
      await updateBrowserBadge(state);
      return;
    }

    await browser.downloads.cancel(item.id).catch(() => undefined);
    const state = await setLastResult('connected', response);
    await updateBrowserBadge(state);
  } catch (error) {
    if (paused) {
      await browser.downloads.resume(item.id).catch(() => undefined);
    }
    const state = await setHostError(
      'HOST_NOT_AVAILABLE',
      error instanceof Error ? error.message : 'Could not hand the browser download to the desktop app.',
      'error',
    );
    await updateBrowserBadge(state);
  } finally {
    interceptedBrowserDownloadIds.delete(item.id);
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

browser.downloads?.onCreated.addListener((item) => {
  void handleBrowserDownloadCreated(item);
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
      const cachedSettings = await setExtensionSettings(message.settings);
      await ensureContextMenu();
      const response = await saveExtensionSettings(cachedSettings);
      if (isErrorResponse(response)) {
        const connection = connectionForErrorCode(response.code);
        const state = await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
        await updateBrowserBadge(state);
        return state;
      }

      const state = await setLastResult('connected', response);
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

      const state = await setLastResult('connected', response);
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

      const state = await setLastResult('connected', response);
      await updateBrowserBadge(state);
      return response;
    }
    default:
      return getPopupState();
  }
});

void refreshConnectionState();

function shouldHandleBrowserDownload(
  url: string,
  settings: ExtensionIntegrationSettings,
  filename?: string,
): boolean {
  return settings.enabled
    && settings.downloadHandoffMode !== 'off'
    && !isHostExcluded(url, settings.excludedHosts)
    && !isFileExtensionIgnored(url, filename, settings.ignoredFileExtensions);
}

function isHostExcluded(url: string, excludedHosts: string[]): boolean {
  const hostname = new URL(url).hostname.toLowerCase();
  return excludedHosts.some((host) => hostname === host || hostname.endsWith(`.${host}`));
}

function isHttpUrl(url: string | undefined): url is string {
  if (!url) return false;
  try {
    const parsed = new URL(url);
    return parsed.protocol === 'http:' || parsed.protocol === 'https:';
  } catch {
    return false;
  }
}

function isFileExtensionIgnored(url: string, filename: string | undefined, ignoredExtensions: string[] = []): boolean {
  const extensions = ignoredExtensions.map(normalizeFileExtension).filter(Boolean);
  if (extensions.length === 0) {
    return false;
  }

  const candidates = [basenameOnly(filename), basenameFromUrl(url)]
    .filter((candidate): candidate is string => Boolean(candidate))
    .map((candidate) => candidate.toLowerCase());

  return candidates.some((candidate) => extensions.some((extension) => candidate.endsWith(`.${extension}`)));
}

function normalizeFileExtension(value: string): string {
  return value.trim().replace(/^\.+/, '').toLowerCase();
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

function isCanceledHandoff(response: HostToExtensionResponse): boolean {
  return response.ok && response.type === 'accepted' && response.payload.status === 'canceled';
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
