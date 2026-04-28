import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  Download,
  ExternalLink,
  FolderOpen,
  Link2,
  Pause,
  Play,
  RotateCw,
  X,
} from 'lucide-react';
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
import { applyAppearance } from './appearance';
import { runPopupAction } from './popupActions';
import {
  calculateDownloadProgressMetrics,
  recordProgressSample,
  shouldShowCompletedFileAction,
  type DownloadProgressMetrics,
  type ProgressSample,
} from './downloadProgressMetrics';
import {
  isTorrentMetadataPending,
  formatTorrentFetchedSize,
  formatTorrentVerifiedSize,
  torrentActivitySummary,
  torrentDisplayName,
} from './queueRowPresentation';

type PopupActionRunner = (
  action: () => Promise<void>,
  options?: { closeOnSuccess?: boolean },
) => Promise<void>;

interface ProgressViewProps {
  job: DownloadJob;
  progress: number;
  progressMetrics: DownloadProgressMetrics;
  isBusy: boolean;
  isConfirmingCancel: boolean;
  errorMessage: string;
  runAction: PopupActionRunner;
  onCancelClick: () => void;
  onClose: () => void;
}

export function DownloadProgressWindow() {
  const [job, setJob] = useState<DownloadJob | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [isConfirmingCancel, setIsConfirmingCancel] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');
  const progressSamplesRef = useRef<ProgressSample[]>([]);
  const currentWindow = isTauriRuntime() ? getCurrentWindow() : null;
  const jobId = useMemo(() => new URLSearchParams(window.location.search).get('jobId') || '', []);

  useEffect(() => {
    let dispose: (() => void | Promise<void>) | undefined;
    let latestSettings: Awaited<ReturnType<typeof getAppSnapshot>>['settings'] | null = null;

    const applySnapshotAppearance = (snapshot: Awaited<ReturnType<typeof getAppSnapshot>>) => {
      latestSettings = snapshot.settings;
      applyAppearance(snapshot.settings);
    };
    const applySnapshotJob = (snapshot: Awaited<ReturnType<typeof getAppSnapshot>>) => {
      const nextJob = snapshot.jobs.find((candidate) => candidate.id === jobId) ?? null;
      if (nextJob) {
        progressSamplesRef.current = recordProgressSample(progressSamplesRef.current, nextJob);
      }
      setJob(nextJob);
    };

    const media = typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-color-scheme: dark)') : null;
    const handleSystemThemeChange = () => {
      if (latestSettings) applyAppearance(latestSettings);
    };
    media?.addEventListener('change', handleSystemThemeChange);

    async function initialize() {
      const snapshot = await getAppSnapshot();
      applySnapshotAppearance(snapshot);
      applySnapshotJob(snapshot);
      dispose = await subscribeToStateChanged((nextSnapshot) => {
        applySnapshotAppearance(nextSnapshot);
        applySnapshotJob(nextSnapshot);
      });
    }

    void initialize();
    return () => {
      media?.removeEventListener('change', handleSystemThemeChange);
      void dispose?.();
    };
  }, [jobId]);

  useEffect(() => {
    setIsConfirmingCancel(false);
  }, [job?.id]);

  async function runAction(
    action: () => Promise<void>,
    { closeOnSuccess = false }: { closeOnSuccess?: boolean } = {},
  ) {
    setIsBusy(true);
    setIsConfirmingCancel(false);
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

  if (!job) {
    return (
      <ProgressShell title="Download progress">
        <div className="flex flex-1 items-center justify-center px-6 text-center text-sm text-muted-foreground">
          This download is no longer available.
        </div>
      </ProgressShell>
    );
  }

  const progress = clampProgress(job.progress);
  const progressMetrics = calculateDownloadProgressMetrics(job, progressSamplesRef.current);
  const activeJobId = job.id;
  const sharedProps = {
    job,
    progress,
    progressMetrics,
    isBusy,
    isConfirmingCancel,
    errorMessage,
    runAction,
    onCancelClick: () => {
      if (!isConfirmingCancel) {
        setIsConfirmingCancel(true);
        return;
      }

      void runAction(() => cancelJob(activeJobId));
    },
    onClose: () => {
      void currentWindow?.close();
    },
  };

  return job.transferKind === 'torrent' ? <TorrentingProgressView {...sharedProps} /> : <CompactDownloadProgressView {...sharedProps} />;
}

