import React, { useEffect, useMemo, useState } from 'react';
import { CheckCircle2, Clock3, Download, ExternalLink, FolderOpen, Pause, Play, RotateCw, X } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { JobState, type DownloadJob } from './types';
import {
  cancelJob,
  getAppSnapshot,
  openJobFile,
  pauseJob,
  resumeJob,
  retryJob,
  revealJobInFolder,
  subscribeToStateChanged,
} from './backend';
import { PopupTitlebar } from './PopupTitlebar';
import { FileBadge, formatBytes, formatTime, getHost } from './popupShared';
import { getErrorMessage } from './errors';

export function DownloadProgressWindow() {
  const [job, setJob] = useState<DownloadJob | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [isConfirmingCancel, setIsConfirmingCancel] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');
  const currentWindow = isTauriRuntime() ? getCurrentWindow() : null;
  const jobId = useMemo(() => new URLSearchParams(window.location.search).get('jobId') || '', []);

  useEffect(() => {
    document.documentElement.classList.add('dark');
    let dispose: (() => void | Promise<void>) | undefined;

    async function initialize() {
      const snapshot = await getAppSnapshot();
      setJob(snapshot.jobs.find((candidate) => candidate.id === jobId) ?? null);
      dispose = await subscribeToStateChanged((nextSnapshot) => {
        setJob(nextSnapshot.jobs.find((candidate) => candidate.id === jobId) ?? null);
      });
    }

    void initialize();
    return () => {
      void dispose?.();
    };
  }, [jobId]);

  useEffect(() => {
    setIsConfirmingCancel(false);
  }, [job?.id]);

  async function runAction(action: () => Promise<void>) {
    setIsBusy(true);
    setIsConfirmingCancel(false);
    setErrorMessage('');
    try {
      await action();
    } catch (error) {
      setErrorMessage(getErrorMessage(error, 'Action failed.'));
    } finally {
      setIsBusy(false);
    }
  }

  if (!job) {
    return (
      <div className="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
        <PopupTitlebar title="Download progress" />
        <div className="flex flex-1 items-center justify-center px-6 text-center text-sm text-muted-foreground">
          This download is no longer available.
        </div>
      </div>
    );
  }

  const progress = Math.max(0, Math.min(100, job.progress));
  const isActive = [JobState.Queued, JobState.Starting, JobState.Downloading].includes(job.state);
  const isPaused = job.state === JobState.Paused;
  const isCompleted = job.state === JobState.Completed;
  const isFailed = job.state === JobState.Failed;
  const activeJobId = job.id;
  const cancelLabel = isConfirmingCancel ? 'Confirm' : 'Cancel';

  function handleCancelClick() {
    if (!isConfirmingCancel) {
      setIsConfirmingCancel(true);
      return;
    }

    void runAction(() => cancelJob(activeJobId));
  }

  return (
    <div className="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
      <PopupTitlebar title="Download progress" />

      <main className="flex min-h-0 flex-1 flex-col bg-surface px-4 py-3">
        <section className="flex min-w-0 gap-3">
          <FileBadge filename={job.filename} />
          <div className="min-w-0 flex-1">
            <h1 className="truncate text-base font-semibold leading-5 text-foreground" title={job.filename}>{job.filename}</h1>
            <div className="mt-0.5 truncate text-xs text-muted-foreground" title={job.url}>{getHost(job.url)}</div>
            <div className={`mt-2 inline-flex h-6 items-center gap-1.5 rounded border px-2 text-xs font-semibold ${statusClass(job.state)}`}>
              {isCompleted ? <CheckCircle2 size={13} /> : isFailed ? <X size={13} /> : <Download size={13} />}
              {statusText(job)}
            </div>
          </div>
        </section>

        <section className="mt-4">
          <div className="mb-1.5 flex items-baseline justify-between">
            <span className="text-2xl font-semibold tabular-nums text-foreground">{progress.toFixed(0)}%</span>
            <span className="text-xs tabular-nums text-muted-foreground">
              {formatBytes(job.downloadedBytes)} / {job.totalBytes > 0 ? formatBytes(job.totalBytes) : 'Unknown'}
            </span>
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-progress-track">
            <div className={`h-1.5 rounded-full transition-all duration-300 ${progressColor(job.state)}`} style={{ width: `${progress}%` }} />
          </div>
        </section>

        <section className="mt-3 grid grid-cols-3 gap-1 text-xs">
          <Metric label="Speed" value={job.state === JobState.Downloading ? `${formatBytes(job.speed)}/s` : '--'} />
          <Metric label="ETA" value={job.state === JobState.Downloading ? formatTime(job.eta) : '--'} />
          <Metric label="State" value={statusText(job)} />
        </section>

        <div className="mt-3 grid grid-cols-[76px_minmax(0,1fr)] gap-x-2 gap-y-1.5 text-xs">
          <div className="flex items-center gap-1.5 text-muted-foreground"><FolderOpen size={14} /> Path</div>
          <div className="truncate text-foreground" title={job.targetPath}>{job.targetPath || 'No destination recorded yet.'}</div>
          <div className="flex items-center gap-1.5 text-muted-foreground"><Clock3 size={14} /> Source</div>
          <div className="truncate text-primary" title={job.url}>{job.url}</div>
        </div>

        {errorMessage ? (
          <div className="mt-2 rounded border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive">
            {errorMessage}
          </div>
        ) : null}

        <div className="mt-auto flex justify-end gap-2 border-t border-border pt-3">
          {isCompleted ? (
            <ActionButton label="Open" icon={<ExternalLink size={16} />} disabled={isBusy} primary onClick={() => void runAction(() => openJobFile(job.id))} />
          ) : null}
          {(isCompleted || isFailed || job.targetPath) ? (
            <ActionButton label="Reveal" icon={<FolderOpen size={16} />} disabled={isBusy} onClick={() => void runAction(() => revealJobInFolder(job.id))} />
          ) : null}
          {isActive ? (
            <ActionButton label="Pause" icon={<Pause size={16} />} disabled={isBusy} onClick={() => void runAction(() => pauseJob(job.id))} />
          ) : null}
          {isPaused ? (
            <ActionButton label="Resume" icon={<Play size={16} />} disabled={isBusy} primary onClick={() => void runAction(() => resumeJob(job.id))} />
          ) : null}
          {isFailed ? (
            <ActionButton label="Retry" icon={<RotateCw size={16} />} disabled={isBusy} primary onClick={() => void runAction(() => retryJob(job.id))} />
          ) : null}
          {(isActive || isPaused) ? (
            <ActionButton label={cancelLabel} icon={<X size={16} />} disabled={isBusy} destructive={isConfirmingCancel} danger={!isConfirmingCancel} onClick={handleCancelClick} />
          ) : null}
          {(isCompleted || isFailed) ? (
            <ActionButton label="Close" icon={<X size={16} />} disabled={isBusy} onClick={() => void currentWindow?.close()} />
          ) : null}
        </div>
      </main>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 px-2 py-1">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-0.5 truncate text-sm font-semibold tabular-nums text-foreground" title={value}>{value}</div>
    </div>
  );
}

