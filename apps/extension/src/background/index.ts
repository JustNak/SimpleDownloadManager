import { isErrorResponse, toUserFacingMessage } from '@myapp/protocol';
import browser from './browser';
import { buildContextMenuPayload, connectionForErrorCode, enqueueDownload, openApp, pingNativeHost, promptDownload } from './nativeMessaging';
import { getPopupState, setHostError, setLastResult, updatePopupState } from './state';
import type { PopupRequest } from '../shared/messages';

const CONTEXT_MENU_ID = 'download-with-myapp';
const interceptedBrowserDownloadIds = new Set<number>();

async function ensureContextMenu() {
  await browser.contextMenus.removeAll();
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
    await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
    return response;
  }

  await setLastResult('connected', response);
  return response;
}

async function handleContextMenuClick(info: browser.contextMenus.OnClickData, tab?: browser.tabs.Tab) {
  const payload = buildContextMenuPayload(info, tab);
  if (!payload) {
    await setHostError('INVALID_URL', 'The selected link did not include a URL.', 'error');
    return;
  }

  await updatePopupState({ isSubmitting: true });

  const response = await enqueueDownload(payload.url, payload.source);
  if (isErrorResponse(response)) {
    const connection = connectionForErrorCode(response.code);
    await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
    return;
  }

  await setLastResult('connected', response);
}

async function handleBrowserDownloadCreated(item: browser.downloads.DownloadItem) {
  if (interceptedBrowserDownloadIds.has(item.id) || !isHttpUrl(item.url)) {
    return;
  }

  interceptedBrowserDownloadIds.add(item.id);
  let paused = false;

  try {
    await browser.downloads.pause(item.id);
    paused = true;

    const response = await promptDownload(item.url, {
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
      await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
      return;
    }

    await browser.downloads.cancel(item.id).catch(() => undefined);
    await setLastResult('connected', response);
  } catch (error) {
    if (paused) {
      await browser.downloads.resume(item.id).catch(() => undefined);
    }
    await setHostError(
      'HOST_NOT_AVAILABLE',
      error instanceof Error ? error.message : 'Could not hand the browser download to the desktop app.',
      'error',
    );
  } finally {
    interceptedBrowserDownloadIds.delete(item.id);
  }
}

browser.runtime.onInstalled.addListener(() => {
  void ensureContextMenu();
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
    case 'popup_open_app': {
      await updatePopupState({ isSubmitting: true });
      const response = await openApp();
      if (isErrorResponse(response)) {
        const connection = connectionForErrorCode(response.code);
        await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
        return response;
      }

      await setLastResult('connected', response);
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
        await setHostError(response.code, toUserFacingMessage(response.code, response.message), connection);
        return response;
      }

      await setLastResult('connected', response);
      return response;
    }
    default:
      return getPopupState();
  }
});

void ensureContextMenu();

function isHttpUrl(url: string | undefined): url is string {
  if (!url) return false;
  try {
    const parsed = new URL(url);
    return parsed.protocol === 'http:' || parsed.protocol === 'https:';
  } catch {
    return false;
  }
}

function basenameOnly(path: string | undefined): string | undefined {
  if (!path) return undefined;
  const normalized = path.replaceAll('\\', '/');
  return normalized.split('/').filter(Boolean).pop();
}
