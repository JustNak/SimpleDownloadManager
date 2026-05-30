import type { ExtensionIntegrationSettings } from '@myapp/protocol';
import browser from 'webextension-polyfill';
import type { PopupRequest, PopupStateResponse } from '../shared/messages';
import { createDefaultExtensionSettings } from '../shared/defaultExtensionSettings';
import { applyExtensionAppearance } from '../shared/appearance';

const connectionStatusDot = document.querySelector<HTMLSpanElement>('#connection-status');
const connectionStatusLabel = document.querySelector<HTMLSpanElement>('#connection-status-label');
const syncButton = document.querySelector<HTMLButtonElement>('#sync-button');
const silentDownloadToggle = document.querySelector<HTMLInputElement>('#silent-download-toggle');
const extensionEnabledToggle = document.querySelector<HTMLInputElement>('#extension-enabled-toggle');
const advancedButton = document.querySelector<HTMLButtonElement>('#advanced-button');

let currentState: PopupStateResponse | null = null;
let isUpdating = false;

async function sendMessage<T>(message: PopupRequest): Promise<T> {
  return browser.runtime.sendMessage(message) as Promise<T>;
}

function renderState(state: PopupStateResponse) {
  currentState = state;
  const settings = state.extensionSettings;

  if (state.connection === 'connected' && state.appearanceSettings) {
    applyExtensionAppearance(state.appearanceSettings);
  }
  updateConnectionStatus(state.connection);

  if (silentDownloadToggle) {
    silentDownloadToggle.checked = settings?.downloadHandoffMode === 'auto';
    silentDownloadToggle.disabled = isUpdating || settings?.enabled === false;
  }

  if (extensionEnabledToggle) {
    const isEnabled = settings?.enabled !== false;
    extensionEnabledToggle.checked = isEnabled;
    extensionEnabledToggle.disabled = isUpdating;
  }

  if (advancedButton) {
    advancedButton.disabled = isUpdating;
  }

  if (syncButton) {
    syncButton.disabled = isUpdating;
  }
}

function updateConnectionStatus(connection: PopupStateResponse['connection']) {
  if (!connectionStatusDot) return;

  switch (connection) {
    case 'connected':
      connectionStatusDot.className = 'connection-dot connected';
      if (connectionStatusLabel) connectionStatusLabel.textContent = 'Connected';
      break;
    case 'checking':
      connectionStatusDot.className = 'connection-dot checking';
      if (connectionStatusLabel) connectionStatusLabel.textContent = 'Checking connection';
      break;
    case 'host_missing':
    case 'app_missing':
    case 'app_unreachable':
    case 'error':
      connectionStatusDot.className = 'connection-dot disconnected';
      if (connectionStatusLabel) connectionStatusLabel.textContent = 'Disconnected';
      break;
    default:
      connectionStatusDot.className = 'connection-dot checking';
      if (connectionStatusLabel) connectionStatusLabel.textContent = 'Checking connection';
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

extensionEnabledToggle?.addEventListener('change', () => {
  void updateSettings({ enabled: extensionEnabledToggle.checked });
});

syncButton?.addEventListener('click', () => {
  void refreshState();
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

async function refreshState() {
  isUpdating = true;
  if (currentState) renderState(currentState);

  try {
    const cachedState = await sendMessage<PopupStateResponse>({ type: 'popup_get_state' });
    renderState(cachedState);
    await sendMessage({ type: 'popup_ping' });
    const state = await sendMessage<PopupStateResponse>({ type: 'popup_get_state' });
    renderState(state);
  } catch (error) {
    renderTransientError(error, 'Could not sync extension status.');
  } finally {
    isUpdating = false;
    if (currentState) renderState(currentState);
  }
}

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
    await refreshState();
  } catch {
    renderState({
      connection: 'error',
      isSubmitting: false,
      extensionSettings: createDefaultExtensionSettings(),
    });
  }
}

void init();
