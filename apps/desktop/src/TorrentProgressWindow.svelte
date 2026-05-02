<script lang="ts">
  import { ExternalLink, FolderOpen, Magnet, Pause, Play, RotateCw, X } from '@lucide/svelte';
  import FileBadge from './FileBadge.svelte';
  import PopupTitlebar from './PopupTitlebar.svelte';
  import { JobState, type DownloadJob } from './types';
  import { openJobFile, pauseJob, resumeJob, retryJob, revealJobInFolder } from './backend';
  import { formatBytes, formatTime, getHost } from './popupShared';
  import { useProgressPopup } from './useProgressPopup.svelte';

  const popup = useProgressPopup();

  function torrentRatio(job: DownloadJob): string {
    return typeof job.torrent?.ratio === 'number' ? job.torrent.ratio.toFixed(2) : '0.00';
  }
</script>

{#if !popup.job}
  <div class="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
    <PopupTitlebar title="Torrent session" />
    <div class="flex flex-1 items-center justify-center px-6 text-center text-sm text-muted-foreground">This torrent session is no longer available.</div>
  </div>
{:else}
  {@const job = popup.job}
  <div class="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
    <PopupTitlebar title="Torrent session" />
    <main class="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface p-4">
      <section class="flex items-start gap-3">
        <FileBadge filename={job.filename} transferKind="torrent" size="lg" />
        <div class="min-w-0 flex-1">
          <h1 class="truncate text-lg font-semibold" title={job.filename}>{job.filename}</h1>
          <div class="truncate text-xs text-muted-foreground" title={job.url}>{getHost(job.url)}</div>
          <div class="mt-2 flex flex-wrap gap-2 text-[11px]">
            <span class="rounded border border-border bg-background px-2 py-1">{job.torrent?.peers ?? 0} peers</span>
            <span class="rounded border border-border bg-background px-2 py-1">{job.torrent?.seeds ?? 0} seeds</span>
            <span class="rounded border border-border bg-background px-2 py-1">Ratio {torrentRatio(job)}</span>
          </div>
        </div>
        <div class="text-right">
          <div class="text-3xl font-semibold tabular-nums">{popup.progress.toFixed(0)}%</div>
          <div class="text-xs capitalize text-muted-foreground">{job.state}</div>
        </div>
      </section>

      <section class="mt-5">
        <div class="h-2 overflow-hidden rounded-full bg-progress-track">
          <div class="h-2 rounded-full bg-warning transition-all duration-300" style={`width: ${popup.progress}%`}></div>
        </div>
      </section>

      <section class="mt-5 grid grid-cols-4 gap-3">
        {@render Metric('Downloaded', formatBytes(job.downloadedBytes))}
        {@render Metric('Uploaded', formatBytes(job.torrent?.uploadedBytes ?? 0))}
        {@render Metric('Speed', job.speed > 0 ? `${formatBytes(job.speed)}/s` : '--')}
        {@render Metric('ETA', formatTime(job.eta))}
      </section>

      <section class="mt-5 min-h-0 flex-1 overflow-hidden rounded border border-border bg-background/40 p-3">
        <div class="mb-2 flex items-center gap-2 text-sm font-semibold"><Magnet size={16} /> Torrent details</div>
        <div class="grid grid-cols-[130px_minmax(0,1fr)] gap-y-2 text-xs">
          <div class="text-muted-foreground">Info hash</div>
          <div class="truncate font-mono" title={job.torrent?.infoHash}>{job.torrent?.infoHash ?? 'Not available'}</div>
          <div class="text-muted-foreground">Files</div>
          <div>{job.torrent?.totalFiles ?? 1}</div>
          <div class="text-muted-foreground">Target</div>
          <div class="truncate" title={job.targetPath}>{job.targetPath}</div>
          <div class="text-muted-foreground">Source</div>
          <div class="truncate text-primary" title={job.url}>{job.url}</div>
        </div>
      </section>

      {#if popup.errorMessage}
        <div class="mt-3 rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">{popup.errorMessage}</div>
      {/if}

      <footer class="mt-4 flex justify-end gap-2 border-t border-border pt-3">
        {#if job.state === JobState.Paused}
          {@render Action('Resume', Play, () => void popup.runAction(() => resumeJob(job.id)), true)}
        {:else if job.state === JobState.Downloading || job.state === JobState.Seeding || job.state === JobState.Queued}
          {@render Action('Pause', Pause, () => void popup.runAction(() => pauseJob(job.id)))}
        {/if}
        {#if job.state === JobState.Failed}
          {@render Action('Retry', RotateCw, () => void popup.runAction(() => retryJob(job.id)), true)}
        {/if}
        {#if job.state === JobState.Completed}
          {@render Action('Open', ExternalLink, () => void popup.runAction(async () => { await openJobFile(job.id); }))}
        {/if}
        {@render Action('Reveal', FolderOpen, () => void popup.runAction(async () => { await revealJobInFolder(job.id); }))}
        {@render Action(popup.isConfirmingCancel ? 'Confirm cancel' : 'Cancel', X, popup.onCancelClick, false, true)}
        {@render Action('Close', X, popup.onClose)}
      </footer>
    </main>
  </div>
{/if}

{#snippet Metric(label: string, value: string)}
  <div class="rounded border border-border bg-background p-3">
    <div class="text-[11px] text-muted-foreground">{label}</div>
    <div class="mt-1 truncate text-sm font-semibold tabular-nums" title={value}>{value}</div>
  </div>
{/snippet}

{#snippet Action(label: string, icon: typeof X, onClick: () => void, primary = false, danger = false)}
  {@const Icon = icon}
  <button class={`inline-flex items-center gap-1.5 rounded border px-3 py-1.5 text-xs font-semibold ${primary ? 'border-primary bg-primary text-primary-foreground' : danger ? 'border-destructive/50 text-destructive hover:bg-destructive/10' : 'border-border hover:bg-muted'}`} onclick={onClick} disabled={popup.isBusy}>
    <Icon size={14} /> {label}
  </button>
{/snippet}
