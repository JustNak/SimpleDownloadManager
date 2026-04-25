import type { DownloadHandoffMode, ExtensionIntegrationSettings } from '@myapp/protocol';
import browser from 'webextension-polyfill';
import type { PopupRequest, PopupStateResponse } from '../shared/messages';

const statusBadge = document.querySelector<HTMLSpanElement>('#connection-status');
const enabledToggle = document.querySelector<HTMLInputElement>('#enabled-toggle');
const handoffMode = document.querySelector<HTMLSelectElement>('#handoff-mode');
const contextMenuToggle = document.querySelector<HTMLInputElement>('#context-menu-toggle');
const progressToggle = document.querySelector<HTMLInputElement>('#progress-toggle');
const badgeToggle = document.querySelector<HTMLInputElement>('#badge-toggle');
const ignoredExtensionInput = document.querySelector<HTMLInputElement>('#ignored-extension-input');
const addExtensionButton = document.querySelector<HTMLButtonElement>('#add-extension-button');
const ignoredExtensions = document.querySelector<HTMLDivElement>('#ignored-extensions');
const excludedHostInput = document.querySelector<HTMLInputElement>('#excluded-host-input');
const addHostButton = document.querySelector<HTMLButtonElement>('#add-host-button');
const excludedHosts = document.querySelector<HTMLDivElement>('#excluded-hosts');
const saveState = document.querySelector<HTMLDivElement>('#save-state');
const activeCount = document.querySelector<HTMLDivElement>('#active-count');
const queuedCount = document.querySelector<HTMLDivElement>('#queued-count');
const attentionCount = document.querySelector<HTMLDivElement>('#attention-count');
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
  renderSummary(state);

  if (!settings) return;

  if (enabledToggle) enabledToggle.checked = settings.enabled;
  if (handoffMode) handoffMode.value = settings.downloadHandoffMode;
  if (contextMenuToggle) contextMenuToggle.checked = settings.contextMenuEnabled;
  if (progressToggle) progressToggle.checked = settings.showProgressAfterHandoff;
  if (badgeToggle) badgeToggle.checked = settings.showBadgeStatus;
  renderIgnoredExtensions(settings.ignoredFileExtensions ?? []);
  renderExcludedHosts(settings.excludedHosts ?? []);
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

function renderSummary(state: PopupStateResponse) {
  const summary = state.queueSummary;
  if (activeCount) activeCount.textContent = String(summary?.active ?? 0);
  if (queuedCount) queuedCount.textContent = String(summary?.queued ?? 0);
  if (attentionCount) attentionCount.textContent = String(summary?.attention ?? summary?.failed ?? 0);
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
    contextMenuToggle,
    progressToggle,
    badgeToggle,
    ignoredExtensionInput,
    addExtensionButton,
    excludedHostInput,
    addHostButton,
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

  const state = await sendMessage<PopupStateResponse>({ type: 'extension_settings_update', settings });
  isSaving = false;
  renderState(state);
}

enabledToggle?.addEventListener('change', () => {
  void updateSettings({ enabled: enabledToggle.checked });
});

handoffMode?.addEventListener('change', () => {
  void updateSettings({ downloadHandoffMode: handoffMode.value as DownloadHandoffMode });
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

refreshButton?.addEventListener('click', async () => {
  await refreshState();
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
  return value
    .trim()
    .replace(/^https?:\/\//i, '')
    .replace(/\/.*$/, '')
    .toLowerCase();
}

async function refreshState() {
  await sendMessage({ type: 'popup_ping' }).catch(() => undefined);
  const state = await sendMessage<PopupStateResponse>({ type: 'popup_get_state' });
  renderState(state);
}

async function init() {
  await refreshState();
}

void init();
