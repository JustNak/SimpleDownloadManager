import { mount, type Component } from 'svelte';
import './app.css';
import { startNotificationSoundBridge } from './notificationSoundBridge';
import { revealPopupWhenReady } from './popupReady';

type RootComponent = Component;

const windowMode = new URLSearchParams(window.location.search).get('window');
const POPUP_WINDOW_MODES = new Set([
  'download-prompt',
  'download-progress',
  'torrent-progress',
  'batch-progress',
]);

async function resolveRootComponent(): Promise<RootComponent> {
  if (windowMode === 'download-prompt') {
    return (await import('./DownloadPromptWindow.svelte')).default;
  }
  if (windowMode === 'batch-progress') {
    return (await import('./BatchProgressWindow.svelte')).default;
  }
  if (windowMode === 'torrent-progress') {
    return (await import('./TorrentProgressWindow.svelte')).default;
  }
  if (windowMode === 'download-progress') {
    return (await import('./DownloadProgressWindow.svelte')).default;
  }

  return (await import('./App.svelte')).default;
}

void start();
startNotificationSoundBridge();

async function start() {
  const target = document.getElementById('root');
  if (!target) {
    throw new Error('Root element was not found.');
  }

  if (isPopupRoute()) {
    document.documentElement.classList.add('popup-window');
  }

  try {
    const RootComponent = await resolveRootComponent();
    mount(RootComponent, { target });
  } catch (error) {
    if (!isPopupRoute()) {
      throw error;
    }
    console.error('Failed to load popup window.', error);
    renderPopupLoadFailure(target);
    await revealPopupWhenReady();
  }
}

function isPopupRoute(): boolean {
  return windowMode !== null && POPUP_WINDOW_MODES.has(windowMode);
}

function renderPopupLoadFailure(target: HTMLElement) {
  target.innerHTML = `
    <div class="app-window popup-load-failure" role="alert">
      <div class="popup-load-failure-title">Popup failed to load</div>
      <div class="popup-load-failure-message">Close this window and try again.</div>
    </div>
  `;
}
