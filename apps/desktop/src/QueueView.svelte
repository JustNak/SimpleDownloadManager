<script lang="ts">
  import type { Component } from 'svelte';
  import {
    Check,
    ChevronDown,
    ExternalLink,
    FolderOpen,
    MoreHorizontal,
    Pause,
    Play,
    RotateCw,
    Trash2,
    X,
  } from '@lucide/svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import FileBadge from './FileBadge.svelte';
  import type { DownloadJob, QueueRowSize } from './types';
  import { JobState } from './types';
  import type { SortMode, SortColumn } from './downloadSorting';
  import { nextSortModeForColumn, sortModeDirection, sortModeKey } from './downloadSorting';
  import {
    canShowProgressPopup,
    canSwapFailedDownloadToBrowser,
    deleteActionLabelForJob,
    defaultDeleteFromDiskForJobs,
  } from './queueCommands';
  import { formatBytes, formatTime, getHost } from './popupShared';

  type IconComponent = Component<{ size?: number; class?: string }>;

  interface Props {
    jobs: DownloadJob[];
    selectedJobId: string | null;
    showDetailsOnClick: boolean;
    queueRowSize: QueueRowSize;
    sortMode: SortMode;
    onSortChange: (sortMode: SortMode) => void;
    onSelectJob: (id: string | null) => void;
    onPause: (id: string) => void;
    onResume: (id: string) => void;
    onCancel: (id: string) => void;
    onRetry: (id: string) => void;
    onRestart: (id: string) => void;
    onOpen: (id: string) => void;
    onReveal: (id: string) => void;
    onShowPopup: (id: string) => void;
    onSwapFailedToBrowser: (id: string) => void;
    onRename: (id: string, filename: string) => void;
    onDelete: (ids: string[], deleteFromDisk: boolean) => void;
  }

  let {
    jobs,
    selectedJobId,
    showDetailsOnClick,
    queueRowSize,
    sortMode,
    onSortChange,
    onSelectJob,
    onPause,
    onResume,
    onCancel,
    onRetry,
    onRestart,
    onOpen,
    onReveal,
    onShowPopup,
    onSwapFailedToBrowser,
    onRename,
    onDelete,
  }: Props = $props();

  let selectedJobIds = new SvelteSet<string>();
  let openMenuJobId = $state<string | null>(null);
  let contextMenu = $state<{ jobId: string; x: number; y: number } | null>(null);
  let renamePromptJob = $state<DownloadJob | null>(null);
  let renameValue = $state('');
  let deletePromptJobs = $state<DownloadJob[]>([]);
  let deleteFromDisk = $state(false);

  const selectedJob = $derived(jobs.find((job) => job.id === selectedJobId) ?? null);
  const rowClass = $derived(queueRowSizeClass(queueRowSize));

  $effect(() => {
    if (!selectedJobId) {
      selectedJobIds.clear();
      return;
    }
    if (!selectedJobIds.has(selectedJobId)) {
      selectedJobIds.clear();
      selectedJobIds.add(selectedJobId);
    }
  });

  function setSort(column: SortColumn) {
    onSortChange(nextSortModeForColumn(sortMode, column));
  }

  function selectRow(job: DownloadJob, event?: MouseEvent) {
    if (event?.ctrlKey || event?.metaKey) {
      if (selectedJobIds.has(job.id)) selectedJobIds.delete(job.id);
      else selectedJobIds.add(job.id);
      onSelectJob(job.id);
      return;
    }
    selectedJobIds.clear();
    selectedJobIds.add(job.id);
    onSelectJob(job.id);
    contextMenu = null;
  }

  function selectedIdsFor(job: DownloadJob): string[] {
    if (selectedJobIds.has(job.id) && selectedJobIds.size > 1) return [...selectedJobIds];
    return [job.id];
  }

  function openDeletePrompt(job: DownloadJob) {
    const ids = new Set(selectedIdsFor(job));
    deletePromptJobs = jobs.filter((candidate) => ids.has(candidate.id));
    deleteFromDisk = defaultDeleteFromDiskForJobs(deletePromptJobs);
    openMenuJobId = null;
    contextMenu = null;
  }

  function confirmDelete() {
    onDelete(deletePromptJobs.map((job) => job.id), deleteFromDisk);
    deletePromptJobs = [];
  }

  function openRename(job: DownloadJob) {
    renamePromptJob = job;
    renameValue = job.filename;
    openMenuJobId = null;
    contextMenu = null;
  }

  function confirmRename(event: SubmitEvent) {
    event.preventDefault();
    if (!renamePromptJob || !renameValue.trim()) return;
    onRename(renamePromptJob.id, renameValue.trim());
    renamePromptJob = null;
  }

  function closeMenus() {
    openMenuJobId = null;
    contextMenu = null;
  }

  function statusClass(job: DownloadJob): string {
    if (job.state === JobState.Completed) return 'bg-success/10 text-success border-success/30';
    if (job.state === JobState.Failed) return 'bg-destructive/10 text-destructive border-destructive/30';
    if (job.state === JobState.Paused) return 'bg-warning/10 text-warning border-warning/30';
    if (job.state === JobState.Seeding) return 'bg-primary-soft text-accent-foreground border-primary/30';
    return 'bg-muted text-muted-foreground border-border';
  }

  function progressColor(job: DownloadJob): string {
    if (job.state === JobState.Failed) return 'bg-destructive';
    if (job.state === JobState.Completed) return 'bg-success';
    if (job.transferKind === 'torrent') return 'bg-warning';
    return 'bg-primary';
  }

  function isActive(job: DownloadJob): boolean {
    return [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding].includes(job.state);
  }

  function queueRowSizeClass(size: QueueRowSize): string {
    switch (size) {
      case 'compact':
        return 'min-h-[28px] py-0 text-xs';
      case 'small':
        return 'min-h-[34px] py-0.5 text-xs';
      case 'large':
        return 'min-h-[54px] py-2 text-sm';
      case 'damn':
        return 'min-h-[68px] py-2.5 text-base';
      default:
        return 'min-h-[42px] py-1 text-sm';
    }
  }

  function sortMark(column: SortColumn): string {
    return sortModeKey(sortMode) === column ? (sortModeDirection(sortMode) === 'asc' ? '^' : 'v') : '';
  }