function CompactDownloadProgressView({
  job,
  progress,
  progressMetrics,
  isBusy,
  isConfirmingCancel,
  errorMessage,
  runAction,
  onCancelClick,
  onClose,
}: ProgressViewProps) {
  return (
    <ProgressShell title="Download progress">
      <CompactMain>
        <HeaderStrip
          job={job}
          title={job.filename}
          subtitle={getHost(job.url)}
          status={statusText(job)}
        />

        <ProgressStrip
          progress={progress}
          bytesText={downloadedText(job)}
          colorClass={progressColor(job)}
        />

        <MetricRail>
          <Metric label="Speed" value={job.state === JobState.Downloading ? `${formatBytes(progressMetrics.averageSpeed)}/s` : '--'} />
          <Metric label="ETA" value={job.state === JobState.Downloading ? formatTime(progressMetrics.timeRemaining) : '--'} />
          <Metric label="Size" value={job.totalBytes > 0 ? formatBytes(job.totalBytes) : 'Unknown'} />
        </MetricRail>

        <DetailRows job={job} />
        <ErrorMessage message={errorMessage} />
        <ActionBar
          job={job}
          isBusy={isBusy}
          isConfirmingCancel={isConfirmingCancel}
          runAction={runAction}
          onCancelClick={onCancelClick}
          onClose={onClose}
        />
      </CompactMain>
    </ProgressShell>
  );
}

function TorrentingProgressView({
  job,
  progress,
  progressMetrics,
  isBusy,
  isConfirmingCancel,
  errorMessage,
  runAction,
  onCancelClick,
  onClose,
}: ProgressViewProps) {
  const metadataPending = isTorrentMetadataPending(job);

  return (
    <ProgressShell title="Torrenting">
      <CompactMain>
        <HeaderStrip
          job={job}
          title={torrentDisplayName(job)}
          subtitle={torrentStatusLine(job)}
          status={statusText(job)}
        />

        {!metadataPending ? (
          <>
            <ProgressStrip
              progress={progress}
              progressLabel={`Verified ${progress.toFixed(0)}%`}
              bytesText={verifiedTorrentText(job)}
              colorClass={progressColor(job)}
            />

            <MetricRail>
              <Metric label="Speed" value={job.state === JobState.Downloading ? `${formatBytes(progressMetrics.averageSpeed)}/s` : '--'} />
              <Metric label="ETA" value={job.state === JobState.Downloading ? formatTime(progressMetrics.timeRemaining) : '--'} />
              <Metric label="Peers" value={torrentPeerCount(job)} />
            </MetricRail>

            <TorrentDownloadedRow job={job} />
          </>
        ) : null}

        <DetailRows job={job} />
        <ErrorMessage message={errorMessage} />
        <ActionBar
          job={job}
          isBusy={isBusy}
          isConfirmingCancel={isConfirmingCancel}
          runAction={runAction}
          onCancelClick={onCancelClick}
          onClose={onClose}
        />
      </CompactMain>
    </ProgressShell>
  );
}

function ProgressShell({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
      <PopupTitlebar title={title} />
      {children}
    </div>
  );
}

function CompactMain({ children }: { children: React.ReactNode }) {
  return (
    <main className="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface px-3 py-1.5">
      {children}
    </main>
  );
}

