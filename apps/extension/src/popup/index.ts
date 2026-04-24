import type { HostToExtensionResponse, PongPayload } from '@myapp/protocol';
import { isErrorResponse, toUserFacingMessage } from '@myapp/protocol';
import browser from 'webextension-polyfill';
import { connectionForErrorCode } from '../background/nativeMessaging';
import type { PopupRequest, PopupStateResponse } from '../shared/messages';

const urlInput = document.querySelector<HTMLInputElement>('#url');
const statusBadge = document.querySelector<HTMLSpanElement>('#connection-status');
const messageArea = document.querySelector<HTMLDivElement>('#message-area');
const form = document.querySelector<HTMLFormElement>('#download-form');
const submitButton = document.querySelector<HTMLButtonElement>('#submit-button');
const openAppButton = document.querySelector<HTMLButtonElement>('#open-app-button');

function setBusy(isBusy: boolean) {
  if (submitButton) submitButton.disabled = isBusy;
  if (openAppButton) openAppButton.disabled = isBusy;
}

function updateConnectionStatus(connection: string) {
  if (!statusBadge) return;
  
  statusBadge.className = `status-badge ${connection}`;
  switch (connection) {
    case 'connected': statusBadge.textContent = 'Connected'; break;
    case 'host_missing': statusBadge.textContent = 'Host Missing'; break;
    case 'app_missing': statusBadge.textContent = 'App Missing'; break;
    case 'app_unreachable': statusBadge.textContent = 'Unreachable'; break;
    case 'error': statusBadge.textContent = 'Error'; break;
    default: statusBadge.textContent = 'Checking'; break;
  }

  // Disable submit if not connected
  if (submitButton) {
    submitButton.disabled = connection !== 'connected';
  }
}

function renderMessage(isError: boolean, message: string) {
  if (!messageArea) return;
  messageArea.className = `message-area ${isError ? 'error' : 'success'}`;
  messageArea.textContent = message;
}

function renderState(state: PopupStateResponse) {
  setBusy(state.isSubmitting);
  updateConnectionStatus(state.connection);

  if (state.lastError) {
    renderMessage(true, toUserFacingMessage(state.lastError.code, state.lastError.message));
  } else if (state.lastResult) {
    renderMessage(false, renderResult(state.lastResult));
  } else {
    if (messageArea) messageArea.textContent = '';
  }
}

function renderResult(response?: HostToExtensionResponse): string {
  if (!response) return '';
  if (isErrorResponse(response)) return '';
  if (response.type === 'accepted') {
    if (response.payload.status === 'canceled') {
      return 'Download canceled.';
    }
    if (response.payload.status === 'duplicate_existing_job') {
      return `Already queued as ${response.payload.jobId}`;
    }

    return `Queued as ${response.payload.jobId}`;
  }

  const payload = response.payload as PongPayload | undefined;
  const summary = payload?.queueSummary;
  if (!summary) {
    return 'Desktop app is connected.';
  }

  const attention = summary.attention ?? summary.failed;
  if (attention > 0) {
    return `Connected. ${summary.active} active, ${attention} need attention, ${summary.completed} finished.`;
  }

  return `Connected. ${summary.active} active, ${summary.completed} finished.`;
}

async function sendMessage<T>(message: PopupRequest): Promise<T> {
  return browser.runtime.sendMessage(message) as Promise<T>;
}

form?.addEventListener('submit', async (event) => {
  event.preventDefault();
  if (!urlInput || !urlInput.value) return;

  setBusy(true);
  const response = await sendMessage<HostToExtensionResponse>({ type: 'popup_enqueue', url: urlInput.value });
  
  if (isErrorResponse(response)) {
    const connState = connectionForErrorCode(response.code);
    renderState({
      connection: connState,
      isSubmitting: false,
      lastError: { code: response.code, message: response.message },
    });
    return;
  }

  renderState({ connection: 'connected', isSubmitting: false, lastResult: response });
  urlInput.value = '';
});

openAppButton?.addEventListener('click', async () => {
  setBusy(true);
  const response = await sendMessage<HostToExtensionResponse>({ type: 'popup_open_app' });
  
  if (isErrorResponse(response)) {
    const connState = connectionForErrorCode(response.code);
    renderState({
      connection: connState,
      isSubmitting: false,
      lastError: { code: response.code, message: response.message },
    });
    return;
  }

  renderState({ connection: 'connected', isSubmitting: false, lastResult: response });
});

// Initial ping
async function init() {
  try {
    const initialState = await sendMessage<HostToExtensionResponse>({ type: 'popup_ping' });
    if (isErrorResponse(initialState)) {
      const connState = connectionForErrorCode(initialState.code);
      renderState({
        connection: connState,
        isSubmitting: false,
        lastError: { code: initialState.code, message: initialState.message },
      });
    } else {
      renderState({ connection: 'connected', isSubmitting: false, lastResult: initialState });
    }
  } catch (err) {
    updateConnectionStatus('error');
    renderMessage(true, 'Failed to connect to background script.');
  }
}

init();
