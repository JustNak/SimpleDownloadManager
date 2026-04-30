import React from 'react';
import {
  ArrowDown,
  ArrowUp,
  Clock,
  ExternalLink,
  FileText,
  FolderOpen,
  Gauge,
  Globe2,
  HardDrive,
  Leaf,
  Link2,
  Magnet,
  Pause,
  Play,
  RotateCw,
  Users,
  X,
} from 'lucide-react';
import {
  openJobFile,
  pauseJob,
  resumeJob,
  retryJob,
  revealJobInFolder,
  swapFailedDownloadToBrowser,
} from './backend';
import { formatBytes, formatTime } from './popupShared';
import { canSwapFailedDownloadToBrowser } from './queueCommands';
import {
  formatTorrentProgressStripText,
  isTorrentCheckingFiles,
  isTorrentMetadataPending,
  isTorrentSeedingRestore,
  torrentDisplayName,
} from './queueRowPresentation';
import {
  buildTorrentPeerHealthDots,
  torrentConnectedText,
  torrentFilesText,
  torrentInfoHash,
  torrentRemainingText,
  torrentSourceSummary,
  type TorrentPeerHealthTone,
} from './torrentProgressPresentation';
import { JobState, type DownloadJob } from './types';
import {
  useProgressPopup,
  type PopupActionRunner,
} from './useProgressPopup';
import { PopupTitlebar } from './PopupTitlebar';

export function TorrentProgressWindow() {
  const popup = useProgressPopup();

  if (!popup.job) {
    return (
      <TorrentShell>
        <div className="flex flex-1 items-center justify-center px-8 text-center text-sm text-muted-foreground">
          This torrent is no longer available.
        </div>
      </TorrentShell>
    );
  }

  return (
    <TorrentShell>
      <TorrentMain
        job={popup.job}
        progress={popup.progress}
        downloadSpeed={popup.progressMetrics.averageSpeed}
        timeRemaining={popup.progressMetrics.timeRemaining}
        isBusy={popup.isBusy}
        isConfirmingCancel={popup.isConfirmingCancel}
        errorMessage={popup.errorMessage}
        runAction={popup.runAction}
        onCancelClick={popup.onCancelClick}
        onClose={popup.onClose}
      />
    </TorrentShell>
  );
}

function TorrentShell({ children }: { children: React.ReactNode }) {
  return (
    <div className="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
      <PopupTitlebar title="Torrent session" />
      {children}
    </div>
  );
}

