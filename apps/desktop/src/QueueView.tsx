import React from 'react';
import { JobState } from './types';
import type { DownloadJob } from './types';
import {
  Box,
  Check,
  Clock3,
  Download,
  FileArchive,
  FileAudio,
  FileCode,
  FileImage,
  FileText,
  FileVideo,
  FolderOpen,
  Globe,
  HardDrive,
  Pause,
  Play,
  RotateCw,
  Trash2,
  X,
} from 'lucide-react';

interface QueueViewProps {
  jobs: DownloadJob[];
  view: string;
  selectedJobId: string | null;
  onSelect: (id: string) => void;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
  onOpen: (id: string) => void;
  onReveal: (id: string) => void;
}

export function QueueView({
  jobs,
  view,
  selectedJobId,
  onSelect,
  onPause,
  onResume,
  onCancel,
  onRetry,
  onRemove,
  onOpen,
  onReveal,
}: QueueViewProps) {
  const selectedJob = jobs.find((job) => job.id === selectedJobId) ?? jobs[0] ?? null;

  if (jobs.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center bg-surface p-8">
        <div className="max-w-sm text-center">
          <div className="mx-auto mb-5 flex h-16 w-16 items-center justify-center rounded-md border border-border bg-card text-primary">
            <Download size={32} />
          </div>
          <h2 className="mb-2 text-lg font-semibold text-foreground">No {view === 'all' ? '' : view} downloads</h2>
          <p className="text-sm leading-6 text-muted-foreground">
            Downloads from the browser extension or the New Download command will appear in this list.
          </p>
        </div>
      </div>
    );
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col bg-surface">
      <div className="min-h-0 flex-1 overflow-auto px-2 py-2">
        <div className="download-table min-w-[960px] overflow-hidden rounded-sm border border-border bg-card">
          <div className="grid grid-cols-[minmax(280px,2.2fr)_150px_180px_110px_100px_150px_96px] border-b border-border bg-header px-5 py-3 text-sm text-muted-foreground">
            <div>Name</div>
            <div>Status</div>
            <div>Progress</div>
            <div>Speed</div>
            <div>ETA</div>
            <div>Size</div>
            <div className="text-right">Actions</div>
          </div>

          <div className="divide-y divide-border/70">
            {jobs.map((job) => {
              const selected = job.id === selectedJob?.id;
              return (
                <div
                  key={job.id}
                  onClick={() => onSelect(job.id)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault();
                      onSelect(job.id);
                    }
                  }}
                  role="button"
                  tabIndex={0}
                  className={`grid min-h-[74px] w-full grid-cols-[minmax(280px,2.2fr)_150px_180px_110px_100px_150px_96px] items-center gap-0 px-5 py-3 text-left text-sm transition ${
                    selected ? 'bg-selected outline outline-1 outline-primary/30' : 'bg-card hover:bg-row-hover'
                  }`}
                >
                  <div className="flex min-w-0 items-center gap-4 pr-4">
                    <FileBadge filename={job.filename} />
                    <div className="min-w-0">
                      <div className="truncate text-[15px] font-semibold text-foreground" title={job.filename}>
                        {job.filename}
                      </div>
                      <div className="mt-1 truncate text-sm text-muted-foreground" title={job.url}>
                        {getHost(job.url)}
                      </div>
                    </div>
                  </div>

                  <div className={`font-medium ${statusClass(job.state)}`}>{statusText(job.state)}</div>

                  <div className="pr-6">
                    <div className="mb-2 text-[15px] font-medium text-foreground">{formatProgress(job)}</div>
                    <ProgressBar job={job} />
                  </div>

                  <div className="tabular-nums text-muted-foreground">
                    {job.state === JobState.Downloading ? `${formatBytes(job.speed)}/s` : '--'}
                  </div>
                  <div className="tabular-nums text-muted-foreground">
                    {job.state === JobState.Downloading ? formatTime(job.eta) : '--'}
                  </div>
                  <div className="tabular-nums text-muted-foreground">
                    {job.totalBytes > 0 ? `${formatBytes(job.downloadedBytes)} / ${formatBytes(job.totalBytes)}` : formatBytes(job.downloadedBytes)}
                  </div>

                  <div className="flex items-center justify-end gap-2" onClick={(event) => event.stopPropagation()}>
                    <RowActions
                      job={job}
                      onPause={onPause}
                      onResume={onResume}
                      onCancel={onCancel}
                      onRetry={onRetry}
                      onRemove={onRemove}
                      onReveal={onReveal}
                    />
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {selectedJob ? (
        <DownloadDetailsPane
          job={selectedJob}
          onPause={onPause}
          onResume={onResume}
          onCancel={onCancel}
          onRetry={onRetry}
          onRemove={onRemove}
          onOpen={onOpen}
          onReveal={onReveal}
        />
      ) : null}
    </section>
  );
}

function DownloadDetailsPane({
  job,
  onPause,
  onResume,
  onCancel,
  onRetry,
  onRemove,
  onOpen,
  onReveal,
}: {
  job: DownloadJob;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
  onOpen: (id: string) => void;
  onReveal: (id: string) => void;
}) {
  const sourceLabel = job.source
    ? `${job.source.browser} ${job.source.entryPoint.replaceAll('_', ' ')}`
    : 'Manual URL';

  return (
    <aside className="details-pane min-h-[190px] shrink-0 border-t border-border bg-card">
      <div className="flex min-w-0 gap-8 px-8 py-7">
        <div className="flex w-28 shrink-0 justify-center">
          <FileBadge filename={job.filename} large />
        </div>

        <div className="min-w-0 flex-1">
          <h3 className="mb-4 truncate text-lg font-semibold text-foreground" title={job.filename}>
            {job.filename}
          </h3>
          <div className="grid max-w-[760px] grid-cols-[118px_minmax(0,1fr)] gap-x-4 gap-y-3 text-sm">
            <DetailLabel icon={<Globe size={16} />} label="Source URL:" />
            <DetailValue value={job.url} accent />

            <DetailLabel icon={<FolderOpen size={16} />} label="Destination:" />
            <DetailValue value={job.targetPath || 'No destination recorded yet.'} />

            <DetailLabel icon={<HardDrive size={16} />} label="File Size:" />
            <DetailValue value={job.totalBytes > 0 ? `${formatBytes(job.totalBytes)} (${job.totalBytes.toLocaleString()} bytes)` : 'Unknown'} />

            <DetailLabel icon={<Download size={16} />} label="Downloaded:" />
            <DetailValue value={`${formatBytes(job.downloadedBytes)} (${job.downloadedBytes.toLocaleString()} bytes)`} />

            <DetailLabel icon={<Clock3 size={16} />} label="Time Remaining:" />
            <DetailValue value={job.state === JobState.Downloading ? formatTime(job.eta) : '--'} />

            <DetailLabel icon={<Check size={16} />} label="Status:" />
            <DetailValue value={statusText(job.state)} />

            <DetailLabel icon={<Globe size={16} />} label="Source:" />
            <DetailValue value={sourceLabel} />
          </div>
        </div>
      </div>
    </aside>
  );
}

function RowActions({
  job,
  onPause,
  onResume,
  onCancel,
  onRetry,
  onRemove,
  onReveal,
}: {
  job: DownloadJob;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
  onReveal: (id: string) => void;
}) {
  return (
    <>
      {[JobState.Queued, JobState.Starting, JobState.Downloading].includes(job.state) ? (
        <IconButton title="Pause" onClick={() => onPause(job.id)}><Pause size={17} /></IconButton>
      ) : null}
      {job.state === JobState.Paused ? (
        <IconButton title="Resume" onClick={() => onResume(job.id)}><Play size={17} /></IconButton>
      ) : null}
      {[JobState.Failed, JobState.Canceled].includes(job.state) ? (
        <IconButton title="Retry" onClick={() => onRetry(job.id)}><RotateCw size={17} /></IconButton>
      ) : null}
      {job.targetPath ? (
        <IconButton title="Open folder" onClick={() => onReveal(job.id)}><FolderOpen size={17} /></IconButton>
      ) : null}
      {![JobState.Completed, JobState.Canceled, JobState.Failed].includes(job.state) ? (
        <IconButton title="Cancel" onClick={() => onCancel(job.id)}><X size={17} /></IconButton>
      ) : null}
      <IconButton title="Remove" onClick={() => onRemove(job.id)}><Trash2 size={17} /></IconButton>
    </>
  );
}

function FileBadge({ filename, large = false }: { filename: string; large?: boolean }) {
  const ext = filename.split('.').pop()?.toLowerCase() || '';
  const iconSize = large ? 34 : 22;
  const icon = getFileIcon(ext, iconSize);
  const label = ext ? ext.slice(0, 4).toUpperCase() : 'FILE';

  return (
    <div className={`file-badge relative flex shrink-0 items-center justify-center rounded-sm border border-border bg-background ${large ? 'h-[100px] w-[76px]' : 'h-[52px] w-10'}`}>
      <div className="absolute right-0 top-0 h-3 w-3 border-b border-l border-border bg-surface" />
      <div className="text-primary">{icon}</div>
      {large ? <div className="absolute bottom-2 text-[10px] font-semibold text-muted-foreground">{label}</div> : null}
    </div>
  );
}

function getFileIcon(ext: string, size: number) {
  if (['mp4', 'mkv', 'avi', 'mov', 'webm'].includes(ext)) return <FileVideo size={size} />;
  if (['mp3', 'wav', 'flac', 'ogg', 'm4a'].includes(ext)) return <FileAudio size={size} />;
  if (['jpg', 'jpeg', 'png', 'gif', 'webp'].includes(ext)) return <FileImage size={size} />;
  if (['zip', 'rar', '7z', 'tar', 'gz'].includes(ext)) return <FileArchive size={size} />;
  if (['exe', 'msi', 'apk', 'dmg', 'pkg', 'deb'].includes(ext)) return <Box size={size} />;
  if (['js', 'ts', 'json', 'html', 'css'].includes(ext)) return <FileCode size={size} />;
  return <FileText size={size} />;
}

function ProgressBar({ job, large = false }: { job: DownloadJob; large?: boolean }) {
  const color =
    job.state === JobState.Completed
      ? 'bg-success'
      : job.state === JobState.Failed
        ? 'bg-destructive'
        : job.state === JobState.Queued
          ? 'bg-warning'
          : 'bg-primary';

  return (
    <div className={`${large ? 'h-2' : 'h-1'} w-full overflow-hidden rounded-full bg-progress-track`}>
      <div className={`${large ? 'h-2' : 'h-1'} rounded-full ${color} transition-all duration-300`} style={{ width: `${Math.max(0, Math.min(100, job.progress))}%` }} />
    </div>
  );
}

function DetailLabel({ icon, label }: { icon: React.ReactNode; label: string }) {
  return (
    <div className="flex items-center gap-2 text-muted-foreground">
      {icon}
      <span>{label}</span>
    </div>
  );
}

function DetailValue({ value, accent = false }: { value: string; accent?: boolean }) {
  return (
    <div className={`min-w-0 truncate ${accent ? 'text-primary' : 'text-foreground'}`} title={value}>
      {value}
    </div>
  );
}

function Metric({ icon, value, label }: { icon: React.ReactNode; value: string; label: string }) {
  return (
    <div className="flex flex-col items-center gap-1 px-4 text-center">
      <div className="mb-1 text-primary">{icon}</div>
      <div className="text-sm font-medium tabular-nums text-foreground">{value}</div>
      <div className="text-sm text-muted-foreground">{label}</div>
    </div>
  );
}

function IconButton({ title, onClick, children }: { title: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      title={title}
      aria-label={title}
      className="flex h-8 w-8 items-center justify-center rounded-md border border-transparent bg-transparent text-muted-foreground transition hover:border-border hover:bg-muted hover:text-foreground"
    >
      {children}
    </button>
  );
}

function formatProgress(job: DownloadJob) {
  if (job.state === JobState.Queued) return '0%';
  if (job.state === JobState.Canceled) return '--';
  return `${job.progress.toFixed(0)}%`;
}

function statusText(state: JobState) {
  switch (state) {
    case JobState.Downloading:
      return 'Downloading';
    case JobState.Paused:
      return 'Paused';
    case JobState.Completed:
      return 'Completed';
    case JobState.Failed:
      return 'Error';
    case JobState.Canceled:
      return 'Canceled';
    case JobState.Starting:
      return 'Starting';
    case JobState.Queued:
      return 'Queued';
    default:
      return state;
  }
}

function statusClass(state: JobState) {
  switch (state) {
    case JobState.Downloading:
    case JobState.Starting:
      return 'text-primary';
    case JobState.Completed:
      return 'text-success';
    case JobState.Failed:
      return 'text-destructive';
    case JobState.Queued:
      return 'text-warning';
    case JobState.Paused:
      return 'text-muted-foreground';
    default:
      return 'text-muted-foreground';
  }
}

function getHost(rawUrl: string) {
  try {
    return new URL(rawUrl).host;
  } catch {
    return rawUrl;
  }
}

function formatBytes(bytes: number, decimals = 1) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(decimals))} ${sizes[i]}`;
}

function formatTime(seconds: number) {
  if (!Number.isFinite(seconds) || seconds <= 0) return '--';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.round(seconds % 60);
  if (minutes < 60) return `${minutes}m ${remainingSeconds}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}
