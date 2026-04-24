import {
  HOST_NAME,
  createEnqueueDownloadRequest,
  createOpenAppRequest,
  createPingRequest,
  createPromptDownloadRequest,
  createSaveExtensionSettingsRequest,
  toUserFacingMessage,
  type BrowserKind,
  type EnqueueDownloadPayload,
  type ErrorCode,
  type ExtensionIntegrationSettings,
  type HostToExtensionResponse,
  type RequestSource,
} from '@myapp/protocol';
import browser from './browser';
import type { PopupStateResponse } from '../shared/messages';

function mapNativeMessagingError(error: unknown): { code: ErrorCode; message: string; connection: 'host_missing' | 'app_missing' | 'app_unreachable' | 'error' } {
  const message = error instanceof Error ? error.message : 'Native messaging failed.';
  const lowered = message.toLowerCase();

  if (lowered.includes('host') && lowered.includes('not found')) {
    return {
      code: 'HOST_REGISTRATION_MISSING',
      message: toUserFacingMessage('HOST_REGISTRATION_MISSING', message),
      connection: 'host_missing',
    };
  }

  if (lowered.includes('specified native messaging host not found')) {
    return {
      code: 'HOST_REGISTRATION_MISSING',
      message: toUserFacingMessage('HOST_REGISTRATION_MISSING', message),
      connection: 'host_missing',
    };
  }

  if (lowered.includes('access') && lowered.includes('denied')) {
    return {
      code: 'PERMISSION_DENIED',
      message,
      connection: 'error',
    };
  }

  return {
    code: 'HOST_NOT_AVAILABLE',
    message: toUserFacingMessage('HOST_NOT_AVAILABLE', message),
    connection: 'error',
  };
}

export function connectionForErrorCode(code: ErrorCode): PopupStateResponse['connection'] {
  switch (code) {
    case 'HOST_REGISTRATION_MISSING':
      return 'host_missing';
    case 'APP_NOT_INSTALLED':
      return 'app_missing';
    case 'APP_UNREACHABLE':
      return 'app_unreachable';
    case 'HOST_PROTOCOL_MISMATCH':
    case 'HOST_NOT_AVAILABLE':
    case 'PERMISSION_DENIED':
    case 'INTERNAL_ERROR':
      return 'error';
    default:
      return 'connected';
  }
}

async function sendNativeMessage(request: object): Promise<HostToExtensionResponse> {
  try {
    return (await browser.runtime.sendNativeMessage(HOST_NAME, request)) as HostToExtensionResponse;
  } catch (error) {
    const mapped = mapNativeMessagingError(error);
    return {
      ok: false,
      requestId: 'native_messaging_error',
      type: 'rejected',
      code: mapped.code,
      message: mapped.message,
    };
  }
}

export function detectBrowser(): BrowserKind {
  const userAgent = navigator.userAgent.toLowerCase();

  if (userAgent.includes('firefox')) {
    return 'firefox';
  }

  if (userAgent.includes('edg/')) {
    return 'edge';
  }

  return 'chrome';
}

export async function pingNativeHost(): Promise<HostToExtensionResponse> {
  return sendNativeMessage(createPingRequest());
}

export async function openApp(): Promise<HostToExtensionResponse> {
  return sendNativeMessage(createOpenAppRequest({ reason: 'user_request' }));
}

export async function enqueueDownload(url: string, source: Omit<RequestSource, 'browser'>): Promise<HostToExtensionResponse> {
  const request = createEnqueueDownloadRequest(url, { ...source, browser: detectBrowser() });
  if (!request.ok) {
    return {
      ok: false,
      requestId: 'validation_error',
      type: 'rejected',
      code: request.code,
      message: request.message,
    };
  }

  return sendNativeMessage(request.value);
}

export async function promptDownload(
  url: string,
  source: Omit<RequestSource, 'browser'>,
  metadata: { suggestedFilename?: string; totalBytes?: number } = {},
): Promise<HostToExtensionResponse> {
  const request = createPromptDownloadRequest(url, { ...source, browser: detectBrowser() }, metadata);
  if (!request.ok) {
    return {
      ok: false,
      requestId: 'validation_error',
      type: 'rejected',
      code: request.code,
      message: request.message,
    };
  }

  return sendNativeMessage(request.value);
}

export async function saveExtensionSettings(settings: ExtensionIntegrationSettings): Promise<HostToExtensionResponse> {
  return sendNativeMessage(createSaveExtensionSettingsRequest(settings));
}

export function buildContextMenuPayload(info: browser.contextMenus.OnClickData, tab?: browser.tabs.Tab): EnqueueDownloadPayload | null {
  if (!info.linkUrl) {
    return null;
  }

  return {
    url: info.linkUrl,
    source: {
      entryPoint: 'context_menu',
      browser: detectBrowser(),
      extensionVersion: browser.runtime.getManifest().version,
      pageUrl: tab?.url,
      pageTitle: tab?.title,
      referrer: tab?.url,
      incognito: tab?.incognito,
    },
  };
}
