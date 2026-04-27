import {
  normalizeExcludedHostPattern,
  type DownloadHandoffMode,
  type ExtensionIntegrationSettings,
} from '@myapp/protocol';
import browser from 'webextension-polyfill';
import type { PopupRequest, PopupStateResponse } from '../shared/messages';
import { createDefaultExtensionSettings } from '../shared/defaultExtensionSettings';

const statusBadge = document.querySelector<HTMLSpanElement>('#connection-status');
const enabledToggle = document.querySelector<HTMLInputElement>('#enabled-toggle');
const handoffMode = document.querySelector<HTMLSelectElement>('#handoff-mode');
const listenPortInput = document.querySelector<HTMLInputElement>('#listen-port-input');
const contextMenuToggle = document.querySelector<HTMLInputElement>('#context-menu-toggle');
const progressToggle = document.querySelector<HTMLInputElement>('#progress-toggle');
const badgeToggle = document.querySelector<HTMLInputElement>('#badge-toggle');
const ignoredExtensionInput = document.querySelector<HTMLInputElement>('#ignored-extension-input');
const addExtensionButton = document.querySelector<HTMLButtonElement>('#add-extension-button');
const ignoredExtensions = document.querySelector<HTMLDivElement>('#ignored-extensions');
const excludedHostInput = document.querySelector<HTMLInputElement>('#excluded-host-input');
const addHostButton = document.querySelector<HTMLButtonElement>('#add-host-button');
const excludedHosts = document.querySelector<HTMLDivElement>('#excluded-hosts');
const authHandoffToggle = document.querySelector<HTMLInputElement>('#auth-handoff-toggle');
const authHostInput = document.querySelector<HTMLInputElement>('#auth-host-input');
const addAuthHostButton = document.querySelector<HTMLButtonElement>('#add-auth-host-button');
const authHosts = document.querySelector<HTMLDivElement>('#auth-hosts');
const saveState = document.querySelector<HTMLDivElement>('#save-state');
const refreshButton = document.querySelector<HTMLButtonElement>('#refresh-button');

let currentState: PopupStateResponse | null = null;
let isSaving = false;

async function sendMessage<T>(message: PopupRequest): Promise<T> {
  return browser.runtime.sendMessage(message) as Promise<T>;
}

