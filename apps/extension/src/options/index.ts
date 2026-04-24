import type { DownloadHandoffMode, ExtensionIntegrationSettings } from '@myapp/protocol';
import browser from 'webextension-polyfill';
import type { PopupRequest, PopupStateResponse } from '../shared/messages';

const statusBadge = document.querySelector<HTMLSpanElement>('#connection-status');
const enabledToggle = document.querySelector<HTMLInputElement>('#enabled-toggle');
const silentToggle = document.querySelector<HTMLInputElement>('#silent-toggle');
const handoffMode = document.querySelector<HTMLSelectElement>('#handoff-mode');
const contextMenuToggle = document.querySelector<HTMLInputElement>('#context-menu-toggle');
const progressToggle = document.querySelector<HTMLInputElement>('#progress-toggle');
const badgeToggle = document.querySelector<HTMLInputElement>('#badge-toggle');
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
  if (silentToggle) silentToggle.checked = settings.downloadHandoffMode === 'auto';
  if (handoffMode) handoffMode.value = settings.downloadHandoffMode;
  if (contextMenuToggle) contextMenuToggle.checked = settings.contextMenuEnabled;
  if (progressToggle) progressToggle.checked = settings.showProgressAfterHandoff;
  if (badgeToggle) badgeToggle.checked = settings.showBadgeStatus;
  renderExcludedHosts(settings.excludedHosts);
  setControlsDisabled(isSaving);

  if (saveState && !isSaving) {
    saveState.textContent = state.lastError
      ? `Cached locally. ${state.lastError.message}`
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

function renderExcludedHosts(hosts: string[]) {
  if (!excludedHosts) return;
  excludedHosts.textContent = '';

  if (hosts.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'empty';
    empty.textContent = 'No excluded sites.';
    excludedHosts.append(empty);
    return;
  }

  for (const host of hosts) {
    const chip = document.createElement('span');
    chip.className = 'chip';
    chip.textContent = host;

    const removeButton = document.createElement('button');
    removeButton.type = 'button';
    removeButton.textContent = 'x';
    removeButton.title = `Remove ${host}`;
    removeButton.addEventListener('click', () => {
      const nextHosts = currentState?.extensionSettings?.excludedHosts.filter((candidate) => candidate !== host) ?? [];
      void updateSettings({ excludedHosts: nextHosts });
    });

    chip.append(removeButton);
    excludedHosts.append(chip);
  }
}

function setControlsDisabled(disabled: boolean) {
  for (const control of [
    enabledToggle,
    silentToggle,
    handoffMode,
    contextMenuToggle,
    progressToggle,
    badgeToggle,
    excludedHostInput,
    addHostButton,
    refreshButton,
  ]) {
    if (control) control.disabled = disabled;
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
  setControlsDisabled(true);
  if (saveState) saveState.textContent = 'Saving settings...';

  const state = await sendMessage<PopupStateResponse>({ type: 'extension_settings_update', settings });
  isSaving = false;
  renderState(state);
}

enabledToggle?.addEventListener('change', () => {
  void updateSettings({ enabled: enabledToggle.checked });
});

silentToggle?.addEventListener('change', () => {
  const mode: DownloadHandoffMode = silentToggle.checked ? 'auto' : 'ask';
  void updateSettings({ downloadHandoffMode: mode });
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
  await sendMessage({ type: 'popup_ping' });
  const state = await sendMessage<PopupStateResponse>({ type: 'popup_get_state' });
  renderState(state);
});

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

function normalizeHost(value: string): string {
  return value
    .trim()
    .replace(/^https?:\/\//i, '')
    .replace(/\/.*$/, '')
    .toLowerCase();
}

async function init() {
  await sendMessage({ type: 'popup_ping' }).catch(() => undefined);
  const state = await sendMessage<PopupStateResponse>({ type: 'popup_get_state' });
  renderState(state);
}

void init();
