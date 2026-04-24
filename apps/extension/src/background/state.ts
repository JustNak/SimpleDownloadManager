import type { ErrorCode, HostToExtensionResponse } from '@myapp/protocol';
import browser from './browser';
import type { PopupStateResponse } from '../shared/messages';

const STATE_KEY = 'popup-state';

const defaultState: PopupStateResponse = {
  connection: 'checking',
  isSubmitting: false,
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

export async function setHostError(code: ErrorCode, message: string, connection: PopupStateResponse['connection']) {
  return updatePopupState({
    connection,
    isSubmitting: false,
    lastResult: undefined,
    lastError: { code, message },
  });
}

export async function setLastResult(connection: PopupStateResponse['connection'], response: HostToExtensionResponse) {
  return updatePopupState({
    connection,
    isSubmitting: false,
    lastResult: response,
    lastError: response.ok ? undefined : { code: response.code, message: response.message },
  });
}
