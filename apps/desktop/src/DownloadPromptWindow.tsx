import React, { useEffect, useMemo, useState } from 'react';
import { AlertTriangle, Download, FolderOpen, Globe, HardDrive, MousePointerClick } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { DownloadPrompt } from './types';
import {
  browseDirectory,
  cancelDownloadPrompt,
  confirmDownloadPrompt,
  getAppSnapshot,
  getCurrentDownloadPrompt,
  showExistingDownloadPrompt,
  swapDownloadPrompt,
  subscribeToDownloadPromptChanged,
  subscribeToStateChanged,
} from './backend';
import { PopupTitlebar } from './PopupTitlebar';
import { FileBadge, formatBytes, getHost, joinDisplayPath } from './popupShared';
import { getErrorMessage } from './errors';
import { applyAppearance } from './appearance';
import { categoryFolderForFilename } from './downloadCategories';

export function DownloadPromptWindow() {
  const [prompt, setPrompt] = useState<DownloadPrompt | null>(null);
  const [directoryOverride, setDirectoryOverride] = useState<string | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');
  const currentWindow = isTauriRuntime() ? getCurrentWindow() : null;

  useEffect(() => {
    let promptDispose: (() => void | Promise<void>) | undefined;
    let stateDispose: (() => void | Promise<void>) | undefined;
    let latestSettings: Awaited<ReturnType<typeof getAppSnapshot>>['settings'] | null = null;

    const applySnapshotAppearance = (snapshot: Awaited<ReturnType<typeof getAppSnapshot>>) => {
      latestSettings = snapshot.settings;
      applyAppearance(snapshot.settings);
    };

    const media = typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-color-scheme: dark)') : null;
    const handleSystemThemeChange = () => {
      if (latestSettings) applyAppearance(latestSettings);
    };
    media?.addEventListener('change', handleSystemThemeChange);

    async function initialize() {
      applySnapshotAppearance(await getAppSnapshot());
      setPrompt(await getCurrentDownloadPrompt());
      promptDispose = await subscribeToDownloadPromptChanged((nextPrompt) => {
        setDirectoryOverride(null);
        setErrorMessage('');
        setIsBusy(false);
        setPrompt(nextPrompt);
      });
      stateDispose = await subscribeToStateChanged((nextSnapshot) => {
        applySnapshotAppearance(nextSnapshot);
      });
    }

    void initialize();
    return () => {
      media?.removeEventListener('change', handleSystemThemeChange);
      void promptDispose?.();
      void stateDispose?.();
    };
  }, []);

  const destination = useMemo(() => {
    if (!prompt) return '';
    return directoryOverride
      ? joinDisplayPath(joinDisplayPath(directoryOverride, categoryFolderForFilename(prompt.filename)), prompt.filename)
      : prompt.targetPath;
  }, [directoryOverride, prompt]);

  const isDuplicate = Boolean(prompt?.duplicateJob);
  const canSwapToBrowser = prompt?.source?.entryPoint === 'browser_download';
  const sourceLabel = prompt?.source
    ? `${prompt.source.browser} ${prompt.source.entryPoint.replaceAll('_', ' ')}`
    : 'Browser download';

  async function runAction(action: () => Promise<void>) {
    setIsBusy(true);
    setErrorMessage('');
    try {
      await action();
    } catch (error) {
      setErrorMessage(getErrorMessage(error, 'Action failed.'));
      setIsBusy(false);
    }
  }

  async function handleChangeDirectory() {
    const selected = await browseDirectory();
    if (selected) setDirectoryOverride(selected);
  }

  async function handleClose() {
    if (prompt) {
      await cancelDownloadPrompt(prompt.id).catch(() => undefined);
      return;
    }
    await currentWindow?.close();
  }

  if (!prompt) {
    return (
      <div className="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
        <PopupTitlebar title="Download prompt" onClose={() => void currentWindow?.close()} />
        <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">Waiting for a download...</div>
      </div>
    );
  }

  return (
    <div className="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
      <PopupTitlebar title={isDuplicate ? 'Duplicate download detected' : 'New download detected'} onClose={() => void handleClose()} />

      <main className="flex min-h-0 flex-1 flex-col bg-surface px-5 py-4">
        {isDuplicate ? (
          <div className="mb-4 flex items-start gap-3 rounded-md border border-warning/40 bg-warning/10 px-3 py-2.5 text-sm text-warning">
            <AlertTriangle size={18} className="mt-0.5 shrink-0" />
            <div className="min-w-0">
              <div className="font-semibold text-foreground">This URL is already in the queue.</div>
              <div className="mt-0.5 truncate text-warning/90">{prompt.duplicateJob?.filename}</div>
            </div>
          </div>
        ) : null}

        <section className="flex min-w-0 gap-4">
          <FileBadge filename={prompt.filename} large />
          <div className="min-w-0 flex-1">
            <h1 className="truncate text-lg font-semibold text-foreground" title={prompt.filename}>{prompt.filename}</h1>
            <div className="mt-1 truncate text-sm text-muted-foreground" title={prompt.url}>{getHost(prompt.url)}</div>
            <div className="mt-4 grid grid-cols-[116px_minmax(0,1fr)] gap-x-3 gap-y-3 text-sm">
              <MetaLabel icon={<Globe size={15} />} label="Source" />
              <MetaValue value={prompt.url} accent />
              <MetaLabel icon={<FolderOpen size={15} />} label="Destination" />
              <MetaValue value={destination || 'Choose a destination before downloading.'} />
              <MetaLabel icon={<HardDrive size={15} />} label="File size" />
              <MetaValue value={formatBytes(prompt.totalBytes)} />
              <MetaLabel icon={<MousePointerClick size={15} />} label="Detected by" />
              <MetaValue value={sourceLabel} />
            </div>
          </div>
        </section>

        {errorMessage ? (
          <div className="mt-4 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {errorMessage}
          </div>
        ) : null}

        <div className="mt-auto flex items-center justify-between gap-3 border-t border-border pt-4">
          <button
            onClick={() => void handleChangeDirectory()}
            disabled={isBusy}
            className="flex h-9 items-center gap-2 rounded-md border border-input px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
          >
            <FolderOpen size={16} />
            Change
          </button>

          <div className="flex items-center gap-2">
            {isDuplicate ? (
              <button
                onClick={() => void runAction(() => showExistingDownloadPrompt(prompt.id))}
                disabled={isBusy}
                className="h-9 rounded-md border border-input px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
              >
                Show Existing
              </button>
            ) : null}
            <button
              onClick={() => void runAction(() => cancelDownloadPrompt(prompt.id))}
              disabled={isBusy}
              className="h-9 rounded-md bg-destructive px-3 text-sm font-semibold text-destructive-foreground transition hover:bg-destructive/90 disabled:cursor-not-allowed disabled:opacity-50"
            >
              Cancel
            </button>
            {canSwapToBrowser ? (
              <button
                onClick={() => void runAction(() => swapDownloadPrompt(prompt.id))}
                disabled={isBusy}
                className="flex h-9 items-center gap-2 rounded-md bg-foreground px-3 text-sm font-semibold text-background transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
              >
                <BrowserWindowIcon />
                Swap
              </button>
            ) : null}
            <button
              onClick={() => void runAction(() => confirmDownloadPrompt(prompt.id, directoryOverride, isDuplicate))}
              disabled={isBusy}
              className="flex h-9 items-center gap-2 rounded-md bg-primary px-4 text-sm font-semibold text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Download size={16} />
              {isDuplicate ? 'Download Anyway' : 'Download'}
            </button>
          </div>
        </div>
      </main>
    </div>
  );
}

function BrowserWindowIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M3 9h18" />
      <path d="M7 6.5h.01" />
      <path d="M10 6.5h.01" />
      <path d="M13 6.5h.01" />
      <path d="M9 14h6" />
      <path d="m13 12 2 2-2 2" />
    </svg>
  );
}

function MetaLabel({ icon, label }: { icon: React.ReactNode; label: string }) {
  return (
    <div className="flex items-center gap-2 text-muted-foreground">
      {icon}
      <span>{label}</span>
    </div>
  );
}

function MetaValue({ value, accent = false }: { value: string; accent?: boolean }) {
  return <div className={`min-w-0 truncate ${accent ? 'text-primary' : 'text-foreground'}`} title={value}>{value}</div>;
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}