function HeaderStrip({
  job,
  title,
  subtitle,
  status,
}: {
  job: DownloadJob;
  title: string;
  subtitle: string;
  status: string;
}) {
  return (
    <section className="flex min-w-0 items-start gap-2">
      <FileBadge filename={job.filename} transferKind={job.transferKind} />
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-start justify-between gap-2">
          <div className="min-w-0">
            <h1 className="truncate text-sm font-semibold leading-5 text-foreground" title={title}>{title}</h1>
            <div className="truncate text-[11px] text-muted-foreground" title={subtitle}>{subtitle}</div>
          </div>
          <span className={`shrink-0 rounded border px-1.5 py-0.5 text-[10px] font-semibold leading-4 ${statusClass(job)}`}>
            {status}
          </span>
        </div>
      </div>
    </section>
  );
}

function ProgressStrip({
  progress,
  progressLabel = `${progress.toFixed(0)}%`,
  bytesText,
  colorClass,
}: {
  progress: number;
  progressLabel?: string;
  bytesText: string;
  colorClass: string;
}) {
  return (
    <section className="mt-1.5">
      <div className="mb-1 flex items-end justify-between gap-2">
        <span className="text-xl font-semibold tabular-nums leading-none text-foreground">{progressLabel}</span>
        <span className="truncate text-[11px] tabular-nums text-muted-foreground" title={bytesText}>{bytesText}</span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-progress-track">
        <div className={`h-1.5 rounded-full transition-all duration-300 ${colorClass}`} style={{ width: `${progress}%` }} />
      </div>
    </section>
  );
}

function TorrentDownloadedRow({ job }: { job: DownloadJob }) {
  const value = torrentFetchedText(job);
  return (
    <div className="mt-1 grid grid-cols-[70px_minmax(0,1fr)] gap-x-1.5 text-[10px] leading-4">
      <div className="flex items-center gap-1 text-muted-foreground"><Download size={12} /> Downloaded</div>
      <div className="truncate text-foreground" title={value}>{value}</div>
    </div>
  );
}

function MetricRail({ children }: { children: React.ReactNode }) {
  return (
    <section className="mt-1.5 grid grid-cols-3 gap-2 bg-background/30 border-t border-border/35 px-2 py-1">
      {children}
    </section>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <div className="text-[10px] leading-3 text-muted-foreground">{label}</div>
      <div className="truncate text-xs font-semibold tabular-nums leading-4 text-foreground" title={value}>{value}</div>
    </div>
  );
}

function DetailRows({ job }: { job: DownloadJob }) {
  return (
    <div className="mt-1 grid grid-cols-[48px_minmax(0,1fr)] gap-x-1.5 gap-y-0 text-[10px] leading-4">
      <div className="flex items-center gap-1 text-muted-foreground"><FolderOpen size={12} /> Path</div>
      <div className="truncate text-foreground" title={job.targetPath}>{job.targetPath || 'No destination recorded yet.'}</div>
      <div className="flex items-center gap-1 text-muted-foreground"><Link2 size={12} /> Source</div>
      <div className="truncate text-primary" title={job.url}>{job.transferKind === 'torrent' ? torrentSourceText(job) : job.url}</div>
    </div>
  );
}

function ErrorMessage({ message }: { message: string }) {
  if (!message) return null;

  return (
    <div className="mt-1.5 truncate rounded border border-destructive/40 bg-destructive/10 px-2 py-1 text-[11px] text-destructive" title={message}>
      {message}
    </div>
  );
}