function TorrentMain({
  job,
  progress,
  downloadSpeed,
  timeRemaining,
  isBusy,
  isConfirmingCancel,
  errorMessage,
  runAction,
  onCancelClick,
  onClose,
}: {
  job: DownloadJob;
  progress: number;
  downloadSpeed: number;
  timeRemaining: number;
  isBusy: boolean;
  isConfirmingCancel: boolean;
  errorMessage: string;
  runAction: PopupActionRunner;
  onCancelClick: () => void;
  onClose: () => void;
}) {
  const stripText = formatTorrentProgressStripText(job, progress, formatBytes);
  const infoHash = torrentInfoHash(job);
  const sourceSummary = torrentSourceSummary(job);

  return (
    <main className="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface">
      <section className="grid grid-cols-[72px_minmax(0,1fr)] gap-4 border-b border-border bg-background px-6 py-2.5">
        <TorrentBadge />
        <div className="min-w-0">
          <div className="flex min-w-0 items-start justify-between gap-4">
            <div className="min-w-0">
              <h1 className="truncate text-xl font-semibold leading-7 text-foreground" title={torrentDisplayName(job)}>
                {torrentDisplayName(job)}
              </h1>
              <div className="mt-1.5 flex min-w-0 items-center gap-3">
                <StatusChip job={job} />
                <span className="truncate text-sm text-muted-foreground" title={torrentSubtitle(job)}>
                  {torrentSubtitle(job)}
                </span>
              </div>
            </div>
          </div>

          <div className="mt-2 flex min-w-0 items-center gap-4 text-xs text-muted-foreground">
            <div className="flex min-w-0 items-center gap-2">
              <span className="shrink-0">Info hash:</span>
              <span className="truncate text-foreground" title={infoHash}>{infoHash}</span>
            </div>
            <div className="h-5 w-px shrink-0 bg-border" />
            <div className="flex min-w-0 items-center gap-2">
              <Link2 size={15} className="shrink-0 text-muted-foreground" />
              <span className="truncate" title={sourceSummary}>Source: {sourceSummary}</span>
            </div>
          </div>
        </div>
      </section>

      <section className="border-b border-border bg-background px-6 py-2.5">
        <div className="mb-2 flex items-end justify-between gap-4">
          <div className="flex items-end gap-5">
            <span className="text-2xl font-semibold leading-none text-primary tabular-nums">{stripText.progressLabel}</span>
            <span className="text-base text-foreground tabular-nums">{stripText.bytesText}</span>
          </div>
          <span className="text-sm text-muted-foreground tabular-nums">
            {torrentRemainingText(job, formatBytes)}
          </span>
        </div>
        <SegmentedTorrentProgress progress={progress} />
      </section>

      <section className="px-6 py-2.5">
        <div className="grid grid-cols-6 divide-x divide-border rounded-md border border-border bg-background">
          <TorrentMetric icon={<ArrowDown size={18} />} label="Down" value={formatSpeed(downloadSpeed)} tone="primary" />
          <TorrentMetric icon={<ArrowUp size={18} />} label="Up" value={formatSpeed(torrentUploadSpeed(job))} tone="warning" />
          <TorrentMetric icon={<Clock size={18} />} label="ETA" value={formatTime(timeRemaining)} />
          <TorrentMetric icon={<Users size={18} />} label="Peers" value={torrentPeerValue(job)} />
          <TorrentMetric icon={<Leaf size={18} />} label="Seeds" value={torrentSeedValue(job)} tone="success" />
          <TorrentMetric icon={<Gauge size={18} />} label="Ratio" value={torrentRatioValue(job)} tone="warning" />
        </div>

        <div className="mt-2.5 overflow-hidden rounded-md border border-border bg-background">
          <TorrentDetailRow icon={<Users size={18} />} label="Peer health" value={<PeerHealth job={job} />} trailing={torrentConnectedText(job)} />
          <TorrentDetailRow icon={<FileText size={18} />} label="Files" value={torrentFilesText(job, formatBytes)} />
          <TorrentDetailRow icon={<HardDrive size={18} />} label="Save to" value={job.targetPath || 'No destination recorded yet.'} />
          <TorrentDetailRow icon={<Globe2 size={18} />} label="Source" value={sourceSummary} />
        </div>

        <ErrorMessage message={errorMessage} />
      </section>

      <TorrentActionBar
        job={job}
        isBusy={isBusy}
        isConfirmingCancel={isConfirmingCancel}
        runAction={runAction}
        onCancelClick={onCancelClick}
        onClose={onClose}
      />
    </main>
  );
}

function TorrentBadge() {
  return (
    <div className="flex h-[72px] w-[72px] items-center justify-center rounded-md border border-border bg-surface text-primary shadow-sm">
      <Magnet size={44} strokeWidth={2.2} />
    </div>
  );
}

function StatusChip({ job }: { job: DownloadJob }) {
  return (
    <span className={`inline-flex h-6 shrink-0 items-center gap-2 rounded-md border px-2.5 text-sm font-medium ${statusClass(job)}`}>
      <ArrowDown size={14} />
      {statusText(job)}
    </span>
  );
}

function SegmentedTorrentProgress({ progress }: { progress: number }) {
  const segmentCount = 42;
  const completed = Math.round((Math.max(0, Math.min(100, progress)) / 100) * segmentCount);

  return (
    <div className="grid h-3.5 grid-cols-[repeat(42,minmax(0,1fr))] gap-1">
      {Array.from({ length: segmentCount }, (_, index) => (
        <div
          key={index}
          className={`rounded-sm ${index < completed ? 'bg-primary' : 'bg-progress-track'}`}
        />
      ))}
    </div>
  );
}

