import React from 'react';
import ReactDOM from 'react-dom/client';
import './app.css';

type RootComponent = React.ComponentType;

const windowMode = new URLSearchParams(window.location.search).get('window');

async function resolveRootComponent(): Promise<RootComponent> {
  if (windowMode === 'download-prompt') {
    return (await import('./DownloadPromptWindow')).DownloadPromptWindow;
  }
  if (windowMode === 'batch-progress') {
    return (await import('./BatchProgressWindow')).BatchProgressWindow;
  }
  if (windowMode === 'torrent-progress') {
    return (await import('./TorrentProgressWindow')).TorrentProgressWindow;
  }
  if (windowMode === 'download-progress') {
    return (await import('./DownloadProgressWindow')).DownloadProgressWindow;
  }

  return (await import('./App')).default;
}

void resolveRootComponent().then((RootComponent) => {
  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <RootComponent />
    </React.StrictMode>,
  );
});