</script>

<section class="flex min-h-0 flex-1 flex-col bg-background">
  <div class="download-table min-h-0 flex-1 overflow-auto">
    <div class="grid min-w-[1080px] grid-cols-[36px_minmax(300px,1.6fr)_110px_110px_110px_120px_120px] border-b border-border bg-header px-3 py-2 text-[11px] font-semibold uppercase tracking-normal text-muted-foreground">
      <div></div>
      <button class="text-left hover:text-foreground" onclick={() => setSort('name')}>Name {sortMark('name')}</button>
      <button class="text-left hover:text-foreground" onclick={() => setSort('date')}>Date {sortMark('date')}</button>
      <button class="text-left hover:text-foreground" onclick={() => setSort('size')}>Size {sortMark('size')}</button>
      <div>Progress</div>
      <div>Speed</div>
      <div class="text-right">Actions</div>
    </div>

    {#if jobs.length === 0}
      <div class="flex h-full min-h-[320px] items-center justify-center text-sm text-muted-foreground">No downloads match this view.</div>
    {:else}
      <div class="min-w-[1080px] divide-y divide-border/70">
        {#each jobs as job (job.id)}
          <div
            class={`relative grid grid-cols-[36px_minmax(300px,1.6fr)_110px_110px_110px_120px_120px] items-center gap-0 px-3 ${rowClass} ${selectedJobIds.has(job.id) ? 'bg-selected' : 'hover:bg-row-hover'}`}
            role="row"
            tabindex="-1"
            oncontextmenu={(event) => { event.preventDefault(); selectRow(job); contextMenu = { jobId: job.id, x: event.clientX, y: event.clientY }; }}
            ondblclick={(event) => event.button === 0 && onOpen(job.id)}
          >
            {#if isActive(job)}
              <div class={`absolute bottom-0 left-0 top-0 opacity-10 ${progressColor(job)}`} style={`width: ${Math.max(0, Math.min(100, job.progress))}%`}></div>
            {/if}
            <label class="relative z-10 flex items-center justify-center">
              <input type="checkbox" checked={selectedJobIds.has(job.id)} oninput={() => selectRow(job)} />
            </label>
            <button class="relative z-10 flex min-w-0 items-center gap-2 text-left" onclick={(event) => selectRow(job, event)}>
              <FileBadge filename={job.filename} transferKind={job.transferKind} size={queueRowSize === 'damn' ? 'lg' : queueRowSize === 'compact' ? 'sm' : 'md'} activityState={job.state === JobState.Completed ? 'completed' : isActive(job) ? 'buffering' : 'none'} />
              <span class="min-w-0">
                <span class="block truncate font-semibold text-foreground" title={job.filename}>{job.filename}</span>
                <span class="block truncate text-[11px] text-muted-foreground" title={job.url}>{getHost(job.url)}</span>
              </span>
            </button>
            <div class="relative z-10 truncate text-xs tabular-nums text-muted-foreground">{job.createdAt ? new Date(job.createdAt).toLocaleDateString() : '--'}</div>
            <div class="relative z-10 truncate text-xs tabular-nums">{job.totalBytes > 0 ? formatBytes(job.totalBytes) : 'Unknown'}</div>
            <div class="relative z-10">
              <div class="mb-1 flex items-center justify-between gap-2 text-[11px] tabular-nums">
                <span>{Math.round(job.progress)}%</span>
                <span class={`rounded border px-1.5 py-0.5 ${statusClass(job)}`}>{job.state}</span>
              </div>
              <div class="h-1.5 overflow-hidden rounded-full bg-progress-track">
                <div class={`h-full rounded-full ${progressColor(job)}`} style={`width: ${Math.max(0, Math.min(100, job.progress))}%`}></div>
              </div>
            </div>
            <div class="relative z-10 truncate text-xs tabular-nums">{job.speed > 0 ? `${formatBytes(job.speed)}/s` : formatTime(job.eta)}</div>
            <div class="relative z-20 flex justify-end gap-1">
              {#if job.state === JobState.Paused}
                <button class="rounded p-1.5 hover:bg-muted" title="Resume" onclick={() => onResume(job.id)}><Play size={15} /></button>
              {:else if isActive(job)}
                <button class="rounded p-1.5 hover:bg-muted" title="Pause" onclick={() => onPause(job.id)}><Pause size={15} /></button>
              {/if}
              {#if job.state === JobState.Failed}
                <button class="rounded p-1.5 hover:bg-muted" title="Retry" onclick={() => onRetry(job.id)}><RotateCw size={15} /></button>
              {/if}
              {#if isActive(job)}
                <button class="rounded p-1.5 hover:bg-muted" title="Cancel" onclick={() => onCancel(job.id)}><X size={15} /></button>
              {/if}
              <button class="rounded p-1.5 hover:bg-muted" title="More" onclick={() => openMenuJobId = openMenuJobId === job.id ? null : job.id}><MoreHorizontal size={16} /></button>
              {#if openMenuJobId === job.id}
                <div class="absolute right-0 top-8 z-30 w-48 rounded border border-border bg-popover py-1 text-xs shadow-xl">
                  {@render MenuItem('Open', ExternalLink, () => onOpen(job.id))}
                  {@render MenuItem('Open Folder', FolderOpen, () => onReveal(job.id))}
                  {#if canShowProgressPopup(job)}{@render MenuItem('Show Popup', ChevronDown, () => onShowPopup(job.id))}{/if}
                  {@render MenuItem('Restart', RotateCw, () => onRestart(job.id))}
                  {#if canSwapFailedDownloadToBrowser(job)}{@render MenuItem('Open in browser', ExternalLink, () => onSwapFailedToBrowser(job.id))}{/if}
                  {@render MenuItem('Rename', Check, () => openRename(job))}
                  {@render MenuItem(deleteActionLabelForJob(job), Trash2, () => openDeletePrompt(job), true)}
                </div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  {#if selectedJob && showDetailsOnClick}
    <aside class="details-pane h-32 shrink-0 overflow-hidden border-t border-border bg-surface px-4 py-2">
      <div class="mb-1 flex items-center justify-between">
        <div class="truncate text-sm font-semibold" title={selectedJob.filename}>{selectedJob.filename}</div>
        <button class="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground" title="Close details" onclick={() => onSelectJob(null)}><X size={14} /></button>
      </div>
      <div class="grid min-w-[1080px] grid-flow-col auto-cols-[minmax(260px,1fr)] grid-rows-2 gap-x-3 gap-y-2 text-xs">
        {@render CompactDetailItem('Path', selectedJob.targetPath || 'No destination recorded yet.')}
        {@render CompactDetailItem('Source', selectedJob.url)}
        {@render CompactDetailItem('State', selectedJob.state)}
        {@render CompactDetailItem('Size', formatBytes(selectedJob.totalBytes))}
        {@render CompactDetailItem('Downloaded', formatBytes(selectedJob.downloadedBytes))}
        {@render CompactDetailItem('ETA', formatTime(selectedJob.eta))}
      </div>
    </aside>
  {/if}
</section>

{#if contextMenu}
  {@const job = jobs.find((candidate) => candidate.id === contextMenu?.jobId)}
  {#if job}
    <button class="fixed inset-0 z-30 cursor-default" aria-label="Close context menu" onclick={() => contextMenu = null}></button>
    <div class="fixed z-40 w-48 rounded border border-border bg-popover py-1 text-xs shadow-xl" style={`left: ${contextMenu.x}px; top: ${contextMenu.y}px;`}>
      {@render MenuItem('Open', ExternalLink, () => onOpen(job.id))}
      {@render MenuItem('Open Folder', FolderOpen, () => onReveal(job.id))}
      {#if canShowProgressPopup(job)}{@render MenuItem('Show Popup', ChevronDown, () => onShowPopup(job.id))}{/if}
      {@render MenuItem('Restart', RotateCw, () => onRestart(job.id))}
      {#if canSwapFailedDownloadToBrowser(job)}{@render MenuItem('Open in browser', ExternalLink, () => onSwapFailedToBrowser(job.id))}{/if}
      {@render MenuItem('Rename', Check, () => openRename(job))}
      {@render MenuItem(deleteActionLabelForJob(job), Trash2, () => openDeletePrompt(job), true)}
    </div>
  {/if}
{/if}

{#if renamePromptJob}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/35">
    <form class="w-[420px] rounded border border-border bg-popover p-4 shadow-2xl" onsubmit={confirmRename}>
      <h2 class="text-sm font-semibold">Rename download</h2>
      <input class="mt-3 w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={renameValue} />
      <div class="mt-4 flex justify-end gap-2">
        <button type="button" class="rounded border border-border px-3 py-1.5 text-xs font-semibold hover:bg-muted" onclick={() => renamePromptJob = null}>Cancel</button>
        <button class="rounded bg-primary px-3 py-1.5 text-xs font-semibold text-primary-foreground">Rename</button>
      </div>
    </form>
  </div>
{/if}

{#if deletePromptJobs.length > 0}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/35">
    <section class="w-[440px] rounded border border-border bg-popover p-4 shadow-2xl">
      <h2 class="text-sm font-semibold">Delete download{deletePromptJobs.length === 1 ? '' : 's'}</h2>
      <p class="mt-2 text-xs text-muted-foreground">Remove {deletePromptJobs.length} selected item{deletePromptJobs.length === 1 ? '' : 's'} from the queue.</p>
      <label class="mt-3 flex items-center gap-2 text-xs">
        <input type="checkbox" bind:checked={deleteFromDisk} />
        Delete downloaded files from disk
      </label>
      <div class="mt-4 flex justify-end gap-2">
        <button class="rounded border border-border px-3 py-1.5 text-xs font-semibold hover:bg-muted" onclick={() => deletePromptJobs = []}>Cancel</button>
        <button class="rounded bg-destructive px-3 py-1.5 text-xs font-semibold text-destructive-foreground" onclick={confirmDelete}>Delete</button>
      </div>
    </section>
  </div>
{/if}

{#snippet MenuItem(label: string, icon: IconComponent, onClick: () => void, danger = false)}
  {@const Icon = icon}
  <button class={`flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-muted ${danger ? 'text-destructive' : 'text-popover-foreground'}`} onclick={() => { onClick(); closeMenus(); }}>
    <Icon size={14} />
    {label}
  </button>
{/snippet}

{#snippet CompactDetailItem(label: string, value: string)}
  <div class="min-w-0 px-1 py-1">
    <div class="text-[10px] uppercase text-muted-foreground">{label}</div>
    <div class="truncate text-xs text-foreground" title={value}>{value}</div>
  </div>
{/snippet}
