<script lang="ts">
  import type { Component } from 'svelte';
  import { AlertTriangle, Archive, CheckCircle2, Download, FolderOpen, Pause, Play, RotateCcw, X } from '@lucide/svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { JobState, type DownloadJob, type Settings } from './types';
  import {
    cancelJobs,
    deleteJobs,
    getBatchProgressSnapshot,
    pauseJobs,
    revealBulkArchive,
    resumeJobs,
    retryBulkArchive,
    subscribeToBatchProgressSnapshot,
    type BatchProgressSnapshot,
  } from './backend';
  import {
    activeBulkFinalizingStepId,
    bulkFinalizingSteps,
    bulkReviewStartSelection,
    calculateBatchProgress,
    deriveBulkPhase,
    deriveBulkUiState,
    isUntouchedBulkReviewGate,
    type BulkFinalizingStepId,
    type BulkPhase,
    type BulkUiState,
    type FailedBatchItem,
    type ProgressBatchContext,
  } from './batchProgress';
  import PopupTitlebar from './PopupTitlebar.svelte';
  import FileBadge from './FileBadge.svelte';
  import { formatBytes, getHost } from './popupShared';
  import { getErrorMessage } from './errors';
  import { applyAppearance } from './appearance';
  import { runPopupAction } from './popupActions';
  import { createDefaultSettings } from './defaultSettings';

  type IconComponent = Component<{ size?: number; class?: string }>;
  type ActionVariant = 'default' | 'primary' | 'cancel' | 'confirm' | 'show';
  const bulkFinalizingStepLabels: Record<BulkFinalizingStepId, string> = {
    uncompressing: 'Uncompressing',
    combining: 'Combining',
    compressing: 'Compressing',
  };

  let context = $state<ProgressBatchContext | null>(null);
  let jobs = $state<DownloadJob[]>([]);
  let isBusy = $state(false);
  let isConfirmingCancel = $state(false);
  let errorMessage = $state('');
  let selectedBulkJobIds = $state<Set<string>>(new Set());
  let reviewSelectionSignature = $state('');
  let lastBulkUiState = $state<BulkUiState | null>(null);
  let bulkHasStarted = $state(false);
  let lastBatchId = $state('');
  const currentWindow = isTauriRuntime() ? getCurrentWindow() : null;
  const batchId = new URLSearchParams(window.location.search).get('batchId') || '';

  const summary = $derived(calculateBatchProgress(jobs));
  const progress = $derived(summary.progress);
  const failedItems = $derived(context?.failedItems ?? []);
  const bulkPhase = $derived(context?.kind === 'bulk' ? deriveBulkPhase(jobs) : null);
  const rawBulkUiState = $derived(context?.kind === 'bulk' ? deriveBulkUiState(jobs) : null);
  const bulkUiState = $derived(context?.kind === 'bulk'
    ? bulkHasStarted && rawBulkUiState === 'review'
      ? 'downloading'
      : rawBulkUiState
    : null);
  const isBulkReviewPhase = $derived(bulkUiState === 'review');
  const selectedBulkCount = $derived(jobs.filter((job) => selectedBulkJobIds.has(job.id)).length);
  const completedArchive = $derived(jobs.find((job) => (
    job.bulkArchive?.archiveStatus === 'completed'
    && Boolean(job.bulkArchive.outputPath)
  ))?.bulkArchive ?? null);
  const failedArchive = $derived(jobs.find((job) => job.bulkArchive?.archiveStatus === 'failed')?.bulkArchive ?? null);
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

  $effect(() => {
    const nextBatchId = context?.batchId ?? batchId;
    if (nextBatchId !== lastBatchId) {
      bulkHasStarted = false;
      lastBatchId = nextBatchId;
    }
  });

  $effect(() => {
    if (context?.kind !== 'bulk' || bulkUiState !== 'review') {
      reviewSelectionSignature = '';
      return;
    }

    const nextSignature = jobs.map((job) => job.id).join('|');
    if (nextSignature !== reviewSelectionSignature) {
      selectedBulkJobIds = new Set(jobs.map((job) => job.id));
      reviewSelectionSignature = nextSignature;
    }
  });

  $effect(() => {
    if (bulkUiState !== lastBulkUiState) {
      isConfirmingCancel = false;
      lastBulkUiState = bulkUiState;
    }
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
        state: JobState.Paused,
        createdAt: now - 1000 * 60 * 8,
        progress: 0,
        totalBytes: 524288000,
        downloadedBytes: 0,
        speed: 0,
        eta: 0,
        targetPath: 'C:\\Users\\You\\Downloads\\model.fbx',
        bulkArchive: { id: 'preview-bulk', name: 'bulk-download.zip', outputKind: 'archive', archiveStatus: 'pending' },
      },
      {
        id: 'preview-2',
        url: 'https://example.com/assets/textures.zip',
        filename: 'textures.zip',
        transferKind: 'http',
        state: JobState.Paused,
        createdAt: now - 1000 * 60 * 7,
        progress: 0,
        totalBytes: 734003200,
        downloadedBytes: 0,
        speed: 0,
        eta: 0,
        targetPath: 'C:\\Users\\You\\Downloads\\textures.zip',
        bulkArchive: { id: 'preview-bulk', name: 'bulk-download.zip', outputKind: 'archive', archiveStatus: 'pending' },
      },
      {
        id: 'preview-3',
        url: 'https://example.com/assets/readme.pdf',
        filename: 'readme.pdf',
        transferKind: 'http',
        state: JobState.Paused,
        createdAt: now - 1000 * 60 * 6,
        progress: 0,
        totalBytes: 12582912,
        downloadedBytes: 0,
        speed: 0,
        eta: 0,
        targetPath: 'C:\\Users\\You\\Downloads\\readme.pdf',
        bulkArchive: { id: 'preview-bulk', name: 'bulk-download.zip', outputKind: 'archive', archiveStatus: 'pending' },
      },
    ];
    const previewSettings = createDefaultSettings();
    previewSettings.torrent = {
      ...previewSettings.torrent,
      seedMode: 'ratio',
      seedRatioLimit: 2,
      seedTimeLimitMinutes: 120,
      portForwardingPort: 6881,
    };

    return {
      context: {
        kind: 'bulk',
        jobIds: previewJobs.map((job) => job.id),
        title: 'Bulk download progress',
        archiveName: 'bulk-download.zip',
        failedItems: [
          {
            url: 'https://datanodes.to/61nni6me5p0n/protected.rar',
            message: 'DataNodes captcha-protected downloads are not supported.',
          },
        ],
      },
      jobs: previewJobs,
      settings: previewSettings,
    };
  }

  async function runAction(
    action: () => Promise<void>,
    { closeOnSuccess = false }: { closeOnSuccess?: boolean } = {},
  ) {
    isBusy = true;
    isConfirmingCancel = false;
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

  function toggleBulkJobSelection(id: string) {
    const nextSelection = new Set(selectedBulkJobIds);
    if (nextSelection.has(id)) {
      nextSelection.delete(id);
    } else {
      nextSelection.add(id);
    }
    selectedBulkJobIds = nextSelection;
  }

  function startBulkDownload() {
    const selection = bulkReviewStartSelection(jobs, selectedBulkJobIds);
    if (selection.includedJobs.length === 0) {
      errorMessage = 'Select at least one file to start.';
      return;
    }

    bulkHasStarted = true;
    void runAction(async () => {
      if (selection.excludedJobs.length > 0) {
        await deleteJobs(selection.excludedJobs.map((job) => job.id), false);
      }
      await resumeJobs(selection.resumableJobs.map((job) => job.id));
    });
  }

  function onBulkPauseResumeClick() {
    const targetJobs = canPause ? jobs.filter(isPausable) : jobs.filter(isResumable);
    void runAction(() => canPause
      ? pauseJobs(targetJobs.map((job) => job.id))
      : resumeJobs(targetJobs.map((job) => job.id)));
  }

  function onBulkCancelClick() {
    if (!isConfirmingCancel) {
      isConfirmingCancel = true;
      return;
    }

    const targetJobs = bulkUiState === 'review' ? jobs : jobs.filter(isCancelable);
    void runAction(
      () => bulkUiState === 'review'
        ? deleteJobs(targetJobs.map((job) => job.id), false)
        : cancelJobs(targetJobs.map((job) => job.id)),
      { closeOnSuccess: bulkUiState === 'review' },
    );
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

  function phaseClass(phase: BulkPhase | BulkUiState | BulkFinalizingStepId | null) {
    if (phase === 'failed') return 'text-destructive';
    if (phase === 'ready') return 'text-success';
    if (phase === 'extracting' || phase === 'uncompressing' || phase === 'combining' || phase === 'creating_folder' || phase === 'compressing' || phase === 'finalizing') {
      return 'text-warning';
    }
    return 'text-primary';
  }

  function actionClass(variant: ActionVariant) {
    switch (variant) {
      case 'primary':
        return 'border border-primary bg-background text-primary hover:bg-primary-soft cursor-pointer';
      case 'cancel':
        return 'border border-destructive bg-destructive text-destructive-foreground hover:bg-destructive/90 cursor-pointer';
      case 'confirm':
      case 'show':
        return 'border border-border bg-white text-black hover:bg-white/90 cursor-pointer';
      default:
        return 'border border-input bg-background text-foreground hover:bg-muted cursor-pointer';
    }
  }

  function isTauriRuntime(): boolean {
    return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
  }

  function failedItemName(item: FailedBatchItem) {
    try {
      const parsed = new URL(item.url);
      const segment = parsed.pathname.split('/').filter(Boolean).pop();
      return segment ? decodeURIComponent(segment) : parsed.host;
    } catch {
      return item.url;
    }
  }

  function failedItemSuffix(count: number) {
    return count > 0 ? `, ${count} not queued` : '';
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

    <main class="flex min-h-0 flex-1 flex-col bg-surface px-5 py-3">
      <section class="flex items-start gap-3">
        <div class="flex h-12 w-11 shrink-0 items-center justify-center rounded-md border border-border bg-background text-primary">
          {#if context.kind === 'bulk'}<Archive size={24} />{:else}<Download size={22} />{/if}
        </div>
        <div class="min-w-0 flex-1">
          <h1 class="truncate text-base font-semibold leading-5 text-foreground" title={context.archiveName ?? context.title}>
            {context.archiveName ?? context.title}
          </h1>
          <div class="mt-1 text-xs text-muted-foreground">
            {#if context.kind === 'bulk' && bulkUiState === 'review'}
              {selectedBulkCount} of {summary.totalCount} selected{failedItemSuffix(failedItems.length)}
            {:else if context.kind === 'bulk' && bulkUiState === 'finalizing'}
              Preparing combined output
            {:else}
              {summary.completedCount} of {summary.totalCount} completed{summary.failedCount > 0 ? `, ${summary.failedCount} failed` : ''}{failedItemSuffix(failedItems.length)}
            {/if}
          </div>
        </div>
        <div class="text-right text-2xl font-semibold tabular-nums text-foreground">
          {isBulkReviewPhase ? selectedBulkCount : progress.toFixed(0)}{isBulkReviewPhase ? '' : '%'}
        </div>
      </section>

      <section class="mt-3">
        <div class="mb-1.5 flex items-center justify-between text-xs tabular-nums text-muted-foreground">
          <span>
            {#if context.kind === 'bulk' && bulkUiState === 'review'}
              Ready to start
            {:else if context.kind === 'bulk' && bulkUiState === 'finalizing'}
              Finalizing
            {:else}
              {summary.activeCount} active
            {/if}
          </span>
          <span>
            {summary.knownTotal
              ? `${formatBytes(summary.downloadedBytes)} / ${formatBytes(summary.totalBytes)}`
              : `${summary.completedCount + summary.failedCount} / ${summary.totalCount} items`}
          </span>
        </div>
        <div class="h-1.5 overflow-hidden rounded-full bg-progress-track">
          <div class={`h-1.5 rounded-full transition-[width,background-color] duration-300 ${summary.failedCount > 0 ? 'bg-destructive' : 'bg-primary'}`} style={`width: ${isBulkReviewPhase ? 0 : progress}%`}></div>
        </div>
      </section>

      {#if context.kind === 'bulk' && bulkUiState === 'finalizing'}
        {@render BulkFinalizingStrip(bulkPhase, jobs)}
      {:else if context.kind === 'bulk' && bulkUiState}
        {@render BulkStateStrip(bulkUiState, jobs)}
      {/if}

      {#if context.kind === 'bulk' && bulkUiState === 'review' && isUntouchedBulkReviewGate(jobs)}
        {@render BulkReviewList(jobs, failedItems)}
      {:else}
        {@render BatchJobList(jobs, failedItems)}
      {/if}

      {#if errorMessage}
        <div class="mt-2 rounded border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive">
          {errorMessage}
        </div>
      {/if}

      {#if context.kind === 'bulk'}
        {#if bulkUiState !== 'finalizing'}
          {@render BulkFooter()}
        {/if}
      {:else}
        {@render MultiFooter()}
      {/if}
    </main>
  </div>
{/if}

{#snippet BatchJobList(jobs: DownloadJob[], failedItems: FailedBatchItem[])}
  <section class="mt-3 min-h-0 flex-1 overflow-y-auto rounded border border-border/60 bg-background/40">
    {#if jobs.length === 0 && failedItems.length === 0}
      <div class="flex h-full min-h-[120px] items-center justify-center px-4 text-center text-sm text-muted-foreground">
        Waiting for queued files to appear.
      </div>
    {:else}
      <div class="divide-y divide-border/60">
        {#each jobs as job (job.id)}
          {@render BatchJobRow(job)}
        {/each}
        {#each failedItems as item, index (`${item.url}-${index}`)}
          {@render FailedBatchItemRow(item)}
        {/each}
      </div>
    {/if}
  </section>
{/snippet}

{#snippet BulkReviewList(jobs: DownloadJob[], failedItems: FailedBatchItem[])}
  <section class="mt-3 min-h-0 flex-1 overflow-y-auto rounded border border-border/60 bg-background/40">
    {#if jobs.length === 0 && failedItems.length === 0}
      <div class="flex h-full min-h-[120px] items-center justify-center px-4 text-center text-sm text-muted-foreground">
        Waiting for queued files to appear.
      </div>
    {:else}
      <div class="divide-y divide-border/60">
        {#each jobs as job (job.id)}
          <label class="grid cursor-pointer grid-cols-[28px_40px_minmax(0,1fr)_82px] items-center gap-2 px-3 py-2 transition hover:bg-row-hover">
            <input
              type="checkbox"
              checked={selectedBulkJobIds.has(job.id)}
              onchange={() => toggleBulkJobSelection(job.id)}
              class="h-4 w-4 accent-primary"
              aria-label={`Include ${job.filename}`}
            />
            <FileBadge filename={job.filename} transferKind={job.transferKind} />
            <div class="min-w-0">
              <div class={`truncate text-sm font-semibold leading-5 ${selectedBulkJobIds.has(job.id) ? 'text-foreground' : 'text-muted-foreground'}`} title={job.filename}>{job.filename}</div>
              <div class="truncate text-xs text-muted-foreground" title={job.url}>{getHost(job.url)}</div>
            </div>
            <div class="min-w-0 text-right text-xs">
              <div class={selectedBulkJobIds.has(job.id) ? 'font-semibold text-primary' : 'font-semibold text-muted-foreground'}>
                {selectedBulkJobIds.has(job.id) ? 'Included' : 'Excluded'}
              </div>
              <div class="mt-0.5 truncate tabular-nums text-muted-foreground" title={formatBytes(job.totalBytes)}>
                {job.totalBytes > 0 ? formatBytes(job.totalBytes) : 'Unknown'}
              </div>
            </div>
          </label>
        {/each}
        {#each failedItems as item, index (`${item.url}-${index}`)}
          {@render FailedBatchItemRow(item)}
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
        <div class={`h-1 rounded-full transition-[width,background-color] duration-300 ${progressColor(job.state)}`} style={`width: ${rowProgress}%`}></div>
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

{#snippet FailedBatchItemRow(item: FailedBatchItem)}
  <div class="grid grid-cols-[40px_minmax(0,1fr)_86px] items-center gap-2 px-3 py-2">
    <div class="flex h-9 w-9 items-center justify-center rounded-md border border-destructive/40 bg-destructive/10 text-destructive">
      <AlertTriangle size={17} />
    </div>
    <div class="min-w-0">
      <div class="truncate text-sm font-semibold leading-5 text-foreground" title={failedItemName(item)}>{failedItemName(item)}</div>
      <div class="truncate text-xs text-muted-foreground" title={item.url}>{getHost(item.url)}</div>
      <div class="mt-1 truncate text-xs text-destructive" title={item.message}>{item.message}</div>
    </div>
    <div class="min-w-0 text-right text-xs">
      <div class="font-semibold text-destructive">Not queued</div>
      <div class="mt-0.5 truncate text-muted-foreground">Resolver</div>
    </div>
  </div>
{/snippet}

{#snippet BulkStateStrip(state: BulkUiState, jobs: DownloadJob[])}
  {@const archive = jobs.find((job) => job.bulkArchive)?.bulkArchive}
  {@const phases = [
    { id: 'review' as BulkUiState, label: 'Review links' },
    { id: 'downloading' as BulkUiState, label: 'Downloading files' },
    { id: 'finalizing' as BulkUiState, label: 'Finalizing output' },
    { id: 'ready' as BulkUiState, label: 'Ready' },
  ]}
  {@const activeIndex = phases.findIndex((item) => item.id === state)}
  <section class="mt-3 rounded border border-border bg-background px-3 py-2">
    <div class="grid grid-cols-4 gap-2">
      {#each phases as item, index (item.id)}
        {@const isDone = state !== 'failed' && index < activeIndex}
        {@const isActive = index === activeIndex}
        <div class={`flex min-w-0 items-center gap-1.5 text-xs font-semibold ${isActive ? phaseClass(state) : isDone ? 'text-success' : 'text-muted-foreground'}`}>
          <span class={`flex h-4 w-4 shrink-0 items-center justify-center rounded-full border text-[10px] ${isActive ? 'border-current' : isDone ? 'border-success bg-success text-success-foreground' : 'border-border'}`}>
            {#if isDone}<CheckCircle2 size={10} />{:else}{index + 1}{/if}
          </span>
          <span class="truncate">{item.label}</span>
        </div>
      {/each}
    </div>
    {#if state === 'failed' && archive?.error}
      <div class="mt-2 truncate text-xs text-destructive" title={archive.error}>{archive.error}</div>
    {/if}
    {#if state === 'ready' && archive?.warning}
      <div class="mt-2 truncate text-xs text-warning" title={archive.warning}>{archive.warning}</div>
    {/if}
  </section>
{/snippet}

{#snippet BulkFinalizingStrip(phase: BulkPhase | null, jobs: DownloadJob[])}
  {@const archive = jobs.find((job) => job.bulkArchive)?.bulkArchive}
  {@const steps = bulkFinalizingSteps(jobs)}
  {@const activeStep = activeBulkFinalizingStepId(phase)}
  {@const activeIndex = steps.findIndex((item) => item.id === activeStep)}
  <section class="mt-3 rounded border border-border bg-background px-3 py-2">
    <div class="mb-2 flex items-center justify-between gap-3">
      <div class="min-w-0">
        <div class="text-xs font-semibold text-warning">Finalizing output</div>
        <div class="mt-0.5 truncate text-xs text-muted-foreground" title={archive?.name}>{archive?.name ?? 'Bulk output'}</div>
      </div>
      <div class="text-xs tabular-nums text-muted-foreground">{steps.length} steps</div>
    </div>
    <div class="grid gap-2" style={`grid-template-columns: repeat(${steps.length}, minmax(0, 1fr));`}>
      {#each steps as item, index (item.id)}
        {@const isDone = activeIndex >= 0 && index < activeIndex}
        {@const isActive = index === activeIndex}
        <div class={`flex min-w-0 items-center gap-1.5 text-xs font-semibold ${isActive ? phaseClass(item.id) : isDone ? 'text-success' : 'text-muted-foreground'}`}>
          <span class={`flex h-4 w-4 shrink-0 items-center justify-center rounded-full border text-[10px] ${isActive ? 'border-current' : isDone ? 'border-success bg-success text-success-foreground' : 'border-border'}`}>
            {#if isDone}<CheckCircle2 size={10} />{:else}{index + 1}{/if}
          </span>
          <span class="truncate">{bulkFinalizingStepLabels[item.id] ?? item.label}</span>
        </div>
      {/each}
    </div>
  </section>
{/snippet}

{#snippet BulkFooter()}
  <div class="mt-3 flex min-h-[45px] shrink-0 justify-end gap-3 border-t border-border pt-3">
    {#if bulkUiState === 'ready' && completedArchive}
      {@render ActionButton('Show', FolderOpen, () => void runAction(() => revealBulkArchive(completedArchive.id), { closeOnSuccess: true }), isBusy, 'show')}
    {:else if bulkUiState === 'review'}
      {@render ActionButton(isConfirmingCancel ? 'Confirm' : 'Cancel', X, onBulkCancelClick, isBusy, isConfirmingCancel ? 'confirm' : 'cancel')}
      {@render ActionButton('Start', Play, () => void startBulkDownload(), isBusy || selectedBulkCount === 0, 'primary')}
    {:else if bulkUiState === 'downloading'}
      {@render ActionButton(canPause ? 'Pause' : 'Resume', canPause ? Pause : Play, onBulkPauseResumeClick, isBusy || (!canPause && !canResume), 'primary')}
      {@render ActionButton(isConfirmingCancel ? 'Confirm' : 'Cancel', X, onBulkCancelClick, isBusy || !canCancel, isConfirmingCancel ? 'confirm' : 'cancel')}
    {:else if bulkUiState === 'failed'}
      {#if failedArchive}
        {@render ActionButton('Retry archive', RotateCcw, () => void runAction(() => retryBulkArchive(failedArchive.id)), isBusy, 'primary')}
      {/if}
      {@render ActionButton('Close', X, () => void currentWindow?.close(), isBusy)}
    {/if}
  </div>
{/snippet}

{#snippet MultiFooter()}
  <div class="mt-3 flex min-h-[45px] shrink-0 justify-end gap-3 border-t border-border pt-3">
    {#if summary.activeCount > 0 || canPause || canResume || canCancel}
      {@render ActionButton('Pause all', Pause, () => void runAction(() => pauseJobs(jobs.filter(isPausable).map((job) => job.id))), isBusy || !canPause)}
      {@render ActionButton('Resume all', Play, () => void runAction(() => resumeJobs(jobs.filter(isResumable).map((job) => job.id))), isBusy || !canResume, 'primary')}
      {@render ActionButton('Cancel active', X, () => void runAction(() => cancelJobs(jobs.filter(isCancelable).map((job) => job.id))), isBusy || !canCancel, 'cancel')}
    {:else}
      {@render ActionButton('Close', X, () => void currentWindow?.close(), isBusy)}
    {/if}
  </div>
{/snippet}

{#snippet ActionButton(label: string, icon: IconComponent, onClick: () => void, disabled = false, variant: ActionVariant = 'default')}
  {@const Icon = icon}
  <button
    onclick={onClick}
    {disabled}
    class={`flex h-8 min-w-[112px] items-center justify-center gap-2 rounded-md px-4 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-50 ${actionClass(variant)}`}
  >
    <Icon size={17} />
    {label}
  </button>
{/snippet}