function TorrentMetric({
  icon,
  label,
  value,
  tone = 'default',
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  tone?: 'default' | 'primary' | 'warning' | 'success';
}) {
  const valueClass = {
    default: 'text-foreground',
    primary: 'text-primary',
    warning: 'text-warning',
    success: 'text-success',
  }[tone];

  return (
    <div className="min-w-0 px-2.5 py-1.5 text-center">
      <div className="flex items-center justify-center gap-1.5 text-xs text-muted-foreground">
        <span className={tone === 'default' ? 'text-foreground' : valueClass}>{icon}</span>
        <span>{label}</span>
      </div>
      <div className={`mt-0.5 truncate text-lg font-semibold leading-5 tabular-nums ${valueClass}`} title={value}>
        {value}
      </div>
    </div>
  );
}

function TorrentDetailRow({
  icon,
  label,
  value,
  trailing,
}: {
  icon: React.ReactNode;
  label: string;
  value: React.ReactNode;
  trailing?: string;
}) {
  return (
    <div className="grid min-h-[29px] grid-cols-[104px_minmax(0,1fr)_auto] items-center border-b border-border last:border-b-0">
      <div className="flex h-full items-center gap-2 border-r border-border px-3 text-muted-foreground">
        <span className="text-foreground">{icon}</span>
        <span className="text-xs">{label}</span>
      </div>
      <div className="min-w-0 px-4 text-sm text-foreground">
        {typeof value === 'string' ? (
          <div className="truncate" title={value}>{value}</div>
        ) : value}
      </div>
      {trailing ? (
        <div className="flex items-center px-3 text-xs text-muted-foreground">
          <span className="whitespace-nowrap">{trailing}</span>
        </div>
      ) : null}
    </div>
  );
}

function PeerHealth({ job }: { job: DownloadJob }) {
  return (
    <div className="flex items-center gap-2">
      {buildTorrentPeerHealthDots(job).map((dot, index) => (
        <span key={index} className={`h-3 w-3 rounded-full ${peerDotClass(dot.tone)}`} />
      ))}
    </div>
  );
}

function TorrentActionBar({
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
  const isActive = [JobState.Queued, JobState.Starting, JobState.Downloading].includes(job.state);
  const isPaused = job.state === JobState.Paused;
  const isSeeding = job.state === JobState.Seeding;
  const isCompleted = job.state === JobState.Completed;
  const isFailed = job.state === JobState.Failed;
  const canShow = Boolean(job.targetPath);
  const cancelLabel = isConfirmingCancel ? 'Confirm' : 'Cancel';

  return (
    <div className="mt-auto flex shrink-0 justify-end gap-3 border-t border-border bg-background px-6 py-2">
      {(isActive || isPaused || isSeeding) && canShow ? (
        <TorrentActionButton
          label="Show"
          icon={<FolderOpen size={18} />}
          disabled={isBusy}
          onClick={() => void runAction(async () => {
            await revealJobInFolder(job.id);
          }, { closeOnSuccess: true })}
        />
      ) : null}
      {isActive ? (
        <TorrentActionButton label="Pause" icon={<Pause size={18} />} disabled={isBusy} primary onClick={() => void runAction(() => pauseJob(job.id))} />
      ) : null}
      {isPaused ? (
        <TorrentActionButton label="Resume" icon={<Play size={18} />} disabled={isBusy} primary onClick={() => void runAction(() => resumeJob(job.id))} />
      ) : null}
      {isCompleted ? (
        <TorrentActionButton
          label="Open"
          icon={<ExternalLink size={18} />}
          disabled={isBusy}
          primary
          onClick={() => void runAction(async () => {
            await openJobFile(job.id);
          }, { closeOnSuccess: true })}
        />
      ) : null}
      {isFailed ? (
        <TorrentActionButton label="Retry" icon={<RotateCw size={18} />} disabled={isBusy} primary onClick={() => void runAction(() => retryJob(job.id))} />
      ) : null}
      {isFailed && canSwapFailedDownloadToBrowser(job) ? (
        <TorrentActionButton
          label="Swap"
          icon={<ExternalLink size={18} />}
          disabled={isBusy}
          onClick={() => void runAction(() => swapFailedDownloadToBrowser(job.id), { closeOnSuccess: true })}
        />
      ) : null}
      {isCompleted && canShow ? (
        <TorrentActionButton
          label="Show"
          icon={<FolderOpen size={18} />}
          disabled={isBusy}
          onClick={() => void runAction(async () => {
            await revealJobInFolder(job.id);
          }, { closeOnSuccess: true })}
        />
      ) : null}
      {(isActive || isPaused) ? (
        <TorrentActionButton label={cancelLabel} icon={<X size={18} />} disabled={isBusy} destructive={isConfirmingCancel} danger={!isConfirmingCancel} onClick={onCancelClick} />
      ) : null}
      {(isCompleted || isFailed) ? (
        <TorrentActionButton label="Close" icon={<X size={18} />} disabled={isBusy} onClick={onClose} />
      ) : null}
    </div>
  );
}

function TorrentActionButton({
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
    ? 'border border-destructive bg-destructive text-destructive-foreground hover:bg-destructive/90'
    : danger
      ? 'border border-destructive text-destructive hover:bg-destructive hover:text-destructive-foreground'
      : primary
        ? 'border border-primary bg-background text-primary hover:bg-primary-soft'
        : 'border border-input bg-background text-foreground hover:bg-muted';

  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`flex h-8 min-w-[128px] items-center justify-center gap-2.5 rounded-md px-5 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-50 ${buttonClass}`}
    >
      {icon}
      {label}
    </button>
  );
}

