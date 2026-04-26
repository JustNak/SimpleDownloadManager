import type {
  ErrorCode,
  ExtensionIntegrationSettings,
  HostToExtensionResponse,
  PongPayload,
} from '@myapp/protocol';
import browser from './browser';
import type { PopupStateResponse } from '../shared/messages';
import {
  createDefaultExtensionSettings,
  defaultExtensionSettings,
  normalizeExtensionSettings,
} from '../shared/defaultExtensionSettings';

const STATE_KEY = 'popup-state';
const EXTENSION_SETTINGS_KEY = 'extension-settings';

const defaultState: PopupStateResponse = {
  connection: 'checking',
  isSubmitting: false,
  extensionSettings: createDefaultExtensionSettings(),
};

type PartialState = Partial<PopupStateResponse>;

export async function getPopupState(): Promise<PopupStateResponse> {
  const stored = await browser.storage.local.get(STATE_KEY);
  return { ...defaultState, ...(stored[STATE_KEY] as PartialState | undefined) };
}

export async function updatePopupState(update: PartialState): Promise<PopupStateResponse> {
  const nextState = { ...(await getPopupState()), ...update };
  await browser.storage.local.set({ [STATE_KEY]: nextState });
  return nextState;
}

export async function getExtensionSettings(): Promise<ExtensionIntegrationSettings> {
  const stored = await browser.storage.local.get(EXTENSION_SETTINGS_KEY);
  return normalizeExtensionSettings(stored[EXTENSION_SETTINGS_KEY] as Partial<ExtensionIntegrationSettings> | undefined);
}

export async function setExtensionSettings(settings: ExtensionIntegrationSettings): Promise<ExtensionIntegrationSettings> {
  const normalized = normalizeExtensionSettings(settings);
  await browser.storage.local.set({ [EXTENSION_SETTINGS_KEY]: normalized });
  await updatePopupState({ extensionSettings: normalized });
  return normalized;
}

export async function setHostError(code: ErrorCode, message: string, connection: PopupStateResponse['connection']) {
  const extensionSettings = await getExtensionSettings();
  return updatePopupState({
    connection,
    isSubmitting: false,
    extensionSettings,
    lastResult: undefined,
    lastError: { code, message },
  });
}

export async function setLastResult(connection: PopupStateResponse['connection'], response: HostToExtensionResponse) {
  const currentState = await getPopupState();
  const payload = response.ok && response.type === 'pong' ? response.payload as PongPayload : undefined;
  const extensionSettings = payload?.extensionSettings
    ? await setExtensionSettings(payload.extensionSettings)
    : await getExtensionSettings();

  return updatePopupState({
    connection: payload?.connectionState ?? connection,
    isSubmitting: false,
    queueSummary: payload?.queueSummary ?? currentState.queueSummary,
    extensionSettings,
    lastResult: response,
    lastError: response.ok ? undefined : { code: response.code, message: response.message },
  });
}

export { defaultExtensionSettings };
