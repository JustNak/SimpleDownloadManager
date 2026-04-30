import React, { Suspense } from 'react';
import ReactDOM from 'react-dom/client';
import './app.css';

const windowMode = new URLSearchParams(window.location.search).get('window');
const RootComponent = React.lazy(() => {
  if (windowMode === 'download-prompt') {
    return import('./DownloadPromptWindow').then((module) => ({
      default: module.DownloadPromptWindow,
    }));
  }

  if (windowMode === 'batch-progress') {
    return import('./BatchProgressWindow').then((module) => ({
      default: module.BatchProgressWindow,
    }));
  }

  if (windowMode === 'torrent-progress') {
    return import('./TorrentProgressWindow').then((module) => ({
      default: module.TorrentProgressWindow,
    }));
  }

  if (windowMode === 'download-progress') {
    return import('./DownloadProgressWindow').then((module) => ({
      default: module.DownloadProgressWindow,
    }));
  }

  return import('./App');
});

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <Suspense fallback={<div className="min-h-screen bg-surface" />}>
      <RootComponent />
    </Suspense>
  </React.StrictMode>,
);
