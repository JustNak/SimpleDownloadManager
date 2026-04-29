import React, { useEffect, useMemo, useState } from 'react';
import { AlertTriangle, ChevronDown, Download, FolderOpen, Globe, HardDrive, MousePointerClick } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { DownloadPrompt } from './types';
import {
  browseDirectory,
  cancelDownloadPrompt,
  confirmDownloadPrompt,
  getAppSnapshot,
  getCurrentDownloadPrompt,
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
  const [duplicateMenuOpen, setDuplicateMenuOpen] = useState(false);
  const [isRenamingDuplicate, setIsRenamingDuplicate] = useState(false);
  const [renamedFilename, setRenamedFilename] = useState('');
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
        setDuplicateMenuOpen(false);
        setIsRenamingDuplicate(false);
        setRenamedFilename('');
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

  const isDuplicate = Boolean(prompt?.duplicateJob || prompt?.duplicatePath);
  const duplicateLabel = prompt?.duplicateJob?.filename
    ?? prompt?.duplicateFilename
    ?? prompt?.duplicatePath
    ?? '';
  const duplicateMessage = prompt?.duplicateJob ? 'Already in queue: ' : 'Destination exists: ';
  const overwriteLabel = prompt?.duplicateJob ? 'replace queue' : 'replace file';
  const canSwapToBrowser = prompt?.source?.entryPoint === 'browser_download' && !isDuplicate;
  const sourceLabel = prompt?.source
    ? `${prompt.source.browser} ${prompt.source.entryPoint.replaceAll('_', ' ')}`
    : 'Browser download';
  const trimmedRenamedFilename = renamedFilename.trim();

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

  function startDuplicateRename() {
    if (!prompt) return;
    setRenamedFilename(prompt.filename);
    setDuplicateMenuOpen(false);
    setIsRenamingDuplicate(true);
  }

  async function confirmDuplicateAction(duplicateAction: 'download_anyway' | 'overwrite') {
    if (!prompt) return;
    setDuplicateMenuOpen(false);
    await runAction(() => confirmDownloadPrompt(prompt.id, directoryOverride, { duplicateAction }));
  }

  async function confirmDuplicateRename() {
    if (!prompt || !trimmedRenamedFilename) return;
    await runAction(() => confirmDownloadPrompt(prompt.id, directoryOverride, {
      duplicateAction: 'rename',
      renamedFilename: trimmedRenamedFilename,
    }));
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

      <main className="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface px-3 py-2">
        {isDuplicate ? (
          <div className="mb-1.5 flex shrink-0 items-center gap-1.5 overflow-hidden rounded border border-warning/45 bg-warning/10 px-2 py-1.5 text-[11px] leading-4 text-warning">
            <AlertTriangle size={14} className="shrink-0" />
            <div className="min-w-0 flex-1 truncate" title={duplicateLabel}>
              <span className="font-semibold text-foreground">{duplicateMessage}</span>
              <span className="text-warning/90">{duplicateLabel}</span>
            </div>
          </div>
        ) : null}

        <section className="flex min-h-0 min-w-0 shrink-0 gap-2">
          <FileBadge filename={prompt.filename} />
          <div className="min-w-0 flex-1 overflow-hidden">
            <h1 className="truncate text-sm font-semibold leading-5 text-foreground" title={prompt.filename}>{prompt.filename}</h1>
            <div className="truncate text-[11px] leading-4 text-muted-foreground" title={prompt.url}>{getHost(prompt.url)}</div>
            <div className="mt-1.5 grid min-w-0 grid-cols-[86px_minmax(0,1fr)] gap-x-2 gap-y-0.5 text-[10px] leading-[14px]">
              <MetaLabel icon={<Globe size={13} />} label="Source" />
              <MetaValue value={prompt.url} accent />
              <MetaLabel icon={<FolderOpen size={13} />} label="Destination" />
              <MetaValue value={destination || 'Choose a destination before downloading.'} />
              <MetaLabel icon={<HardDrive size={13} />} label="File size" />
              <MetaValue value={formatBytes(prompt.totalBytes)} />
              <MetaLabel icon={<MousePointerClick size={13} />} label="Detected by" />
              <MetaValue value={sourceLabel} />
            </div>
          </div>
        </section>

        {errorMessage ? (
          <div className="mt-1.5 shrink-0 truncate rounded border border-destructive/40 bg-destructive/10 px-2 py-1 text-[11px] leading-4 text-destructive" title={errorMessage}>
            {errorMessage}
          </div>
        ) : null}

        <div className="mt-auto flex min-h-[38px] shrink-0 items-center justify-between gap-2 border-t border-border pt-2">
          <button
            onClick={() => void handleChangeDirectory()}
            disabled={isBusy}
            className="flex h-8 shrink-0 items-center gap-2 rounded border border-input px-3 text-xs font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
          >
            <FolderOpen size={16} />
            Change
          </button>

          {isDuplicate && isRenamingDuplicate ? (
            <div className="flex min-w-0 flex-1 items-center justify-end gap-1.5">
              <input
                value={renamedFilename}
                onChange={(event) => setRenamedFilename(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') void confirmDuplicateRename();
                  if (event.key === 'Escape') setIsRenamingDuplicate(false);
                }}
                autoFocus
                className="h-8 min-w-0 flex-1 rounded border border-input bg-background px-2.5 text-xs text-foreground outline-none transition focus:border-primary"
                aria-label="Renamed filename"
                title={renamedFilename}
              />
              <button
                onClick={() => {
                  setIsRenamingDuplicate(false);
                  setRenamedFilename('');
                }}
                disabled={isBusy}
                className="h-8 rounded bg-destructive px-3 text-xs font-semibold text-destructive-foreground transition hover:bg-destructive/90 disabled:cursor-not-allowed disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                onClick={() => void confirmDuplicateRename()}
                disabled={isBusy || !trimmedRenamedFilename}
                className="h-8 rounded bg-primary px-4 text-xs font-semibold text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
              >
                Rename
              </button>
            </div>
          ) : (
            <div className="flex min-w-0 items-center justify-end gap-1.5">
              <button
                onClick={() => void runAction(() => cancelDownloadPrompt(prompt.id))}
                disabled={isBusy}
                className="h-8 rounded bg-destructive px-3 text-xs font-semibold text-destructive-foreground transition hover:bg-destructive/90 disabled:cursor-not-allowed disabled:opacity-50"
              >
                Cancel
              </button>
              {canSwapToBrowser ? (
                <button
                  onClick={() => void runAction(() => swapDownloadPrompt(prompt.id))}
                  disabled={isBusy}
                  className="flex h-8 items-center gap-2 rounded bg-foreground px-3 text-xs font-semibold text-background transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <BrowserWindowIcon />
                  Swap
                </button>
              ) : null}
              {isDuplicate ? (
                <div className="relative">
                  <button
                    onClick={() => setDuplicateMenuOpen((open) => !open)}
                    disabled={isBusy}
                    className="flex h-8 min-w-[118px] items-center justify-center gap-1.5 rounded bg-primary px-3 text-xs font-semibold text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <Download size={14} />
                    Choose Action
                    <ChevronDown size={13} />
                  </button>
                  {duplicateMenuOpen ? (
                    <div className="absolute bottom-full right-0 z-10 mb-1 w-52 overflow-hidden rounded border border-border bg-popover text-xs text-popover-foreground shadow-xl">
                      <button
                        onClick={() => void confirmDuplicateAction('overwrite')}
                        className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left font-semibold text-warning hover:bg-muted"
                      >
                        <span>Overwrite</span>
                        <span className="text-[10px] font-medium text-muted-foreground">{overwriteLabel}</span>
                      </button>
                      <button
                        onClick={startDuplicateRename}
                        className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left font-semibold text-foreground hover:bg-muted"
                      >
                        <span>Rename</span>
                        <span className="text-[10px] font-medium text-muted-foreground">edit name</span>
                      </button>
                      <button
                        onClick={() => void confirmDuplicateAction('download_anyway')}
                        className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left font-semibold text-foreground hover:bg-muted"
                      >
                        <span>Download Anyway</span>
                        <span className="text-[10px] font-medium text-muted-foreground">copy</span>
                      </button>
                    </div>
                  ) : null}
                </div>
              ) : (
                <button
                  onClick={() => void runAction(() => confirmDownloadPrompt(prompt.id, directoryOverride))}
                  disabled={isBusy}
                  className="flex h-8 min-w-[92px] items-center justify-center gap-2 rounded bg-primary px-4 text-xs font-semibold text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <Download size={15} />
                  Download
                </button>
              )}
            </div>
          )}
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
    <div className="flex min-w-0 items-center gap-1.5 text-muted-foreground">
      <span className="shrink-0">{icon}</span>
      <span className="truncate">{label}</span>
    </div>
  );
}

function MetaValue({ value, accent = false }: { value: string; accent?: boolean }) {
  return <div className={`min-w-0 truncate ${accent ? 'text-primary' : 'text-foreground'}`} title={value}>{value}</div>;
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}
