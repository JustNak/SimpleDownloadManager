import { mount, type Component } from 'svelte';
import './app.css';

type RootComponent = Component;

const windowMode = new URLSearchParams(window.location.search).get('window');

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

void resolveRootComponent().then((RootComponent) => {
  const target = document.getElementById('root');
  if (!target) {
    throw new Error('Root element was not found.');
  }

  mount(RootComponent, { target });
});
