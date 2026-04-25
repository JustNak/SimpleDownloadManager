import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { BatchProgressWindow } from './BatchProgressWindow';
import { DownloadProgressWindow } from './DownloadProgressWindow';
import { DownloadPromptWindow } from './DownloadPromptWindow';
import './app.css';

const windowMode = new URLSearchParams(window.location.search).get('window');
const RootComponent =
  windowMode === 'download-prompt'
    ? DownloadPromptWindow
    : windowMode === 'batch-progress'
      ? BatchProgressWindow
    : windowMode === 'download-progress'
      ? DownloadProgressWindow
      : App;

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <RootComponent />
  </React.StrictMode>,
);