function ActionBar({
  job,
  isBusy,
  isConfirmingCancel,
  runAction,
  onCancelClick,
  onClose,
}: {
  job: DownloadJob;
  isBusy: boolean;
  isConfirmingCancel: boolean;
  runAction: PopupActionRunner;
  onCancelClick: () => void;
  onClose: () => void;
}) {
  const isActive = [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding].includes(job.state);
  const isPaused = job.state === JobState.Paused;
  const isCompleted = job.state === JobState.Completed;
  const isFailed = job.state === JobState.Failed;
  const cancelLabel = isConfirmingCancel ? 'Confirm' : 'Cancel';

  return (
    <div className="mt-auto flex justify-end gap-1.5 border-t border-border pt-1.5">
      {isCompleted ? (
        <ActionButton
          label="Open"
          icon={<ExternalLink size={14} />}
          disabled={isBusy}
          primary
          onClick={() => void runAction(async () => {
            await openJobFile(job.id);
          }, { closeOnSuccess: true })}
        />
      ) : null}
      {shouldShowCompletedFileAction(job) ? (
        <ActionButton
          label="Show"
          icon={<FolderOpen size={14} />}
          disabled={isBusy}
          onClick={() => void runAction(async () => {
            await revealJobInFolder(job.id);
          }, { closeOnSuccess: true })}
        />
      ) : null}
      {isActive ? (
        <ActionButton label="Pause" icon={<Pause size={14} />} disabled={isBusy} onClick={() => void runAction(() => pauseJob(job.id))} />
      ) : null}
      {isPaused ? (
        <ActionButton label="Resume" icon={<Play size={14} />} disabled={isBusy} primary onClick={() => void runAction(() => resumeJob(job.id))} />
      ) : null}
      {isFailed ? (
        <ActionButton label="Retry" icon={<RotateCw size={14} />} disabled={isBusy} primary onClick={() => void runAction(() => retryJob(job.id))} />
      ) : null}
      {(isActive || isPaused) ? (
        <ActionButton label={cancelLabel} icon={<X size={14} />} disabled={isBusy} destructive={isConfirmingCancel} danger={!isConfirmingCancel} onClick={onCancelClick} />
      ) : null}
      {(isCompleted || isFailed) ? (
        <ActionButton label="Close" icon={<X size={14} />} disabled={isBusy} onClick={onClose} />
      ) : null}
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
      className={`flex h-6 items-center gap-1 rounded px-1.5 text-[11px] font-medium transition disabled:cursor-not-allowed disabled:opacity-50 ${buttonClass}`}
    >
      {icon}
      {label}
    </button>
  );
}

function statusText(job: DownloadJob) {
  if (isTorrentMetadataPending(job)) return 'Finding metadata';

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

function statusClass(job: DownloadJob) {
  if (job.state === JobState.Completed) return 'border-success/40 bg-success/10 text-success';
  if (job.state === JobState.Failed) return 'border-destructive/40 bg-destructive/10 text-destructive';
  if (job.state === JobState.Queued || isTorrentMetadataPending(job)) return 'border-warning/40 bg-warning/10 text-warning';
  if (job.state === JobState.Paused || job.state === JobState.Canceled) return 'border-border bg-muted text-muted-foreground';
  return 'border-primary/40 bg-primary/10 text-primary';
}

function progressColor(job: DownloadJob) {
  if (job.state === JobState.Completed) return 'bg-success';
  if (job.state === JobState.Failed) return 'bg-destructive';
  if (job.state === JobState.Queued || isTorrentMetadataPending(job)) return 'bg-warning';
  if (job.transferKind === 'torrent') return 'bg-warning';
  return 'bg-primary';
}

function downloadedText(job: DownloadJob) {
  return `${formatBytes(job.downloadedBytes)} / ${job.totalBytes > 0 ? formatBytes(job.totalBytes) : 'Unknown'}`;
}

function verifiedTorrentText(job: DownloadJob) {
  return formatTorrentVerifiedSize(job, formatBytes);
}

function torrentFetchedText(job: DownloadJob) {
  return formatTorrentFetchedSize(job, formatBytes);
}

function torrentStatusLine(job: DownloadJob) {
  if (isTorrentMetadataPending(job)) return 'Finding metadata';
  if (job.state === JobState.Seeding) return `Seeding - ${verifiedTorrentText(job)}`;
  const activity = torrentActivitySummary(job);
  return activity === 'No peer activity yet' ? statusText(job) : `${statusText(job)} - ${activity}`;
}

function torrentPeerCount(job: DownloadJob) {
  return typeof job.torrent?.peers === 'number' ? job.torrent.peers.toLocaleString() : '--';
}

function torrentSourceText(job: DownloadJob) {
  if (job.url.startsWith('magnet:')) return 'Magnet link';
  return job.url;
}

function clampProgress(progress: number) {
  if (!Number.isFinite(progress)) return 0;
  return Math.max(0, Math.min(100, progress));
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}