function ActionButton({
  label,
  icon,
  onClick,
  disabled,
  primary = false,
  danger = false,
  destructive = false,
}: {
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
  danger?: boolean;
  destructive?: boolean;
}) {
  const buttonClass = destructive
    ? 'bg-destructive text-destructive-foreground hover:bg-destructive/90'
    : danger
      ? 'border border-destructive/50 bg-destructive/10 text-destructive hover:bg-destructive hover:text-destructive-foreground'
      : primary
        ? 'bg-primary text-primary-foreground hover:bg-primary/90'
        : 'border border-input text-foreground hover:bg-muted';

  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`flex h-8 items-center gap-1.5 rounded px-2.5 text-xs font-medium transition disabled:cursor-not-allowed disabled:opacity-50 ${buttonClass}`}
    >
      {icon}
      {label}
    </button>
  );
}

function statusText(job: DownloadJob) {
  switch (job.state) {
    case JobState.Downloading:
      return 'Downloading';
    case JobState.Starting:
      return 'Starting';
    case JobState.Queued:
      return 'Queued';
    case JobState.Paused:
      return 'Paused';
    case JobState.Completed:
      return 'Completed';
    case JobState.Failed:
      return job.failureCategory ? `${job.failureCategory} error` : 'Error';
    case JobState.Canceled:
      return 'Canceled';
    default:
      return job.state;
  }
}

function statusClass(state: JobState) {
  if (state === JobState.Completed) return 'border-success/40 bg-success/10 text-success';
  if (state === JobState.Failed) return 'border-destructive/40 bg-destructive/10 text-destructive';
  if (state === JobState.Queued) return 'border-warning/40 bg-warning/10 text-warning';
  return 'border-primary/40 bg-primary/10 text-primary';
}

function progressColor(state: JobState) {
  if (state === JobState.Completed) return 'bg-success';
  if (state === JobState.Failed) return 'bg-destructive';
  if (state === JobState.Queued) return 'bg-warning';
  return 'bg-primary';
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}
