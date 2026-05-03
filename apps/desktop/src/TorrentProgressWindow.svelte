<script lang="ts">
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
  } from '@lucide/svelte';
  import PopupTitlebar from './PopupTitlebar.svelte';
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
  import { useProgressPopup } from './useProgressPopup.svelte';

  type IconComponent = typeof X;
  type MetricTone = 'default' | 'primary' | 'warning' | 'success';

  const popup = useProgressPopup();
  const segmentCount = 42;

  function completedSegments(progress: number) {
    const clampedProgress = Math.max(0, Math.min(100, Number.isFinite(progress) ? progress : 0));
    return Math.round((clampedProgress / 100) * segmentCount);
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

  function metricValueClass(tone: MetricTone) {
    if (tone === 'primary') return 'text-primary';
    if (tone === 'warning') return 'text-warning';
    if (tone === 'success') return 'text-success';
    return 'text-foreground';
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

  function isActive(job: DownloadJob) {
    return [JobState.Queued, JobState.Starting, JobState.Downloading].includes(job.state);
  }

  function isPaused(job: DownloadJob) {
    return job.state === JobState.Paused;
  }

  function isSeeding(job: DownloadJob) {
    return job.state === JobState.Seeding;
  }

  function isCompleted(job: DownloadJob) {
    return job.state === JobState.Completed;
  }

  function isFailed(job: DownloadJob) {
    return job.state === JobState.Failed;
  }

  function actionClass(primary: boolean, danger: boolean, destructive: boolean) {
    if (destructive) return 'border border-destructive bg-destructive text-destructive-foreground hover:bg-destructive/90';
    if (danger) return 'border border-destructive text-destructive hover:bg-destructive hover:text-destructive-foreground';
    if (primary) return 'border border-primary bg-background text-primary hover:bg-primary-soft';
    return 'border border-input bg-background text-foreground hover:bg-muted';
  }
</script>

{#if !popup.job}
  <div class="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
    <PopupTitlebar title="Torrent session" />
    <div class="flex flex-1 items-center justify-center px-8 text-center text-sm text-muted-foreground">This torrent is no longer available.</div>
  </div>
{:else}
  {@const job = popup.job}
  {@const stripText = formatTorrentProgressStripText(job, popup.progress, formatBytes)}
  {@const infoHash = torrentInfoHash(job)}
  {@const sourceSummary = torrentSourceSummary(job)}
  {@const canShow = Boolean(job.targetPath)}
  <div class="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
    <PopupTitlebar title="Torrent session" />
    <main class="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface">
      <section class="grid grid-cols-[72px_minmax(0,1fr)] gap-4 border-b border-border bg-background px-6 py-2.5">
        <div class="flex h-[72px] w-[72px] items-center justify-center rounded-md border border-border bg-surface text-primary shadow-sm">
          <Magnet size={44} strokeWidth={2.2} />
        </div>
        <div class="min-w-0">
          <div class="flex min-w-0 items-start justify-between gap-4">
            <div class="min-w-0">
              <h1 class="truncate text-xl font-semibold leading-7 text-foreground" title={torrentDisplayName(job)}>
                {torrentDisplayName(job)}
              </h1>
              <div class="mt-1.5 flex min-w-0 items-center gap-3">
                {@render StatusChip(job)}
                <span class="truncate text-sm text-muted-foreground" title={torrentSubtitle(job)}>
                  {torrentSubtitle(job)}
                </span>
              </div>
            </div>
          </div>

          <div class="mt-2 flex min-w-0 items-center gap-4 text-xs text-muted-foreground">
            <div class="flex min-w-0 items-center gap-2">
              <span class="shrink-0">Info hash:</span>
              <span class="truncate text-foreground" title={infoHash}>{infoHash}</span>
            </div>
            <div class="h-5 w-px shrink-0 bg-border"></div>
            <div class="flex min-w-0 items-center gap-2">
              <Link2 size={15} class="shrink-0 text-muted-foreground" />
              <span class="truncate" title={sourceSummary}>Source: {sourceSummary}</span>
            </div>
          </div>
        </div>
      </section>

      <section class="border-b border-border bg-background px-6 py-2.5">
        <div class="mb-2 flex items-end justify-between gap-4">
          <div class="flex items-end gap-5">
            <span class="text-2xl font-semibold leading-none text-primary tabular-nums">{stripText.progressLabel}</span>
            <span class="text-base text-foreground tabular-nums">{stripText.bytesText}</span>
          </div>
          <span class="text-sm text-muted-foreground tabular-nums">
            {torrentRemainingText(job, formatBytes)}
          </span>
        </div>
        {@render SegmentedTorrentProgress(popup.progress)}
      </section>

      <section class="px-6 py-2.5">
        <div class="grid grid-cols-6 divide-x divide-border rounded-md border border-border bg-background">
          {@render TorrentMetric(ArrowDown, 'Down', formatSpeed(popup.progressMetrics.averageSpeed), 'primary')}
          {@render TorrentMetric(ArrowUp, 'Up', formatSpeed(torrentUploadSpeed(job)), 'warning')}
          {@render TorrentMetric(Clock, 'ETA', formatTime(popup.progressMetrics.timeRemaining))}
          {@render TorrentMetric(Users, 'Peers', torrentPeerValue(job))}
          {@render TorrentMetric(Leaf, 'Seeds', torrentSeedValue(job), 'success')}
          {@render TorrentMetric(Gauge, 'Ratio', torrentRatioValue(job), 'warning')}
        </div>

        <div class="mt-2.5 overflow-hidden rounded-md border border-border bg-background">
          {@render PeerHealthRow(job)}
          {@render TorrentDetailRow(FileText, 'Files', torrentFilesText(job, formatBytes))}
          {@render TorrentDetailRow(HardDrive, 'Save to', job.targetPath || 'No destination recorded yet.')}
          {@render TorrentDetailRow(Globe2, 'Source', sourceSummary)}
        </div>

        {#if popup.errorMessage}
          <div class="mt-3 truncate rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" title={popup.errorMessage}>
            {popup.errorMessage}
          </div>
        {/if}
      </section>

      <div class="mt-auto flex shrink-0 justify-end gap-3 border-t border-border bg-background px-6 py-2">
        {#if (isActive(job) || isPaused(job) || isSeeding(job)) && canShow}
          {@render Action('Show', FolderOpen, () => void popup.runAction(async () => { await revealJobInFolder(job.id); }, { closeOnSuccess: true }))}
        {/if}
        {#if isActive(job)}
          {@render Action('Pause', Pause, () => void popup.runAction(() => pauseJob(job.id)), true)}
        {/if}
        {#if isPaused(job)}
          {@render Action('Resume', Play, () => void popup.runAction(() => resumeJob(job.id)), true)}
        {/if}
        {#if isCompleted(job)}
          {@render Action('Open', ExternalLink, () => void popup.runAction(async () => { await openJobFile(job.id); }, { closeOnSuccess: true }), true)}
        {/if}
        {#if isFailed(job)}
          {@render Action('Retry', RotateCw, () => void popup.runAction(() => retryJob(job.id)), true)}
        {/if}
        {#if isFailed(job) && canSwapFailedDownloadToBrowser(job)}
          {@render Action('Swap', ExternalLink, () => void popup.runAction(() => swapFailedDownloadToBrowser(job.id), { closeOnSuccess: true }))}
        {/if}
        {#if isCompleted(job) && canShow}
          {@render Action('Show', FolderOpen, () => void popup.runAction(async () => { await revealJobInFolder(job.id); }, { closeOnSuccess: true }))}
        {/if}
        {#if isActive(job) || isPaused(job)}
          {@render Action(popup.isConfirmingCancel ? 'Confirm' : 'Cancel', X, popup.onCancelClick, false, !popup.isConfirmingCancel, popup.isConfirmingCancel)}
        {/if}
        {#if isCompleted(job) || isFailed(job)}
          {@render Action('Close', X, popup.onClose)}
        {/if}
      </div>
    </main>
  </div>
{/if}

{#snippet StatusChip(job: DownloadJob)}
  <span class={`inline-flex h-6 shrink-0 items-center gap-2 rounded-md border px-2.5 text-sm font-medium ${statusClass(job)}`}>
    <ArrowDown size={14} />
    {statusText(job)}
  </span>
{/snippet}

{#snippet SegmentedTorrentProgress(progress: number)}
  {@const completed = completedSegments(progress)}
  <div class="grid h-3.5 grid-cols-[repeat(42,minmax(0,1fr))] gap-1">
    {#each Array.from({ length: segmentCount }) as _, index}
      <div class={`rounded-sm ${index < completed ? 'bg-primary' : 'bg-progress-track'}`}></div>
    {/each}
  </div>
{/snippet}

{#snippet TorrentMetric(icon: IconComponent, label: string, value: string, tone: MetricTone = 'default')}
  {@const Icon = icon}
  {@const valueClass = metricValueClass(tone)}
  <div class="min-w-0 px-2.5 py-1.5 text-center">
    <div class="flex items-center justify-center gap-1.5 text-xs text-muted-foreground">
      <span class={tone === 'default' ? 'text-foreground' : valueClass}><Icon size={18} /></span>
      <span>{label}</span>
    </div>
    <div class={`mt-0.5 truncate text-lg font-semibold leading-5 tabular-nums ${valueClass}`} title={value}>
      {value}
    </div>
  </div>
{/snippet}

{#snippet PeerHealthRow(job: DownloadJob)}
  <div class="grid min-h-[29px] grid-cols-[104px_minmax(0,1fr)_auto] items-center border-b border-border">
    <div class="flex h-full items-center gap-2 border-r border-border px-3 text-muted-foreground">
      <span class="text-foreground"><Users size={18} /></span>
      <span class="text-xs">Peer health</span>
    </div>
    <div class="min-w-0 px-4 text-sm text-foreground">
      <div class="flex items-center gap-2">
        {#each buildTorrentPeerHealthDots(job) as dot}
          <span class={`h-3 w-3 rounded-full ${peerDotClass(dot.tone)}`}></span>
        {/each}
      </div>
    </div>
    <div class="flex items-center px-3 text-xs text-muted-foreground">
      <span class="whitespace-nowrap">{torrentConnectedText(job)}</span>
    </div>
  </div>
{/snippet}

{#snippet TorrentDetailRow(icon: IconComponent, label: string, value: string)}
  {@const Icon = icon}
  <div class="grid min-h-[29px] grid-cols-[104px_minmax(0,1fr)_auto] items-center border-b border-border last:border-b-0">
    <div class="flex h-full items-center gap-2 border-r border-border px-3 text-muted-foreground">
      <span class="text-foreground"><Icon size={18} /></span>
      <span class="text-xs">{label}</span>
    </div>
    <div class="min-w-0 px-4 text-sm text-foreground">
      <div class="truncate" title={value}>{value}</div>
    </div>
  </div>
{/snippet}

{#snippet Action(label: string, icon: IconComponent, onClick: () => void, primary = false, danger = false, destructive = false)}
  {@const Icon = icon}
  <button
    onclick={onClick}
    disabled={popup.isBusy}
    class={`flex h-8 min-w-[128px] items-center justify-center gap-2.5 rounded-md px-5 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-50 ${actionClass(primary, danger, destructive)}`}
  >
    <Icon size={18} />
    {label}
  </button>
{/snippet}