function renderState(state: PopupStateResponse) {
  currentState = state;
  const settings = state.extensionSettings;
  updateConnectionStatus(state.connection);

  if (!settings) return;

  if (enabledToggle) enabledToggle.checked = settings.enabled;
  if (handoffMode) handoffMode.value = settings.downloadHandoffMode;
  if (listenPortInput) listenPortInput.value = String(settings.listenPort ?? 1420);
  if (contextMenuToggle) contextMenuToggle.checked = settings.contextMenuEnabled;
  if (progressToggle) progressToggle.checked = settings.showProgressAfterHandoff;
  if (badgeToggle) badgeToggle.checked = settings.showBadgeStatus;
  if (authHandoffToggle) authHandoffToggle.checked = settings.authenticatedHandoffEnabled;
  renderIgnoredExtensions(settings.ignoredFileExtensions ?? []);
  renderExcludedHosts(settings.excludedHosts ?? []);
  renderAuthHosts(settings.authenticatedHandoffHosts ?? []);
  setControlsDisabled(isSaving, settings.enabled);

  if (saveState && !isSaving) {
    saveState.textContent = state.lastError
      ? `Saved locally. ${state.lastError.message}`
      : 'Settings are up to date.';
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

function renderIgnoredExtensions(extensions: string[]) {
  if (!ignoredExtensions) return;
  ignoredExtensions.textContent = '';

  if (extensions.length === 0) {
    ignoredExtensions.append(emptyState('No ignored file extensions.'));
    return;
  }

  for (const extension of extensions) {
    ignoredExtensions.append(
      removableChip(`.${extension}`, `Remove .${extension}`, () => {
        const nextExtensions = currentState?.extensionSettings?.ignoredFileExtensions.filter(
          (candidate) => candidate !== extension,
        ) ?? [];
        void updateSettings({ ignoredFileExtensions: nextExtensions });
      }),
    );
  }
}

function renderExcludedHosts(hosts: string[]) {
  if (!excludedHosts) return;
  excludedHosts.textContent = '';

  if (hosts.length === 0) {
    excludedHosts.append(emptyState('No excluded sites.'));
    return;
  }

  for (const host of hosts) {
    excludedHosts.append(
      removableChip(host, `Remove ${host}`, () => {
        const nextHosts = currentState?.extensionSettings?.excludedHosts.filter((candidate) => candidate !== host) ?? [];
        void updateSettings({ excludedHosts: nextHosts });
      }),
    );
  }
}

function renderAuthHosts(hosts: string[]) {
  if (!authHosts) return;
  authHosts.textContent = '';

  if (hosts.length === 0) {
    authHosts.append(emptyState('No authenticated hosts.'));
    return;
  }

  for (const host of hosts) {
    authHosts.append(
      removableChip(host, `Remove ${host}`, () => {
        const nextHosts = currentState?.extensionSettings?.authenticatedHandoffHosts.filter((candidate) => candidate !== host) ?? [];
        void updateSettings({ authenticatedHandoffHosts: nextHosts });
      }),
    );
  }
}

function removableChip(label: string, removeLabel: string, onRemove: () => void): HTMLSpanElement {
  const chip = document.createElement('span');
  chip.className = 'chip';
  chip.textContent = label;

  const removeButton = document.createElement('button');
  removeButton.type = 'button';
  removeButton.textContent = 'x';
  removeButton.title = removeLabel;
  removeButton.setAttribute('aria-label', removeLabel);
  removeButton.addEventListener('click', onRemove);

  chip.append(removeButton);
  return chip;
}

function emptyState(text: string): HTMLDivElement {
  const empty = document.createElement('div');
  empty.className = 'empty';
  empty.textContent = text;
  return empty;
}

function setControlsDisabled(saving: boolean, extensionEnabled: boolean) {
  if (enabledToggle) enabledToggle.disabled = saving;
  if (refreshButton) refreshButton.disabled = saving;

  const extensionControls = [
    handoffMode,
    listenPortInput,
    contextMenuToggle,
    progressToggle,
    badgeToggle,
    ignoredExtensionInput,
    addExtensionButton,
    excludedHostInput,
    addHostButton,
    authHandoffToggle,
    authHostInput,
    addAuthHostButton,
  ];

  for (const control of extensionControls) {
    if (control) control.disabled = saving || !extensionEnabled;
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

  isSaving = true;
  setControlsDisabled(true, settings.enabled);
  if (saveState) saveState.textContent = 'Saving settings...';

  try {
    const state = await sendMessage<PopupStateResponse>({ type: 'extension_settings_update', settings });
    renderState(state);
  } catch (error) {
    renderTransientError(error, 'Could not update extension settings.');
  } finally {
    isSaving = false;
    if (currentState) renderState(currentState);
  }
}

enabledToggle?.addEventListener('change', () => {
  void updateSettings({ enabled: enabledToggle.checked });
});

handoffMode?.addEventListener('change', () => {
  void updateSettings({ downloadHandoffMode: handoffMode.value as DownloadHandoffMode });
});

listenPortInput?.addEventListener('change', () => {
  void updateSettings({ listenPort: normalizeListenPort(listenPortInput.value) });
});

listenPortInput?.addEventListener('keydown', (event) => {
  if (event.key === 'Enter') {
    event.preventDefault();
    listenPortInput.blur();
  }
});

contextMenuToggle?.addEventListener('change', () => {
  void updateSettings({ contextMenuEnabled: contextMenuToggle.checked });
});

progressToggle?.addEventListener('change', () => {
  void updateSettings({ showProgressAfterHandoff: progressToggle.checked });
});

badgeToggle?.addEventListener('change', () => {
  void updateSettings({ showBadgeStatus: badgeToggle.checked });
});

authHandoffToggle?.addEventListener('change', () => {
  void updateSettings({ authenticatedHandoffEnabled: authHandoffToggle.checked });
});

addExtensionButton?.addEventListener('click', () => {
  addIgnoredExtensions();
});

ignoredExtensionInput?.addEventListener('keydown', (event) => {
  if (event.key === 'Enter') {
    event.preventDefault();
    addIgnoredExtensions();
  }
});

addHostButton?.addEventListener('click', () => {
  addExcludedHost();
});

excludedHostInput?.addEventListener('keydown', (event) => {
  if (event.key === 'Enter') {
    event.preventDefault();
    addExcludedHost();
  }
});

addAuthHostButton?.addEventListener('click', () => {
  addAuthHost();
});

authHostInput?.addEventListener('keydown', (event) => {
  if (event.key === 'Enter') {
    event.preventDefault();
    addAuthHost();
  }
});

refreshButton?.addEventListener('click', () => {
  void refreshState();
});

function addIgnoredExtensions() {
  const extensionsToAdd = parseFileExtensions(ignoredExtensionInput?.value ?? '');
  if (extensionsToAdd.length === 0) return;

  const extensions = currentState?.extensionSettings?.ignoredFileExtensions ?? [];
  const nextExtensions = [...extensions];
  for (const extension of extensionsToAdd) {
    if (!nextExtensions.includes(extension)) {
      nextExtensions.push(extension);
    }
  }

  if (ignoredExtensionInput) ignoredExtensionInput.value = '';
  void updateSettings({ ignoredFileExtensions: nextExtensions });
}

function addExcludedHost() {
  const host = normalizeHost(excludedHostInput?.value ?? '');
  if (!host) return;

  const hosts = currentState?.extensionSettings?.excludedHosts ?? [];
  if (hosts.includes(host)) {
    if (excludedHostInput) excludedHostInput.value = '';
    return;
  }

  if (excludedHostInput) excludedHostInput.value = '';
  void updateSettings({ excludedHosts: [...hosts, host] });
}

function addAuthHost() {
  const host = normalizeHost(authHostInput?.value ?? '');
  if (!host) return;

  const hosts = currentState?.extensionSettings?.authenticatedHandoffHosts ?? [];
  if (hosts.includes(host)) {
    if (authHostInput) authHostInput.value = '';
    return;
  }

  if (authHostInput) authHostInput.value = '';
  void updateSettings({ authenticatedHandoffHosts: [...hosts, host] });
}

function parseFileExtensions(value: string): string[] {
  return Array.from(
    new Set(
      value
        .split(/[,\s]+/)
        .map(normalizeFileExtension)
        .filter(Boolean),
    ),
  );
}

function normalizeFileExtension(value: string): string {
  const extension = value.trim().replace(/^\.+/, '').toLowerCase();
  if (!extension || extension.includes('/') || extension.includes('\\') || /^\.+$/.test(extension)) {
    return '';
  }

  return extension;
}

function normalizeHost(value: string): string {
  return normalizeExcludedHostPattern(value);
}

function normalizeListenPort(value: string): number {
  const port = Number.parseInt(value, 10);
  return Number.isFinite(port) && port >= 1 && port <= 65535 ? port : 1420;
}

async function refreshState() {
  isSaving = true;
  const extensionEnabled = currentState?.extensionSettings?.enabled ?? true;
  setControlsDisabled(true, extensionEnabled);
  if (saveState) saveState.textContent = 'Refreshing status...';

  try {
    await sendMessage({ type: 'popup_ping' }).catch(() => undefined);
    const state = await sendMessage<PopupStateResponse>({ type: 'popup_get_state' });
    renderState(state);
  } catch (error) {
    renderTransientError(error, 'Could not refresh extension status.');
  } finally {
    isSaving = false;
    if (currentState) renderState(currentState);
  }
}

async function init() {
  await refreshState();
}

void init();

function renderTransientError(error: unknown, fallback: string) {
  const message = error instanceof Error ? error.message : fallback;
  if (saveState) saveState.textContent = message;

  if (!currentState) {
    currentState = fallbackErrorState(message);
    updateConnectionStatus('error');
    return;
  }

  currentState = {
    ...currentState,
    connection: 'error',
    lastError: { code: 'HOST_NOT_AVAILABLE', message },
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
