import { isErrorResponse, toUserFacingMessage } from '@myapp/protocol';
import browser from './browser';
import { buildContextMenuPayload, connectionForErrorCode, enqueueDownload, openApp, pingNativeHost } from './nativeMessaging';
import { getPopupState, setHostError, setLastResult, updatePopupState } from './state';
import type { PopupRequest } from '../shared/messages';

const CONTEXT_MENU_ID = 'download-with-myapp';

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