function ErrorMessage({ message }: { message: string }) {
  if (!message) return null;

  return (
    <div className="mt-3 truncate rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" title={message}>
      {message}
    </div>
  );
}

function statusText(job: DownloadJob) {
  if (isTorrentSeedingRestore(job)) return 'Restoring seeding';
  if (isTorrentCheckingFiles(job)) return 'Checking files';
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

function torrentSubtitle(job: DownloadJob) {
  const peers = job.torrent?.peers;
  if (job.state === JobState.Downloading && typeof peers === 'number') {
    return `${statusText(job)} - ${peers.toLocaleString()} peers active`;
  }
  return statusText(job);
}

function statusClass(job: DownloadJob) {
  if (job.state === JobState.Completed) return 'border-success/40 bg-success/10 text-success';
  if (job.state === JobState.Failed) return 'border-destructive/40 bg-destructive/10 text-destructive';
  if (job.state === JobState.Paused || job.state === JobState.Canceled) return 'border-border bg-muted text-muted-foreground';
  if (job.state === JobState.Queued || isTorrentSeedingRestore(job) || isTorrentCheckingFiles(job) || isTorrentMetadataPending(job)) {
    return 'border-warning/40 bg-warning/10 text-warning';
  }
  return 'border-primary/40 bg-primary/10 text-primary';
}

function peerDotClass(tone: TorrentPeerHealthTone) {
  if (tone === 'success') return 'bg-success';
  if (tone === 'warning') return 'bg-warning';
  return 'bg-progress-track';
}

function torrentUploadSpeed(job: DownloadJob) {
  if (typeof job.torrent?.diagnostics?.sessionUploadSpeed === 'number') {
    return Math.max(0, job.torrent.diagnostics.sessionUploadSpeed);
  }
  if (job.state === JobState.Seeding) return Math.max(0, job.speed || 0);
  return 0;
}

function torrentPeerValue(job: DownloadJob) {
  return typeof job.torrent?.peers === 'number' ? job.torrent.peers.toLocaleString() : '--';
}

function torrentSeedValue(job: DownloadJob) {
  return typeof job.torrent?.seeds === 'number' ? job.torrent.seeds.toLocaleString() : '--';
}

function torrentRatioValue(job: DownloadJob) {
  return typeof job.torrent?.ratio === 'number' ? job.torrent.ratio.toFixed(2) : '--';
}

function formatSpeed(bytesPerSecond: number) {
  return bytesPerSecond > 0 ? `${formatBytes(bytesPerSecond)}/s` : '--';
}
