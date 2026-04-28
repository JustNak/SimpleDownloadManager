import React, { useEffect, useMemo, useState } from 'react';
import { Archive, CheckCircle2, Download, FolderOpen, Pause, Play, X } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { JobState, type DownloadJob } from './types';
import {
  cancelJob,
  getAppSnapshot,
  getProgressBatchContext,
  pauseJob,
  resumeJob,
  revealJobInFolder,
  subscribeToStateChanged,
} from './backend';
import {
  calculateBatchProgress,
  deriveBulkPhase,
  type BulkPhase,
  type ProgressBatchContext,
} from './batchProgress';
import { PopupTitlebar } from './PopupTitlebar';
import { FileBadge, formatBytes, getHost } from './popupShared';
import { getErrorMessage } from './errors';
import { applyAppearance } from './appearance';
import { runPopupAction } from './popupActions';

export function BatchProgressWindow() {
  const [context, setContext] = useState<ProgressBatchContext | null>(null);
  const [jobs, setJobs] = useState<DownloadJob[]>([]);
  const [isBusy, setIsBusy] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');
  const currentWindow = isTauriRuntime() ? getCurrentWindow() : null;
  const batchId = useMemo(() => new URLSearchParams(window.location.search).get('batchId') || '', []);

  useEffect(() => {
    let dispose: (() => void | Promise<void>) | undefined;
    let disposed = false;
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
      const nextContext = batchId ? await getProgressBatchContext(batchId) : null;
      if (disposed) return;
      setContext(nextContext);

      const snapshot = await getAppSnapshot();
      if (disposed) return;
      applySnapshotAppearance(snapshot);
      setJobs(filterBatchJobs(snapshot.jobs, nextContext));

      dispose = await subscribeToStateChanged((nextSnapshot) => {
        applySnapshotAppearance(nextSnapshot);
        setJobs(filterBatchJobs(nextSnapshot.jobs, nextContext));
      });
    }

    void initialize().catch((error) => {
      if (!disposed) setErrorMessage(getErrorMessage(error, 'Could not load batch progress.'));
    });

    return () => {
      disposed = true;
      media?.removeEventListener('change', handleSystemThemeChange);
      void dispose?.();
    };
  }, [batchId]);

  async function runAction(
    action: () => Promise<void>,
    { closeOnSuccess = false }: { closeOnSuccess?: boolean } = {},
  ) {
    setIsBusy(true);
    setErrorMessage('');
    const result = await runPopupAction({
      action,
      close: closeOnSuccess && currentWindow ? () => currentWindow.close() : undefined,
    });
    if (!result.ok) {
      setErrorMessage(result.message);
    }
    setIsBusy(false);
  }

  if (!context) {
    return (
      <div className="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
        <PopupTitlebar title="Batch progress" />
        <div className="flex flex-1 items-center justify-center px-6 text-center text-sm text-muted-foreground">
          {errorMessage || 'This batch progress context is no longer available.'}
        </div>
      </div>
    );
  }

  const summary = calculateBatchProgress(jobs);
  const progress = summary.progress;
  const bulkPhase = context.kind === 'bulk' ? deriveBulkPhase(jobs) : null;
  const completedJob = jobs.find((job) => job.state === JobState.Completed && job.targetPath);
  const canPause = jobs.some(isPausable);
  const canResume = jobs.some(isResumable);
  const canCancel = jobs.some(isCancelable);

  return (
    <div className="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
      <PopupTitlebar title={context.title} />

      <main className="flex min-h-0 flex-1 flex-col bg-surface px-4 py-3">
        <section className="flex items-start gap-3">
          <div className="flex h-12 w-10 shrink-0 items-center justify-center rounded-sm border border-border bg-background text-primary">
            {context.kind === 'bulk' ? <Archive size={22} /> : <Download size={22} />}
          </div>
          <div className="min-w-0 flex-1">
            <h1 className="truncate text-base font-semibold leading-5 text-foreground" title={context.archiveName ?? context.title}>
              {context.archiveName ?? context.title}
            </h1>
            <div className="mt-0.5 text-xs text-muted-foreground">
              {summary.completedCount} of {summary.totalCount} completed
              {summary.failedCount > 0 ? `, ${summary.failedCount} failed` : ''}
            </div>
          </div>
          <div className="text-right text-2xl font-semibold tabular-nums text-foreground">
            {progress.toFixed(0)}%
          </div>
        </section>

        <section className="mt-3">
          <div className="mb-1.5 flex items-center justify-between text-xs tabular-nums text-muted-foreground">
            <span>{summary.activeCount} active</span>
            <span>
              {summary.knownTotal
                ? `${formatBytes(summary.downloadedBytes)} / ${formatBytes(summary.totalBytes)}`
                : `${summary.completedCount + summary.failedCount} / ${summary.totalCount} items`}
            </span>
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-progress-track">
            <div className={`h-1.5 rounded-full transition-all duration-300 ${summary.failedCount > 0 ? 'bg-destructive' : 'bg-primary'}`} style={{ width: `${progress}%` }} />
          </div>
        </section>

        {bulkPhase ? <BulkPhaseStrip phase={bulkPhase} jobs={jobs} /> : null}

        <BatchJobList jobs={jobs} />

        {errorMessage ? (
          <div className="mt-2 rounded border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive">
            {errorMessage}
          </div>
        ) : null}

        <div className="mt-3 flex flex-wrap justify-end gap-2 border-t border-border pt-3">
          <ActionButton
            label="Pause all"
            icon={<Pause size={16} />}
            disabled={isBusy || !canPause}
            onClick={() => void runAction(() => runForJobs(jobs.filter(isPausable), pauseJob))}
          />
          <ActionButton
            label="Resume all"
            icon={<Play size={16} />}
            disabled={isBusy || !canResume}
            primary={canResume}
            onClick={() => void runAction(() => runForJobs(jobs.filter(isResumable), resumeJob))}
          />
          <ActionButton
            label="Cancel active"
            icon={<X size={16} />}
            disabled={isBusy || !canCancel}
            danger={canCancel}
            onClick={() => void runAction(() => runForJobs(jobs.filter(isCancelable), cancelJob))}
          />
          <ActionButton
            label="Reveal completed"
            icon={<FolderOpen size={16} />}
            disabled={isBusy || !completedJob}
            onClick={() => {
              if (!completedJob) return;
              void runAction(async () => {
                await revealJobInFolder(completedJob.id);
              }, { closeOnSuccess: true });
            }}
          />
          {summary.activeCount === 0 ? (
            <ActionButton label="Close" icon={<X size={16} />} disabled={isBusy} onClick={() => void currentWindow?.close()} />
          ) : null}
        </div>
      </main>
    </div>
  );
}

