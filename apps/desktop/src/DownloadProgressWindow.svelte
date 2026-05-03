<script lang="ts">
  import { ExternalLink, FolderOpen, Pause, Play, RotateCw, X } from '@lucide/svelte';
  import FileBadge from './FileBadge.svelte';
  import PopupTitlebar from './PopupTitlebar.svelte';
  import { JobState, type DownloadJob } from './types';
  import { openJobFile, pauseJob, resumeJob, retryJob, revealJobInFolder, swapFailedDownloadToBrowser } from './backend';
  import { canSwapFailedDownloadToBrowser } from './queueCommands';
  import { formatBytes, formatTime, getHost } from './popupShared';
  import { useProgressPopup, type PopupActionRunner } from './useProgressPopup.svelte';
  import type { DownloadProgressMetrics } from './downloadProgressMetrics';

  const popup = useProgressPopup();

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

  function statusText(job: DownloadJob) {
    if (job.state === JobState.Downloading) return 'Downloading';
    if (job.state === JobState.Paused) return 'Paused';
    if (job.state === JobState.Completed) return 'Complete';
    if (job.state === JobState.Failed) return 'Failed';
    return job.state;
  }

  function progressColor(job: DownloadJob) {
    if (job.state === JobState.Failed) return 'bg-destructive';
    if (job.state === JobState.Completed) return 'bg-success';
    return 'bg-primary';
  }

  function downloadedText(job: DownloadJob) {
    return job.totalBytes > 0
      ? `${formatBytes(job.downloadedBytes)} / ${formatBytes(job.totalBytes)}`
      : formatBytes(job.downloadedBytes);
  }
</script>

{#if !popup.job}
  <div class="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
    <PopupTitlebar title="Download progress" />
    <div class="flex flex-1 items-center justify-center px-6 text-center text-sm text-muted-foreground">This download is no longer available.</div>
  </div>
{:else}
  {@render ProgressView(
    popup.job,
    popup.progress,
    popup.progressMetrics,
    popup.isBusy,
    popup.isConfirmingCancel,
    popup.errorMessage,
    popup.runAction,
    popup.onCancelClick,
    popup.onClose,
  )}
{/if}

{#snippet ProgressView(
  job: DownloadJob,
  progress: number,
  progressMetrics: DownloadProgressMetrics,
  isBusy: boolean,
  isConfirmingCancel: boolean,
  errorMessage: string,
  runAction: PopupActionRunner,
  onCancelClick: () => void,
  onClose: () => void,
)}
  <div class="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
    <PopupTitlebar title="Download progress" />
    <main class="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface px-3 py-1.5">
      <section class="flex min-w-0 items-start gap-2">
        <FileBadge filename={job.filename} transferKind={job.transferKind} />
        <div class="min-w-0 flex-1">
          <div class="flex min-w-0 items-start justify-between gap-2">
            <div class="min-w-0">
              <h1 class="truncate text-sm font-semibold leading-5 text-foreground" title={job.filename}>{job.filename}</h1>
              <div class="truncate text-[11px] text-muted-foreground" title={job.url}>{getHost(job.url)}</div>
            </div>
            <span class="shrink-0 rounded border border-border bg-muted px-1.5 py-0.5 text-[10px] font-semibold leading-4">{statusText(job)}</span>
          </div>
        </div>
      </section>

      <section class="mt-1.5">
        <div class="mb-1 flex items-end justify-between gap-2">
          <span class="text-xl font-semibold tabular-nums leading-none text-foreground">{progress.toFixed(0)}%</span>
          <span class="truncate text-[11px] tabular-nums text-muted-foreground" title={downloadedText(job)}>{downloadedText(job)}</span>
        </div>
        <div class="h-1.5 overflow-hidden rounded-full bg-progress-track">
          <div class={`h-1.5 rounded-full transition-[width,background-color] duration-300 ${progressColor(job)}`} style={`width: ${progress}%`}></div>
        </div>
      </section>

      <section class="mt-1.5 grid grid-cols-3 gap-2 border-t border-border/35 bg-background/30 px-2 py-1">
        {@render Metric('Speed', job.state === JobState.Downloading ? `${formatBytes(progressMetrics.averageSpeed)}/s` : '--')}
        {@render Metric('ETA', job.state === JobState.Downloading ? formatTime(progressMetrics.timeRemaining) : '--')}
        {@render Metric('Size', job.totalBytes > 0 ? formatBytes(job.totalBytes) : 'Unknown')}
      </section>

      <div class="mt-1 grid grid-cols-[48px_minmax(0,1fr)] gap-x-1.5 gap-y-0 text-[10px] leading-4">
        <div class="flex items-center gap-1 text-muted-foreground"><FolderOpen size={12} /> Path</div>
        <div class="truncate text-foreground" title={job.targetPath}>{job.targetPath || 'No destination recorded yet.'}</div>
        <div class="flex items-center gap-1 text-muted-foreground"><ExternalLink size={12} /> Source</div>
        <div class="truncate text-primary" title={job.url}>{job.url}</div>
      </div>

      {#if errorMessage}
        <div class="mt-1.5 truncate rounded border border-destructive/40 bg-destructive/10 px-2 py-1 text-[11px] text-destructive" title={errorMessage}>{errorMessage}</div>
      {/if}

      <div class="mt-auto flex justify-end gap-2 border-t border-border pt-2">
        {#if job.state === JobState.Paused}{@render Action('Resume', Play, isBusy, () => void runAction(() => resumeJob(job.id)), true)}{/if}
        {#if job.state === JobState.Downloading || job.state === JobState.Queued || job.state === JobState.Starting}{@render Action('Pause', Pause, isBusy, () => void runAction(() => pauseJob(job.id)))}{/if}
        {#if job.state === JobState.Failed}{@render Action('Retry', RotateCw, isBusy, () => void runAction(() => retryJob(job.id)), true)}{/if}
        {#if canSwapFailedDownloadToBrowser(job)}{@render Action('Open in browser', ExternalLink, isBusy, () => void runAction(() => swapFailedDownloadToBrowser(job.id), { closeOnSuccess: true }))}{/if}
        {@render Action('Reveal', FolderOpen, isBusy, () => void runAction(async () => { await revealJobInFolder(job.id); }))}
        {@render Action('Open', ExternalLink, isBusy, () => void runAction(async () => { await openJobFile(job.id); }))}
        {@render Action(isConfirmingCancel ? 'Confirm cancel' : 'Cancel', X, isBusy, onCancelClick, false, true)}
        {@render Action('Close', X, isBusy, onClose)}
      </div>
    </main>
  </div>
{/snippet}

{#snippet Metric(label: string, value: string)}
  <div class="min-w-0">
    <div class="text-[10px] leading-3 text-muted-foreground">{label}</div>
    <div class="truncate text-xs font-semibold tabular-nums leading-4 text-foreground" title={value}>{value}</div>
  </div>
{/snippet}

{#snippet Action(label: string, icon: typeof X, disabled: boolean, onClick: () => void, primary = false, danger = false)}
  {@const Icon = icon}
  <button
    class={`inline-flex items-center gap-1.5 rounded border px-2 py-1 text-[11px] font-semibold disabled:opacity-45 ${primary ? 'border-primary bg-primary text-primary-foreground' : danger ? 'border-destructive/50 text-destructive hover:bg-destructive/10' : 'border-border hover:bg-muted'}`}
    {disabled}
    onclick={onClick}
  >
    <Icon size={13} /> {label}
  </button>
{/snippet}
