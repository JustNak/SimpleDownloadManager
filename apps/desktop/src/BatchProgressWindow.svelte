<script lang="ts">
  import type { Component } from 'svelte';
  import { Archive, CheckCircle2, Download, FolderOpen, Pause, Play, X } from '@lucide/svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { JobState, type DownloadJob, type Settings } from './types';
  import {
    cancelJob,
    getBatchProgressSnapshot,
    pauseJob,
    resumeJob,
    revealJobInFolder,
    subscribeToBatchProgressSnapshot,
    type BatchProgressSnapshot,
  } from './backend';
  import {
    calculateBatchProgress,
    deriveBulkPhase,
    type BulkPhase,
    type ProgressBatchContext,
  } from './batchProgress';
  import PopupTitlebar from './PopupTitlebar.svelte';
  import FileBadge from './FileBadge.svelte';
  import { formatBytes, getHost } from './popupShared';
  import { getErrorMessage } from './errors';
  import { applyAppearance } from './appearance';
  import { runPopupAction } from './popupActions';

  type IconComponent = Component<{ size?: number; class?: string }>;

  let context = $state<ProgressBatchContext | null>(null);
  let jobs = $state<DownloadJob[]>([]);
  let isBusy = $state(false);
  let errorMessage = $state('');
  const currentWindow = isTauriRuntime() ? getCurrentWindow() : null;
  const batchId = new URLSearchParams(window.location.search).get('batchId') || '';

  const summary = $derived(calculateBatchProgress(jobs));
  const progress = $derived(summary.progress);
  const bulkPhase = $derived(context?.kind === 'bulk' ? deriveBulkPhase(jobs) : null);
  const completedJob = $derived(jobs.find((job) => job.state === JobState.Completed && job.targetPath));
  const canPause = $derived(jobs.some(isPausable));
  const canResume = $derived(jobs.some(isResumable));
  const canCancel = $derived(jobs.some(isCancelable));

  $effect(() => {
    let dispose: (() => void | Promise<void>) | undefined;
    let disposed = false;
    let latestSettings: Settings | null = null;

    const applySnapshotAppearance = (snapshot: Awaited<ReturnType<typeof getBatchProgressSnapshot>>) => {
      latestSettings = snapshot.settings;
      applyAppearance(snapshot.settings);
    };

    const media = typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-color-scheme: dark)') : null;
    const handleSystemThemeChange = () => {
      if (latestSettings) applyAppearance(latestSettings);
    };
    media?.addEventListener('change', handleSystemThemeChange);

    async function initialize() {
      const snapshot = batchId ? await getBatchProgressSnapshot(batchId) : await getPreviewBatchProgressSnapshot();
      if (disposed) return;
      context = snapshot?.context ?? null;
      if (snapshot) {
        applySnapshotAppearance(snapshot);
        jobs = snapshot.jobs;
      }

      dispose = await subscribeToBatchProgressSnapshot((nextSnapshot) => {
        applySnapshotAppearance(nextSnapshot);
        context = nextSnapshot.context;
        jobs = nextSnapshot.jobs;
      });
    }

    void initialize().catch((error) => {
      if (!disposed) errorMessage = getErrorMessage(error, 'Could not load batch progress.');
    });

    return () => {
      disposed = true;
      media?.removeEventListener('change', handleSystemThemeChange);
      void dispose?.();
    };
  });

  function getPreviewBatchProgressSnapshot(): BatchProgressSnapshot | null {
    if (isTauriRuntime()) return null;
    const now = Date.now();
    const previewJobs: DownloadJob[] = [
      {
        id: 'preview-1',
        url: 'https://example.com/assets/model.fbx',
        filename: 'model.fbx',
        transferKind: 'http',
        state: JobState.Downloading,
        createdAt: now - 1000 * 60 * 8,
        progress: 62,
        totalBytes: 524288000,
        downloadedBytes: 325058560,
        speed: 7340032,
        eta: 28,
        targetPath: 'C:\\Users\\You\\Downloads\\model.fbx',
        bulkArchive: { id: 'preview-bulk', name: 'bulk-download.zip', archiveStatus: 'pending' },
      },
      {
        id: 'preview-2',
        url: 'https://example.com/assets/textures.zip',
        filename: 'textures.zip',
        transferKind: 'http',
        state: JobState.Downloading,
        createdAt: now - 1000 * 60 * 7,
        progress: 38,
        totalBytes: 734003200,
        downloadedBytes: 279969792,
        speed: 5242880,
        eta: 86,
        targetPath: 'C:\\Users\\You\\Downloads\\textures.zip',
        bulkArchive: { id: 'preview-bulk', name: 'bulk-download.zip', archiveStatus: 'pending' },
      },
      {
        id: 'preview-3',
        url: 'https://example.com/assets/readme.pdf',
        filename: 'readme.pdf',
        transferKind: 'http',
        state: JobState.Completed,
        createdAt: now - 1000 * 60 * 6,
        progress: 100,
        totalBytes: 12582912,
        downloadedBytes: 12582912,
        speed: 0,
        eta: 0,
        targetPath: 'C:\\Users\\You\\Downloads\\readme.pdf',
        bulkArchive: { id: 'preview-bulk', name: 'bulk-download.zip', archiveStatus: 'pending' },
      },
    ];
    return {
      context: {
        kind: 'bulk',
        jobIds: previewJobs.map((job) => job.id),
        title: 'Bulk download progress',
        archiveName: 'bulk-download.zip',
      },
      jobs: previewJobs,
      settings: {
        downloadDirectory: 'C:\\Users\\You\\Downloads',
        maxConcurrentDownloads: 3,
        autoRetryAttempts: 3,
        speedLimitKibPerSecond: 0,
        downloadPerformanceMode: 'balanced',
        torrent: {
          enabled: true,
          downloadDirectory: 'C:\\Users\\You\\Downloads\\Torrent',
          seedMode: 'ratio',
          seedRatioLimit: 2,
          seedTimeLimitMinutes: 120,
          uploadLimitKibPerSecond: 0,
          portForwardingEnabled: false,
          portForwardingPort: 6881,
          peerConnectionWatchdogMode: 'diagnose',
        },
        notificationsEnabled: true,
        theme: 'system',
        accentColor: '#3b82f6',
        showDetailsOnClick: true,
        queueRowSize: 'medium',
        startOnStartup: false,
        startupLaunchMode: 'open',
        extensionIntegration: {
          enabled: true,
          downloadHandoffMode: 'ask',
          listenPort: 17654,
          contextMenuEnabled: true,
          showProgressAfterHandoff: true,
          showBadgeStatus: true,
          excludedHosts: [],
          ignoredFileExtensions: [],
          authenticatedHandoffEnabled: true,
          authenticatedHandoffHosts: [],
        },
      },
    };
  }

  async function runAction(
    action: () => Promise<void>,
    { closeOnSuccess = false }: { closeOnSuccess?: boolean } = {},
  ) {
    isBusy = true;
    errorMessage = '';
    const result = await runPopupAction({
      action,
      close: closeOnSuccess && currentWindow ? () => currentWindow.close() : undefined,
    });
    if (!result.ok) {
      errorMessage = result.message;
    }
    isBusy = false;
  }

  async function runForJobs(targetJobs: DownloadJob[], action: (id: string) => Promise<void>) {
    for (const job of targetJobs) {
      await action(job.id);
    }
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
</script>

{#if !context}
  <div class="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
    <PopupTitlebar title="Batch progress" />
    <div class="flex flex-1 items-center justify-center px-6 text-center text-sm text-muted-foreground">
      {errorMessage || 'This batch progress context is no longer available.'}
    </div>
  </div>
{:else}
  <div class="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
    <PopupTitlebar title={context.title} />

    <main class="flex min-h-0 flex-1 flex-col bg-surface px-4 py-3">
      <section class="flex items-start gap-3">
        <div class="flex h-12 w-10 shrink-0 items-center justify-center rounded-sm border border-border bg-background text-primary">
          {#if context.kind === 'bulk'}<Archive size={22} />{:else}<Download size={22} />{/if}
        </div>
        <div class="min-w-0 flex-1">
          <h1 class="truncate text-base font-semibold leading-5 text-foreground" title={context.archiveName ?? context.title}>
            {context.archiveName ?? context.title}
          </h1>
          <div class="mt-0.5 text-xs text-muted-foreground">
            {summary.completedCount} of {summary.totalCount} completed{summary.failedCount > 0 ? `, ${summary.failedCount} failed` : ''}
          </div>
        </div>
        <div class="text-right text-2xl font-semibold tabular-nums text-foreground">
          {progress.toFixed(0)}%
        </div>
      </section>

      <section class="mt-3">
        <div class="mb-1.5 flex items-center justify-between text-xs tabular-nums text-muted-foreground">
          <span>{summary.activeCount} active</span>
          <span>
            {summary.knownTotal
              ? `${formatBytes(summary.downloadedBytes)} / ${formatBytes(summary.totalBytes)}`
              : `${summary.completedCount + summary.failedCount} / ${summary.totalCount} items`}
          </span>
        </div>
        <div class="h-1.5 overflow-hidden rounded-full bg-progress-track">
          <div class={`h-1.5 rounded-full transition-all duration-300 ${summary.failedCount > 0 ? 'bg-destructive' : 'bg-primary'}`} style={`width: ${progress}%`}></div>
        </div>
      </section>

      {#if bulkPhase}
        {@render BulkPhaseStrip(bulkPhase, jobs)}
      {/if}

      {@render BatchJobList(jobs)}

      {#if errorMessage}
        <div class="mt-2 rounded border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive">
          {errorMessage}
        </div>
      {/if}

      <div class="mt-3 flex flex-wrap justify-end gap-2 border-t border-border pt-3">
        {@render ActionButton('Pause all', Pause, () => void runAction(() => runForJobs(jobs.filter(isPausable), pauseJob)), isBusy || !canPause)}
        {@render ActionButton('Resume all', Play, () => void runAction(() => runForJobs(jobs.filter(isResumable), resumeJob)), isBusy || !canResume, canResume)}
        {@render ActionButton('Cancel active', X, () => void runAction(() => runForJobs(jobs.filter(isCancelable), cancelJob)), isBusy || !canCancel, false, canCancel)}
        {@render ActionButton(
          'Reveal completed',
          FolderOpen,
          () => {
            if (!completedJob) return;
            void runAction(async () => {
              await revealJobInFolder(completedJob.id);
            }, { closeOnSuccess: true });
          },
          isBusy || !completedJob,
        )}
        {#if summary.activeCount === 0}
          {@render ActionButton('Close', X, () => void currentWindow?.close(), isBusy)}
        {/if}
      </div>
    </main>
  </div>
{/if}

{#snippet BatchJobList(jobs: DownloadJob[])}
  <section class="mt-3 min-h-0 flex-1 overflow-y-auto rounded border border-border/60 bg-background/40">
    {#if jobs.length === 0}
      <div class="flex h-full min-h-[120px] items-center justify-center px-4 text-center text-sm text-muted-foreground">
        Waiting for queued files to appear.
      </div>
    {:else}
      <div class="divide-y divide-border/60">
        {#each jobs as job (job.id)}
          {@render BatchJobRow(job)}
        {/each}
      </div>
    {/if}
  </section>
{/snippet}

{#snippet BatchJobRow(job: DownloadJob)}
  {@const rowProgress = Math.max(0, Math.min(100, job.progress))}
  <div class="grid grid-cols-[40px_minmax(0,1fr)_86px] items-center gap-2 px-3 py-2">
    <FileBadge filename={job.filename} transferKind={job.transferKind} />
    <div class="min-w-0">
      <div class="truncate text-sm font-semibold leading-5 text-foreground" title={job.filename}>{job.filename}</div>
      <div class="truncate text-xs text-muted-foreground" title={job.url}>{job.transferKind === 'torrent' ? 'Torrent' : getHost(job.url)}</div>
      <div class="mt-1 h-1 overflow-hidden rounded-full bg-progress-track">
        <div class={`h-1 rounded-full transition-all duration-300 ${progressColor(job.state)}`} style={`width: ${rowProgress}%`}></div>
      </div>
    </div>
    <div class="min-w-0 text-right text-xs">
      <div class={`font-semibold ${statusTextClass(job.state)}`}>{statusText(job)}</div>
      <div class="mt-0.5 tabular-nums text-muted-foreground">
        {job.state === JobState.Downloading ? `${formatBytes(job.speed)}/s` : `${rowProgress.toFixed(0)}%`}
      </div>
      <div class="truncate tabular-nums text-muted-foreground" title={formatBytes(job.totalBytes)}>
        {job.totalBytes > 0 ? formatBytes(job.totalBytes) : 'Unknown'}
      </div>
    </div>
  </div>
{/snippet}

{#snippet BulkPhaseStrip(phase: BulkPhase, jobs: DownloadJob[])}
  {@const archive = jobs.find((job) => job.bulkArchive)?.bulkArchive}
  {@const phases = [
    { id: 'downloading' as BulkPhase, label: 'Downloading files' },
    { id: 'compressing' as BulkPhase, label: 'Compressing archive' },
    { id: 'ready' as BulkPhase, label: 'Ready' },
  ]}
  {@const activeIndex = phase === 'failed' ? phases.findIndex((item) => item.id === 'compressing') : phases.findIndex((item) => item.id === phase)}
  <section class="mt-3 rounded border border-border bg-background px-3 py-2">
    <div class="grid grid-cols-3 gap-2">
      {#each phases as item, index (item.id)}
        {@const isDone = phase !== 'failed' && index < activeIndex}
        {@const isActive = index === activeIndex}
        <div class={`flex min-w-0 items-center gap-1.5 text-xs font-semibold ${isActive ? phaseClass(phase) : isDone ? 'text-success' : 'text-muted-foreground'}`}>
          <span class={`flex h-4 w-4 shrink-0 items-center justify-center rounded-full border text-[10px] ${isActive ? 'border-current' : isDone ? 'border-success bg-success text-success-foreground' : 'border-border'}`}>
            {#if isDone}<CheckCircle2 size={10} />{:else}{index + 1}{/if}
          </span>
          <span class="truncate">{item.label}</span>
        </div>
      {/each}
    </div>
    {#if phase === 'failed' && archive?.error}
      <div class="mt-2 truncate text-xs text-destructive" title={archive.error}>{archive.error}</div>
    {/if}
  </section>
{/snippet}

{#snippet ActionButton(label: string, icon: IconComponent, onClick: () => void, disabled = false, primary = false, danger = false)}
  {@const Icon = icon}
  {@const buttonClass = danger
    ? 'border border-destructive/50 bg-destructive/10 text-destructive hover:bg-destructive hover:text-destructive-foreground'
    : primary
      ? 'bg-primary text-primary-foreground hover:bg-primary/90'
      : 'border border-input text-foreground hover:bg-muted'}
  <button
    onclick={onClick}
    {disabled}
    class={`flex h-8 items-center gap-1.5 rounded px-2.5 text-xs font-medium transition disabled:cursor-not-allowed disabled:opacity-50 ${buttonClass}`}
  >
    <Icon size={16} />
    {label}
  </button>
{/snippet}