function BatchJobList({ jobs }: { jobs: DownloadJob[] }) {
  return (
    <section className="mt-3 min-h-0 flex-1 overflow-y-auto rounded border border-border/60 bg-background/40">
      {jobs.length === 0 ? (
        <div className="flex h-full min-h-[120px] items-center justify-center px-4 text-center text-sm text-muted-foreground">
          Waiting for queued files to appear.
        </div>
      ) : (
        <div className="divide-y divide-border/60">
          {jobs.map((job) => (
            <BatchJobRow key={job.id} job={job} />
          ))}
        </div>
      )}
    </section>
  );
}

function BatchJobRow({ job }: { job: DownloadJob }) {
  const progress = Math.max(0, Math.min(100, job.progress));

  return (
    <div className="grid grid-cols-[40px_minmax(0,1fr)_86px] items-center gap-2 px-3 py-2">
      <FileBadge filename={job.filename} transferKind={job.transferKind} />
      <div className="min-w-0">
        <div className="truncate text-sm font-semibold leading-5 text-foreground" title={job.filename}>{job.filename}</div>
        <div className="truncate text-xs text-muted-foreground" title={job.url}>{job.transferKind === 'torrent' ? 'Torrent' : getHost(job.url)}</div>
        <div className="mt-1 h-1 overflow-hidden rounded-full bg-progress-track">
          <div className={`h-1 rounded-full transition-all duration-300 ${progressColor(job.state)}`} style={{ width: `${progress}%` }} />
        </div>
      </div>
      <div className="min-w-0 text-right text-xs">
        <div className={`font-semibold ${statusTextClass(job.state)}`}>{statusText(job)}</div>
        <div className="mt-0.5 tabular-nums text-muted-foreground">
          {job.state === JobState.Downloading ? `${formatBytes(job.speed)}/s` : `${progress.toFixed(0)}%`}
        </div>
        <div className="truncate tabular-nums text-muted-foreground" title={formatBytes(job.totalBytes)}>
          {job.totalBytes > 0 ? formatBytes(job.totalBytes) : 'Unknown'}
        </div>
      </div>
    </div>
  );
}

