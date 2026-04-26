import type { ExtensionIntegrationSettings } from '@myapp/protocol';
import browser from 'webextension-polyfill';
import type { PopupRequest, PopupStateResponse } from '../shared/messages';
import { createDefaultExtensionSettings } from '../shared/defaultExtensionSettings';

const statusBadge = document.querySelector<HTMLSpanElement>('#connection-status');
const silentDownloadToggle = document.querySelector<HTMLInputElement>('#silent-download-toggle');
const silentDownloadHint = document.querySelector<HTMLDivElement>('#silent-download-hint');
const extensionToggleButton = document.querySelector<HTMLButtonElement>('#extension-toggle-button');
const advancedButton = document.querySelector<HTMLButtonElement>('#advanced-button');

let currentState: PopupStateResponse | null = null;
let isUpdating = false;

async function sendMessage<T>(message: PopupRequest): Promise<T> {
  return browser.runtime.sendMessage(message) as Promise<T>;
}

function renderState(state: PopupStateResponse) {
  currentState = state;
  const settings = state.extensionSettings;

  updateConnectionStatus(state.connection);

  if (silentDownloadToggle) {
    silentDownloadToggle.checked = settings?.downloadHandoffMode === 'auto';
    silentDownloadToggle.disabled = isUpdating || settings?.enabled === false;
  }

  if (silentDownloadHint) {
    silentDownloadHint.textContent = settings?.downloadHandoffMode === 'auto'
      ? 'Send downloads without a prompt.'
      : 'Ask before sending downloads.';
  }

  if (extensionToggleButton) {
    const isEnabled = settings?.enabled !== false;
    extensionToggleButton.textContent = isEnabled ? 'Disable Extension' : 'Enable Extension';
    extensionToggleButton.className = isEnabled ? 'danger' : 'primary';
    extensionToggleButton.disabled = isUpdating;
  }

  if (advancedButton) {
    advancedButton.disabled = isUpdating;
  }
}

function updateConnectionStatus(connection: PopupStateResponse['connection']) {
  if (!statusBadge) return;

  statusBadge.className = `status ${connection}`;
  switch (connection) {
    case 'connected':
      statusBadge.textContent = 'Connected';
      break;
    case 'host_missing':
      statusBadge.textContent = 'Host Missing';
      break;
    case 'app_missing':
      statusBadge.textContent = 'App Missing';
      break;
    case 'app_unreachable':
      statusBadge.textContent = 'Unreachable';
      break;
    case 'error':
      statusBadge.textContent = 'Error';
      break;
    default:
      statusBadge.textContent = 'Checking';
      break;
  }
}

function nextSettings(update: Partial<ExtensionIntegrationSettings>): ExtensionIntegrationSettings | null {
  const settings = currentState?.extensionSettings;
  if (!settings) return null;
  return { ...settings, ...update };
}

async function updateSettings(update: Partial<ExtensionIntegrationSettings>) {
  const settings = nextSettings(update);
  if (!settings) return;

  isUpdating = true;
  if (currentState) renderState(currentState);
  try {
    const state = await sendMessage<PopupStateResponse>({ type: 'extension_settings_update', settings });
    renderState(state);
  } catch (error) {
    renderTransientError(error, 'Could not update extension settings.');
  } finally {
    isUpdating = false;
    if (currentState) renderState(currentState);
  }
}

silentDownloadToggle?.addEventListener('change', () => {
  void updateSettings({
    downloadHandoffMode: silentDownloadToggle.checked ? 'auto' : 'ask',
  });
});

extensionToggleButton?.addEventListener('click', () => {
  const isEnabled = currentState?.extensionSettings?.enabled !== false;
  void updateSettings({ enabled: !isEnabled });
});

advancedButton?.addEventListener('click', async () => {
  isUpdating = true;
  if (currentState) renderState(currentState);
  try {
    const state = await sendMessage<PopupStateResponse>({ type: 'popup_open_options' });
    renderState(state);
  } catch (error) {
    renderTransientError(error, 'Could not open advanced settings.');
  } finally {
    isUpdating = false;
    if (currentState) renderState(currentState);
  }
});

function renderTransientError(error: unknown, fallback: string) {
  const message = error instanceof Error ? error.message : fallback;
  if (!currentState) {
    currentState = fallbackErrorState(message);
    return;
  }

  currentState = {
    ...currentState,
    connection: 'error',
    lastError: {
      code: 'HOST_NOT_AVAILABLE',
      message,
    },
  };
}

function fallbackErrorState(message: string): PopupStateResponse {
  return {
    connection: 'error',
    isSubmitting: false,
    lastError: { code: 'HOST_NOT_AVAILABLE', message },
    extensionSettings: createDefaultExtensionSettings(),
  };
}

async function init() {
  try {
    await sendMessage({ type: 'popup_ping' });
    const state = await sendMessage<PopupStateResponse>({ type: 'popup_get_state' });
    renderState(state);
  } catch {
    renderState({
      connection: 'error',
      isSubmitting: false,
      extensionSettings: createDefaultExtensionSettings(),
    });
  }
}

void init();