function BulkPhaseStrip({ phase, jobs }: { phase: BulkPhase; jobs: DownloadJob[] }) {
  const archive = jobs.find((job) => job.bulkArchive)?.bulkArchive;
  const phases: Array<{ id: BulkPhase; label: string }> = [
    { id: 'downloading', label: 'Downloading files' },
    { id: 'compressing', label: 'Compressing archive' },
    { id: 'ready', label: 'Ready' },
  ];
  const activeIndex = phase === 'failed' ? phases.findIndex((item) => item.id === 'compressing') : phases.findIndex((item) => item.id === phase);

  return (
    <section className="mt-3 rounded border border-border bg-background px-3 py-2">
      <div className="grid grid-cols-3 gap-2">
        {phases.map((item, index) => {
          const isDone = phase !== 'failed' && index < activeIndex;
          const isActive = index === activeIndex;
          return (
            <div key={item.id} className={`flex min-w-0 items-center gap-1.5 text-xs font-semibold ${isActive ? phaseClass(phase) : isDone ? 'text-success' : 'text-muted-foreground'}`}>
              <span className={`flex h-4 w-4 shrink-0 items-center justify-center rounded-full border text-[10px] ${isActive ? 'border-current' : isDone ? 'border-success bg-success text-success-foreground' : 'border-border'}`}>
                {isDone ? <CheckCircle2 size={10} /> : index + 1}
              </span>
              <span className="truncate">{item.label}</span>
            </div>
          );
        })}
      </div>
      {phase === 'failed' && archive?.error ? (
        <div className="mt-2 truncate text-xs text-destructive" title={archive.error}>{archive.error}</div>
      ) : null}
    </section>
  );
}

function ActionButton({
  label,
  icon,
  onClick,
  disabled,
  primary = false,
  danger = false,
}: {
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
  danger?: boolean;
}) {
  const buttonClass = danger
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

async function runForJobs(jobs: DownloadJob[], action: (id: string) => Promise<void>) {
  for (const job of jobs) {
    await action(job.id);
  }
}

function filterBatchJobs(jobs: DownloadJob[], context: ProgressBatchContext | null) {
  if (!context) return [];
  const ids = new Set(context.jobIds);
  return jobs.filter((job) => ids.has(job.id));
}

function isPausable(job: DownloadJob) {
  return [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding].includes(job.state);
}

function isResumable(job: DownloadJob) {
  return [JobState.Paused, JobState.Failed, JobState.Canceled].includes(job.state);
}

function isCancelable(job: DownloadJob) {
  return [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding, JobState.Paused].includes(job.state);
}

function statusText(job: DownloadJob) {
  switch (job.state) {
    case JobState.Seeding:
      return 'Seeding';
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

function statusTextClass(state: JobState) {
  if (state === JobState.Completed) return 'text-success';
  if (state === JobState.Failed) return 'text-destructive';
  if (state === JobState.Queued) return 'text-warning';
  if (state === JobState.Paused) return 'text-muted-foreground';
  return 'text-primary';
}

function progressColor(state: JobState) {
  if (state === JobState.Completed) return 'bg-success';
  if (state === JobState.Failed) return 'bg-destructive';
  if (state === JobState.Queued) return 'bg-warning';
  return 'bg-primary';
}

function phaseClass(phase: BulkPhase) {
  if (phase === 'failed') return 'text-destructive';
  if (phase === 'ready') return 'text-success';
  if (phase === 'compressing') return 'text-warning';
  return 'text-primary';
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}
