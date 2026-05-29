<script lang="ts">
  import type { Component } from 'svelte';
  import {
    ArrowDown,
    ArrowUp,
    ArrowUpDown,
    Check,
    ChevronDown,
    ChevronRight,
    Clock3,
    Download,
    ExternalLink,
    FileText,
    FolderOpen,
    Globe,
    GripHorizontal,
    HardDrive,
    MoreHorizontal,
    Pause,
    Pencil,
    Play,
    RotateCcw,
    RotateCw,
    Trash2,
    Upload,
    Users,
    X,
  } from '@lucide/svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import FileBadge from './FileBadge.svelte';
  import { getDeleteContextMenuLabel, getDeletePromptContent } from './deletePrompts';
  import type { DownloadProgressMetrics } from './downloadProgressMetrics';
  import type { SortMode, SortColumn } from './downloadSorting';
  import { nextSortModeForColumn, sortModeDirection, sortModeKey } from './downloadSorting';
  import {
    canRemoveDownloadImmediately,
    canRetryJob,
    canShowProgressPopup,
    canSwapFailedDownloadToBrowser,
    deleteActionLabelForJob,
    defaultDeleteFromDiskForJobs,
  } from './queueCommands';
  import {
    clampQueueProgress,
    fileBadgeActivityState,
    formatQueueSize,
    formatQueueSizeTitle,
    isBrowserAdoptedTransferKind,
    isBulkQueueView,
    queueStatusPresentation,
    queueTableColumnsForView,
    shouldShowNameProgress,
    torrentActivitySummary,
    torrentDetailMetrics,
    torrentDisplayName,
    type QueueStatusTone,
  } from './queueRowPresentation';
  import {
    isJobArtifactMissing,
    selectJobRange,
    shouldBlurJobIdentity,
    shouldOpenJobFileOnDoubleClick,
  } from './queueInteractions';
  import {
    BULK_MEMBER_ROW_HEIGHT,
    bulkExpansionHeight,
    bulkMemberPanelHeight,
    pruneRecordKeys,
  } from './queueBulkExpansion';
  import {
    formatFullJobDate,
    formatJobDate,
    formatQueueSpeed,
    formatQueueTime,
    formatTorrentRatio,
    formatTorrentSeedMetric,
  } from './queueFormatting';
  import { deleteJobIdsForPrompt, selectedIdsForJob } from './queueSelection';
  import { getVirtualQueueWindow, type VirtualQueueExtraHeight } from './queueVirtualization';
  import {
    BULK_QUEUE_TABLE_GRID_CLASS,
    DETAILS_CLOSE_THRESHOLD,
    DETAILS_DEFAULT_HEIGHT,
    DETAILS_MIN_HEIGHT,
    QUEUE_TABLE_GRID_CLASS,
    clamp,
    detailsLevelForHeight,
    getDetailsMaxHeight,
    queueAlignmentClass,
    queueDateCellClass,
    queueHeaderCellClass,
    queueHeaderSelfClass,
    queueMetricCellClass,
    queueRowSizeClass,
    queueTableCellClass,
    snapDetailsHeight,
    type DetailsLevel,
    type QueueTableAlignment,
  } from './queueViewLayout';
  import { formatBytes, getHost } from './popupShared';
  import type { DownloadJob, QueueRowSize } from './types';
  import { JobState } from './types';
  import { isBulkAggregateJob, type BulkAggregateDownloadJob, type BulkMembersByArchiveId, type QueueDisplayJob } from './bulkQueueRows';

  type IconComponent = Component<{ size?: number; class?: string; strokeWidth?: number }>;

  interface Props {
    jobs: QueueDisplayJob[];
    view: string;
    selectedJobId: string | null;
    showDetailsOnClick: boolean;
    expandActiveBulkRowsByDefault: boolean;
    queueRowSize: QueueRowSize;
    sortMode: SortMode;
    progressMetricsByJobId: Record<string, DownloadProgressMetrics>;
    bulkMembersByArchiveId: BulkMembersByArchiveId;
    pendingActionIds: Set<string>;
    onSortChange: (sortMode: SortMode) => void;
    onSelectJob: (id: string | null) => void;
    onClearSelection: () => void;
    onPause: (id: string) => void;
    onResume: (id: string) => void;
    onCancel: (id: string) => void;
    onRetry: (id: string) => void;
    onRetryBulkMembers: (id: string) => void;
    onRetryArchive: (id: string) => void;
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
    view,
    selectedJobId,
    showDetailsOnClick,
    expandActiveBulkRowsByDefault = false,
    queueRowSize,
    sortMode,
    progressMetricsByJobId,
    bulkMembersByArchiveId = {},
    pendingActionIds = new Set(),
    onSortChange,
    onSelectJob,
    onClearSelection,
    onPause,
    onResume,
    onCancel,
    onRetry,
    onRetryBulkMembers,
    onRetryArchive,
    onRestart,
    onOpen,
    onReveal,
    onShowPopup,
    onSwapFailedToBrowser,
    onRename,
    onDelete,
  }: Props = $props();

  let selectedJobIds = new SvelteSet<string>();
  let expandedBulkRowIds = new SvelteSet<string>();
  let excludedBulkMemberIds = new SvelteSet<string>();
  let contextMenu = $state<{ jobId: string; x: number; y: number } | null>(null);
  let renamePromptJob = $state<QueueDisplayJob | null>(null);
  let renameValue = $state('');
  let deletePromptJobs = $state<QueueDisplayJob[]>([]);
  let deleteFromDisk = $state(false);
  let detailsHeight = $state(DETAILS_DEFAULT_HEIGHT);
  let isResizingDetails = $state(false);
  let isSelectingByDrag = $state(false);
  let queueRoot: HTMLElement | null = $state(null);
  let scrollContainer: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let viewportHeight = $state(0);
  let bulkMemberScrollTops = $state<Record<string, number>>({});
  let bulkMemberViewportHeights = $state<Record<string, number>>({});
  let resizeStart: { y: number; height: number; containerHeight: number; proposedHeight: number } | null = null;
  let selectionDrag: { anchorId: string; selected: boolean; baseSelection: Set<string> } | null = null;

  const selectedJob = $derived(selectedJobId ? jobs.find((job) => job.id === selectedJobId) ?? null : null);
  const selectedJobs = $derived(jobs.filter((job) => selectedJobIds.has(job.id)));
  const tableColumns = $derived(queueTableColumnsForView(view));
  const isBulkTable = $derived(isBulkQueueView(view));
  const isTorrentTable = $derived(tableColumns[2] === 'Seed');
  const tableGridClass = $derived(isBulkTable ? BULK_QUEUE_TABLE_GRID_CLASS : QUEUE_TABLE_GRID_CLASS);
  const rowClass = $derived(queueRowSizeClass(queueRowSize));
  const detailsLevel = $derived(detailsLevelForHeight(detailsHeight));
  const visibleJobIds = $derived(jobs.map((job) => job.id));
  const allVisibleSelected = $derived(jobs.length > 0 && jobs.every((job) => selectedJobIds.has(job.id)));
  const hasVisibleSelection = $derived(jobs.some((job) => selectedJobIds.has(job.id)));
  const expandedRowExtraHeights = $derived.by(() => {
    const extraHeights: VirtualQueueExtraHeight[] = [];
    jobs.forEach((job, index) => {
      const height = bulkExpansionHeightForJob(job);
      if (height > 0) extraHeights.push({ index, height });
    });
    return extraHeights;
  });
  const virtualQueue = $derived(getVirtualQueueWindow({
    totalCount: jobs.length,
    rowSize: queueRowSize,
    scrollTop,
    viewportHeight,
    extraHeights: expandedRowExtraHeights,
  }));
  const renderedJobs = $derived(virtualQueue.enabled ? jobs.slice(virtualQueue.startIndex, virtualQueue.endIndex) : jobs);

  $effect(() => {
    if (!selectedJobId) {
      selectedJobIds.clear();
      return;
    }
    if (selectedJobIds.size === 0 && jobs.some((job) => job.id === selectedJobId)) {
      selectedJobIds.add(selectedJobId);
    }
  });

  $effect(() => {
    const visibleBulkIds = new Set(jobs.filter(canExpandBulkAggregate).map((job) => job.id));
    for (const id of [...expandedBulkRowIds]) {
      if (!visibleBulkIds.has(id)) expandedBulkRowIds.delete(id);
    }
    bulkMemberScrollTops = pruneRecordKeys(bulkMemberScrollTops, visibleBulkIds);
    bulkMemberViewportHeights = pruneRecordKeys(bulkMemberViewportHeights, visibleBulkIds);

    const visibleMemberIds = new Set(jobs.flatMap((job) => isBulkAggregateJob(job) ? job.bulkMemberIds : []));
    for (const id of [...excludedBulkMemberIds]) {
      if (!visibleMemberIds.has(id)) excludedBulkMemberIds.delete(id);
    }

    if (!expandActiveBulkRowsByDefault) return;
    for (const job of jobs) {
      if (canExpandBulkAggregate(job) && [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Paused].includes(job.state)) {
        expandedBulkRowIds.add(job.id);
      }
    }
  });

  $effect(() => {
    if (!isSelectingByDrag) return;
    const stopSelectionDrag = () => {
      isSelectingByDrag = false;
      selectionDrag = null;
    };
    window.addEventListener('pointerup', stopSelectionDrag);
    window.addEventListener('pointercancel', stopSelectionDrag);
    return () => {
      window.removeEventListener('pointerup', stopSelectionDrag);
      window.removeEventListener('pointercancel', stopSelectionDrag);
    };
  });

  $effect(() => {
    if (!contextMenu) return;
    const closeMenu = () => closeMenus();
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeMenus();
    };
    document.addEventListener('click', closeMenu);
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      document.removeEventListener('click', closeMenu);
      document.removeEventListener('keydown', closeOnEscape);
    };
  });

  $effect(() => {
    const root = queueRoot;
    if (!root) return;
    const resizeObserver = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      detailsHeight = clamp(detailsHeight, DETAILS_MIN_HEIGHT, getDetailsMaxHeight(entry.contentRect.height));
    });
    resizeObserver.observe(root);
    return () => resizeObserver.disconnect();
  });

  $effect(() => {
    const container = scrollContainer;
    if (!container) return;
    const updateScrollMetrics = () => {
      scrollTop = container.scrollTop;
      viewportHeight = container.clientHeight;
    };
    updateScrollMetrics();
    const resizeObserver = new ResizeObserver(updateScrollMetrics);
    resizeObserver.observe(container);
    return () => resizeObserver.disconnect();
  });

  $effect(() => {
    if (!isResizingDetails) return;

    const resizeFromClientY = (clientY: number) => {
      if (!resizeStart) return;
      const nextHeight = resizeStart.height + resizeStart.y - clientY;
      const maxHeight = getDetailsMaxHeight(resizeStart.containerHeight);
      resizeStart.proposedHeight = nextHeight;
      if (nextHeight < DETAILS_CLOSE_THRESHOLD) {
        resizeStart = null;
        isResizingDetails = false;
        clearJobSelection();
        return;
      }
      detailsHeight = snapDetailsHeight(nextHeight, maxHeight);
    };

    const resizePointer = (event: PointerEvent) => resizeFromClientY(event.clientY);
    const stopResize = () => {
      const start = resizeStart;
      resizeStart = null;
      isResizingDetails = false;
      if (!start) return;
      if (start.proposedHeight < DETAILS_CLOSE_THRESHOLD) {
        clearJobSelection();
        return;
      }
      detailsHeight = snapDetailsHeight(start.proposedHeight, getDetailsMaxHeight(start.containerHeight));
    };

    window.addEventListener('pointermove', resizePointer);
    window.addEventListener('pointerup', stopResize);
    window.addEventListener('pointercancel', stopResize);
    return () => {
      window.removeEventListener('pointermove', resizePointer);
      window.removeEventListener('pointerup', stopResize);
      window.removeEventListener('pointercancel', stopResize);
    };
  });

  function setSort(column: SortColumn) {
    onSortChange(nextSortModeForColumn(sortMode, column));
  }

  function selectSingleJob(jobId: string) {
    selectedJobIds.clear();
    selectedJobIds.add(jobId);
    onSelectJob(jobId);
  }

  function clearJobSelection() {
    selectedJobIds.clear();
    onClearSelection();
  }

  function toggleSingleJobSelection(jobId: string) {
    const isOnlySelectedJob = selectedJobId === jobId && selectedJobIds.size === 1 && selectedJobIds.has(jobId);
    if (isOnlySelectedJob) {
      clearJobSelection();
      return;
    }
    selectSingleJob(jobId);
  }

  function setJobSelection(jobId: string, selected: boolean) {
    if (selected) selectedJobIds.add(jobId);
    else selectedJobIds.delete(jobId);

    if (selectedJobIds.size === 0) {
      onClearSelection();
      return;
    }

    if (selected || selectedJobId === jobId) {
      onSelectJob(selected ? jobId : Array.from(selectedJobIds)[0]);
    }
  }

  function setAllVisibleSelected(selected: boolean) {
    selectedJobIds.clear();
    if (selected) {
      for (const jobId of visibleJobIds) selectedJobIds.add(jobId);
    }
    const firstSelected = Array.from(selectedJobIds)[0];
    if (firstSelected) onSelectJob(firstSelected);
    else onClearSelection();
  }

  function toggleBulkRow(jobId: string) {
    if (expandedBulkRowIds.has(jobId)) {
      expandedBulkRowIds.delete(jobId);
      return;
    }
    expandedBulkRowIds.add(jobId);
  }

  function bulkMembersForJob(job: QueueDisplayJob): DownloadJob[] {
    return isBulkAggregateJob(job) ? bulkMembersByArchiveId[job.bulkArchiveId] ?? [] : [];
  }

  function bulkExpansionHeightForJob(job: QueueDisplayJob): number {
    if (!isBulkTable || !canExpandBulkAggregate(job) || !expandedBulkRowIds.has(job.id)) return 0;
    return bulkExpansionHeight(bulkMembersForJob(job).length);
  }

  function bulkMemberVirtualQueue(job: QueueDisplayJob, memberCount: number) {
    return getVirtualQueueWindow({
      totalCount: memberCount,
      rowSize: 'compact',
      rowHeightOverride: BULK_MEMBER_ROW_HEIGHT,
      scrollTop: bulkMemberScrollTops[job.id] ?? 0,
      viewportHeight: bulkMemberViewportHeights[job.id] ?? bulkMemberPanelHeight(memberCount),
    });
  }

  function updateBulkMemberPanelMetrics(jobId: string, element: HTMLElement) {
    const nextScrollTop = element.scrollTop;
    const nextViewportHeight = element.clientHeight;
    if (bulkMemberScrollTops[jobId] !== nextScrollTop) {
      bulkMemberScrollTops = { ...bulkMemberScrollTops, [jobId]: nextScrollTop };
    }
    if (bulkMemberViewportHeights[jobId] !== nextViewportHeight) {
      bulkMemberViewportHeights = { ...bulkMemberViewportHeights, [jobId]: nextViewportHeight };
    }
  }

  function canExcludeBulkReviewMember(member: DownloadJob): boolean {
    return [JobState.Queued, JobState.Paused].includes(member.state);
  }

  function isBulkReviewGroup(job: QueueDisplayJob): boolean {
    return isBulkAggregateJob(job)
      && job.bulkArchive?.archiveStatus === 'pending'
      && bulkMembersForJob(job).some(canExcludeBulkReviewMember);
  }

  function setBulkMemberIncluded(memberId: string, included: boolean) {
    if (included) excludedBulkMemberIds.delete(memberId);
    else excludedBulkMemberIds.add(memberId);
  }

  function includedBulkMemberCount(job: BulkAggregateDownloadJob): number {
    return bulkMembersForJob(job).filter((member) => !excludedBulkMemberIds.has(member.id)).length;
  }

  function canShowBulkPrimaryAction(job: BulkAggregateDownloadJob): boolean {
    return isBulkReviewGroup(job) || job.state === JobState.Paused;
  }

  function bulkPrimaryActionLabel(job: BulkAggregateDownloadJob): string {
    return isBulkReviewGroup(job) ? 'Start' : 'Resume';
  }

  function bulkPrimaryActionDisabled(job: BulkAggregateDownloadJob): boolean {
    return isBulkReviewGroup(job) && includedBulkMemberCount(job) === 0;
  }

  function runBulkPrimaryAction(job: BulkAggregateDownloadJob) {
    if (isBulkReviewGroup(job)) {
      startBulkReview(job);
      return;
    }
    onResume(job.id);
  }

  function startBulkReview(job: BulkAggregateDownloadJob) {
    const members = bulkMembersForJob(job);
    const excludedIds = members
      .filter((member) => excludedBulkMemberIds.has(member.id) && canExcludeBulkReviewMember(member))
      .map((member) => member.id);
    const includedPausedIds = members
      .filter((member) => !excludedBulkMemberIds.has(member.id) && [JobState.Paused, JobState.Failed, JobState.Canceled].includes(member.state))
      .map((member) => member.id);

    if (excludedIds.length > 0) onDelete(excludedIds, false);
    for (const memberId of includedPausedIds) onResume(memberId);
    for (const memberId of excludedIds) excludedBulkMemberIds.delete(memberId);
  }

  function applySelectionRange(anchorId: string, currentId: string, selected: boolean, baseSelection = new Set(selectedJobIds)) {
    const range = selectJobRange(visibleJobIds, anchorId, currentId);
    if (range.length === 0) return;

    selectedJobIds.clear();
    for (const jobId of baseSelection) selectedJobIds.add(jobId);
    for (const jobId of range) {
      if (selected) selectedJobIds.add(jobId);
      else selectedJobIds.delete(jobId);
    }

    if (selected) {
      onSelectJob(currentId);
    } else if (selectedJobIds.size === 0) {
      onClearSelection();
    } else {
      onSelectJob(Array.from(selectedJobIds)[0]);
    }
  }

  function startSelectionDrag(jobId: string, selected: boolean) {
    selectionDrag = { anchorId: jobId, selected, baseSelection: new Set(selectedJobIds) };
    isSelectingByDrag = true;
    setJobSelection(jobId, selected);
  }

  function continueSelectionDrag(jobId: string) {
    if (!selectionDrag) return;
    applySelectionRange(selectionDrag.anchorId, jobId, selectionDrag.selected, selectionDrag.baseSelection);
  }

  function selectedJobsFor(job: QueueDisplayJob): QueueDisplayJob[] {
    const ids = new Set(selectedIdsForJob(job, selectedJobIds));
    return jobs.filter((candidate) => ids.has(candidate.id));
  }

  function openDeletePromptForJobs(jobs: QueueDisplayJob[]) {
    deletePromptJobs = jobs;
    deleteFromDisk = defaultDeleteFromDiskForJobs(deletePromptJobs);
    closeMenus();
  }

  function openDeletePrompt(job: DownloadJob) {
    openDeletePromptForJobs(selectedJobsFor(job));
  }

  function openDeleteFromDiskPrompt(job: QueueDisplayJob) {
    deletePromptJobs = selectedJobsFor(job);
    deleteFromDisk = true;
    closeMenus();
  }

  function confirmDelete() {
    onDelete(deleteJobIdsForPrompt(deletePromptJobs), deleteFromDisk);
    deletePromptJobs = [];
  }

  function openRename(job: DownloadJob) {
    renamePromptJob = job;
    renameValue = job.filename;
    closeMenus();
  }

  function confirmRename(event: SubmitEvent) {
    event.preventDefault();
    if (!renamePromptJob || !renameValue.trim()) return;
    onRename(renamePromptJob.id, renameValue.trim());
    renamePromptJob = null;
  }

  function closeMenus() {
    contextMenu = null;
  }

  function openJobMenu(job: QueueDisplayJob, x: number, y: number) {
    const isSelectedMultiContext = selectedJobIds.size > 1 && selectedJobIds.has(job.id);
    if (!isSelectedMultiContext) selectSingleJob(job.id);
    contextMenu = getContextMenuPosition(job.id, x, y);
  }

  function openContextMenu(job: QueueDisplayJob, event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    openJobMenu(job, event.clientX, event.clientY);
  }

  function openActionsMenu(job: QueueDisplayJob, event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    const target = event.currentTarget;
    if (!(target instanceof HTMLElement)) return;
    const rect = target.getBoundingClientRect();
    openJobMenu(job, Math.round(rect.right - 192), Math.round(rect.bottom + 4));
  }

  function startDetailsResize(clientY: number) {
    if (resizeStart) return;
    const containerHeight = queueRoot?.clientHeight ?? window.innerHeight;
    resizeStart = {
      y: clientY,
      height: detailsHeight,
      containerHeight,
      proposedHeight: detailsHeight,
    };
    isResizingDetails = true;
  }

  function statusBadgeClass(tone: QueueStatusTone) {
    switch (tone) {
      case 'success':
        return 'border-success/35 bg-success/10 text-success';
      case 'destructive':
        return 'border-destructive/35 bg-destructive/10 text-destructive';
      case 'warning':
        return 'border-warning/35 bg-warning/10 text-warning';
      case 'primary':
        return 'border-primary/35 bg-primary/10 text-primary';
      default:
        return 'border-border bg-muted text-muted-foreground';
    }
  }

  function nameProgressClass(tone: QueueStatusTone) {
    switch (tone) {
      case 'destructive':
        return 'bg-destructive/20';
      case 'warning':
        return 'bg-warning/20';
      case 'muted':
        return 'bg-muted';
      default:
        return 'bg-primary/30';
    }
  }

  function inlineNameDensity(size: QueueRowSize): {
    container: string;
    progressInset: string;
    titleGap: string;
    titleText: string;
    metaText: string;
  } {
    switch (size) {
      case 'compact':
        return {
          container: 'px-2 py-0',
          progressInset: 'inset-y-0.5',
          titleGap: 'gap-1.5',
          titleText: 'text-xs leading-4',
          metaText: 'mt-0 text-[10px] leading-3',
        };
      case 'small':
        return {
          container: 'px-2 py-0.5',
          progressInset: 'inset-y-0.5',
          titleGap: 'gap-1.5',
          titleText: 'text-xs leading-4',
          metaText: 'mt-0 text-[11px] leading-3',
        };
      case 'large':
        return {
          container: 'px-2 py-1.5',
          progressInset: 'inset-y-1',
          titleGap: 'gap-2',
          titleText: 'text-sm leading-5',
          metaText: 'mt-0.5 text-xs leading-4',
        };
      case 'damn':
        return {
          container: 'px-2 py-2',
          progressInset: 'inset-y-1',
          titleGap: 'gap-2',
          titleText: 'text-base leading-6',
          metaText: 'mt-0.5 text-sm leading-5',
        };
      case 'medium':
      default:
        return {
          container: 'px-2 py-1',
          progressInset: 'inset-y-1',
          titleGap: 'gap-2',
          titleText: 'text-sm leading-5',
          metaText: 'mt-0.5 text-xs leading-4',
        };
    }
  }

  function statusBadgeDensity(size: QueueRowSize): string {
    switch (size) {
      case 'compact':
        return 'px-1 py-0 text-[9px] leading-3';
      case 'small':
        return 'px-1 py-0 text-[9px] leading-3';
      case 'large':
        return 'px-1.5 py-[1px] text-[10px] leading-4';
      case 'damn':
        return 'px-2 py-[2px] text-[11px] leading-4';
      case 'medium':
      default:
        return 'px-1.5 py-[1px] text-[10px] leading-4';
    }
  }

  function torrentMetricValue(metric: ReturnType<typeof torrentDetailMetrics>[number]) {
    if (metric.kind === 'upload') return `${formatBytes(metric.value)}/s`;
    if (metric.kind === 'peers') return `${metric.value} peers`;
    return `${metric.value} seeds`;
  }

  function torrentDetailTitle(metrics: ReturnType<typeof torrentDetailMetrics>) {
    return metrics.map((metric) => `${metric.label}: ${torrentMetricValue(metric)}`).join(' / ');
  }

  function torrentMetricTextClass(kind: ReturnType<typeof torrentDetailMetrics>[number]['kind']) {
    return kind === 'peers' ? 'text-sky-300' : 'text-fuchsia-300';
  }

  function torrentMetricIconClass(kind: ReturnType<typeof torrentDetailMetrics>[number]['kind']) {
    return kind === 'peers' ? 'text-sky-400' : 'text-fuchsia-400';
  }

  function sortIcon(column: string): IconComponent {
    const sortColumn = columnToSortColumn(column);
    if (!sortColumn || sortModeKey(sortMode) !== sortColumn) return ArrowUpDown;
    return sortModeDirection(sortMode) === 'asc' ? ArrowUp : ArrowDown;
  }

  function sortableHeaderClass(column: SortColumn, alignment: QueueTableAlignment = 'start') {
    const active = sortModeKey(sortMode) === column;
    return `inline-flex w-fit max-w-full items-center gap-1 transition ${queueHeaderSelfClass(alignment)} ${queueAlignmentClass(alignment)} ${
      active
        ? 'text-primary'
        : 'text-muted-foreground hover:text-foreground'
    }`;
  }

  function sortableHeaderTitle(column: SortColumn) {
    const active = sortModeKey(sortMode) === column;
    const nextDirection = active && sortModeDirection(sortMode) === 'asc' ? 'descending' : 'ascending';
    return `Sort by ${column} ${nextDirection}`;
  }

  function columnToSortColumn(column: string): SortColumn | null {
    if (column === 'Name') return 'name';
    if (column === 'Date') return 'date';
    if (column === 'Size') return 'size';
    return null;
  }

  function getContextMenuPosition(jobId: string, x: number, y: number) {
    const menuWidth = 192;
    const menuHeight = 180;
    return {
      jobId,
      x: clamp(x, 8, Math.max(8, window.innerWidth - menuWidth - 8)),
      y: clamp(y, 8, Math.max(8, window.innerHeight - menuHeight - 8)),
    };
  }

  function isActive(job: DownloadJob): boolean {
    return [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding].includes(job.state);
  }

  function isRemoving(job: QueueDisplayJob) {
    return job.removalState === 'removing';
  }

  function isCleanupFailed(job: QueueDisplayJob) {
    return job.removalState === 'cleanup_failed';
  }

  function isActionPending(job: QueueDisplayJob): boolean {
    if (isRemoving(job)) return true;
    if (pendingActionIds.has(job.id)) return true;
    return isBulkAggregateJob(job) && job.bulkMemberIds.some((memberId) => pendingActionIds.has(memberId));
  }

  function isCompletedBulkAggregate(job: DownloadJob): boolean {
    return isBulkAggregateJob(job) && job.state === JobState.Completed && Boolean(job.bulkArchiveOutputPath);
  }

  function canExpandBulkAggregate(job: QueueDisplayJob): job is BulkAggregateDownloadJob {
    return isBulkAggregateJob(job) && !isRemoving(job) && !isCompletedBulkAggregate(job);
  }

  function isCanceledBulkAggregate(job: DownloadJob): boolean {
    return isBulkAggregateJob(job) && job.state === JobState.Canceled;
  }

  function isFailedBulkAggregate(job: DownloadJob): boolean {
    return isBulkAggregateJob(job) && job.bulkArchive?.archiveStatus === 'failed';
  }

  function canRetryBulkMembers(job: DownloadJob): boolean {
    return isBulkAggregateJob(job) && job.bulkRetryableMemberCount > 0;
  }

  function canOpenJobFile(job: QueueDisplayJob): boolean {
    return Boolean(job.targetPath?.trim())
      && (job.state === JobState.Completed || job.state === JobState.Seeding);
  }

  function canOpenSelectedDeletePrompt(job: QueueDisplayJob): boolean {
    if (isRemoving(job)) return false;
    return !isBulkAggregateJob(job) || isCanceledBulkAggregate(job);
  }

  function showsEtaMetrics(job: DownloadJob): boolean {
    return !isBulkAggregateJob(job);
  }
</script>

{#if jobs.length === 0}
  <div class="flex min-h-0 flex-1 items-center justify-center bg-surface p-8">
    <div class="max-w-sm text-center">
      <div class="mx-auto mb-5 flex h-16 w-16 items-center justify-center rounded-md border border-border bg-card text-primary">
        <Download size={32} />
      </div>
      <h2 class="mb-2 text-lg font-semibold text-foreground">No downloads</h2>
      <p class="text-sm leading-6 text-muted-foreground">Downloads from the browser extension or the New Download command will appear in this list.</p>
    </div>
  </div>
{:else}
  <section bind:this={queueRoot} class="flex min-h-0 flex-1 flex-col bg-surface">
    <div
      bind:this={scrollContainer}
      class="min-h-0 flex-1 overflow-auto"
      onscroll={(event) => {
        scrollTop = event.currentTarget.scrollTop;
        viewportHeight = event.currentTarget.clientHeight;
      }}
    >
      <div class="download-table min-w-[980px] overflow-visible border-b border-t border-border bg-card">
        <div class={`grid ${tableGridClass} border-b border-border bg-header px-3 py-1.5 text-xs font-medium text-muted-foreground`}>
          <div class="flex items-center gap-3">
            <input
              type="checkbox"
              checked={allVisibleSelected}
              title={allVisibleSelected ? 'Clear selection' : 'Select all downloads'}
              aria-label={allVisibleSelected ? 'Clear selection' : 'Select all downloads'}
              class="h-3.5 w-3.5 shrink-0 cursor-pointer accent-primary"
              onclick={(event) => event.stopPropagation()}
              oninput={(event) => setAllVisibleSelected(event.currentTarget.checked)}
            />
            {#each [tableColumns[0]] as column}
              {@const SortIcon = sortIcon(column)}
              <button type="button" aria-pressed={sortModeKey(sortMode) === 'name'} title={sortableHeaderTitle('name')} class={sortableHeaderClass('name')} onclick={() => setSort('name')}>
                {column}
                <SortIcon size={12} />
              </button>
            {/each}
          </div>
          {#each [tableColumns[1]] as column}
            {@const SortIcon = sortIcon(column)}
            <button type="button" aria-pressed={sortModeKey(sortMode) === 'date'} title={sortableHeaderTitle('date')} class={sortableHeaderClass('date', 'center')} onclick={() => setSort('date')}>
              {column}
              <SortIcon size={12} />
            </button>
          {/each}
          <div class={queueHeaderCellClass('center')} title={isTorrentTable ? 'Seed upload speed' : undefined}>{tableColumns[2]}</div>
          {#if !isBulkTable}
            <div class={queueHeaderCellClass('center')} title={isTorrentTable ? 'Share ratio' : undefined}>{tableColumns[3]}</div>
          {/if}
          {#each [tableColumns[isBulkTable ? 3 : 4]] as column}
            {@const SortIcon = sortIcon(column)}
            <button type="button" aria-pressed={sortModeKey(sortMode) === 'size'} title={sortableHeaderTitle('size')} class={sortableHeaderClass('size', 'center')} onclick={() => setSort('size')}>
              {column}
              <SortIcon size={12} />
            </button>
          {/each}
          <div class="text-right">{tableColumns[isBulkTable ? 4 : 5]}</div>
        </div>

        {#if hasVisibleSelection && selectedJobIds.size > 1}
          <div class="flex h-9 items-center justify-between border-b border-border bg-primary-soft px-3 text-xs text-primary">
            <span>{selectedJobIds.size} downloads selected</span>
            <div class="flex items-center gap-2">
              {#if selectedJobs.every((job) => canOpenSelectedDeletePrompt(job))}
                <button type="button" class="rounded px-2 py-1 font-semibold hover:bg-primary/10" onclick={() => openDeletePromptForJobs(selectedJobs)}>Delete All</button>
              {/if}
              <button type="button" class="rounded px-2 py-1 font-semibold hover:bg-primary/10" onclick={clearJobSelection}>Clear</button>
            </div>
          </div>
        {/if}

        {#if virtualQueue.enabled}
          <div style={`height: ${virtualQueue.topPadding}px;`}></div>
        {/if}

        <div class="divide-y divide-border/70">
          {#each renderedJobs as job (job.id)}
            {@const metrics = progressMetricsByJobId[job.id]}
            {@const selected = job.id === selectedJob?.id}
            {@const multiSelected = selectedJobIds.has(job.id)}
            {@const rowSelected = selected || multiSelected}
            {@const artifactMissing = isJobArtifactMissing(job)}
            {@const blurIdentity = shouldBlurJobIdentity(job)}
            {@const averageSpeed = metrics?.averageSpeed ?? job.speed}
            {@const timeRemaining = metrics?.timeRemaining ?? job.eta}
            {@const statusPresentation = queueStatusPresentation(job)}
            <div
              class={`grid w-full ${tableGridClass} items-center gap-0 px-3 text-left transition-colors ${rowClass} ${rowSelected ? 'bg-selected outline outline-1 outline-primary/30' : 'bg-card hover:bg-row-hover'} ${artifactMissing ? 'opacity-45 grayscale' : ''}`}
              role="button"
              tabindex="0"
              style={virtualQueue.enabled ? `height: ${virtualQueue.rowHeight}px;` : undefined}
              onclick={() => {
                toggleSingleJobSelection(job.id);
                closeMenus();
              }}
              ondblclick={(event) => {
                if (!shouldOpenJobFileOnDoubleClick(job, event.button)) return;
                event.preventDefault();
                selectSingleJob(job.id);
                closeMenus();
                onOpen(job.id);
              }}
              oncontextmenu={(event) => openContextMenu(job, event)}
              onkeydown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault();
                  toggleSingleJobSelection(job.id);
                }
              }}
              onpointerenter={() => continueSelectionDrag(job.id)}
            >
              <div class="flex min-w-0 items-center gap-3 pr-4">
                {#if isBulkTable && canExpandBulkAggregate(job)}
                  {@const BulkChevron = expandedBulkRowIds.has(job.id) ? ChevronDown : ChevronRight}
                  <button
                    type="button"
                    class="-mr-2 flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground transition hover:bg-muted hover:text-foreground"
                    title={expandedBulkRowIds.has(job.id) ? 'Collapse files' : 'Show files'}
                    aria-label={expandedBulkRowIds.has(job.id) ? 'Collapse files' : 'Show files'}
                    onclick={(event) => {
                      event.stopPropagation();
                      toggleBulkRow(job.id);
                    }}
                  >
                    <BulkChevron size={16} />
                  </button>
                {/if}
                <FileBadge
                  filename={job.filename}
                  transferKind={job.transferKind}
                  selected={multiSelected}
                  selectionTitle={multiSelected ? `Deselect ${job.filename}` : `Select ${job.filename}`}
                  onSelectionChange={(checked) => setJobSelection(job.id, checked)}
                  onSelectionPointerDown={(event) => {
                    event.stopPropagation();
                    startSelectionDrag(job.id, !selectedJobIds.has(job.id));
                  }}
                  muted={artifactMissing}
                  blurred={blurIdentity}
                  rowSize={queueRowSize}
                  activityState={fileBadgeActivityState(job, false)}
                />
                {@render InlineNameProgress(job, statusPresentation, artifactMissing, blurIdentity, queueRowSize)}
              </div>

              <div class={queueDateCellClass()} title={formatFullJobDate(job.createdAt)}>
                {formatJobDate(job.createdAt)}
              </div>

              <div class={queueMetricCellClass()}>
                {isTorrentTable ? formatTorrentSeedMetric(job) : formatQueueSpeed(job, averageSpeed)}
              </div>
              {#if !isBulkTable}
                <div class={queueTableCellClass('center')}>
                  {isTorrentTable ? formatTorrentRatio(job) : formatQueueTime(job, timeRemaining)}
                </div>
              {/if}
              <div class={queueTableCellClass('center')} title={formatQueueSizeTitle(job, formatBytes)}>
                {formatQueueSize(job, formatBytes)}
              </div>

              <div
                class="relative flex items-center justify-end gap-1"
                role="presentation"
                onclick={(event) => event.stopPropagation()}
                ondblclick={(event) => event.stopPropagation()}
                onkeydown={(event) => event.stopPropagation()}
              >
                {#if isRemoving(job)}
                  <button class="flex h-8 w-8 cursor-not-allowed items-center justify-center rounded-md border border-transparent bg-transparent text-muted-foreground opacity-55" title="Removing files" aria-label="Removing files" disabled><Trash2 size={17} /></button>
                {:else}
                {#if isCompletedBulkAggregate(job)}
                  <button class="flex h-8 w-8 items-center justify-center rounded-md border border-transparent bg-transparent text-muted-foreground transition-colors hover:border-border hover:bg-muted hover:text-foreground" title="Show" aria-label="Show" onclick={() => onReveal(job.id)}><FolderOpen size={17} /></button>
                {:else if isBulkAggregateJob(job) && canShowBulkPrimaryAction(job)}
                  <button
                    class="flex h-8 w-8 items-center justify-center rounded-md border border-transparent bg-transparent text-muted-foreground transition-colors hover:border-border hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-45"
                    title={bulkPrimaryActionLabel(job)}
                    aria-label={bulkPrimaryActionLabel(job)}
                    disabled={bulkPrimaryActionDisabled(job) || isActionPending(job)}
                    onclick={() => runBulkPrimaryAction(job)}
                  >
                    <Play size={17} />
                  </button>
                {:else if job.state === JobState.Paused}
                  <button class="flex h-8 w-8 items-center justify-center rounded-md border border-transparent bg-transparent text-muted-foreground transition-colors hover:border-border hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-45" title="Resume" aria-label="Resume" disabled={isActionPending(job)} onclick={() => onResume(job.id)}><Play size={17} /></button>
                {:else if isActive(job)}
                  <button class="flex h-8 w-8 items-center justify-center rounded-md border border-transparent bg-transparent text-muted-foreground transition-colors hover:border-border hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-45" title="Pause" aria-label="Pause" disabled={isActionPending(job)} onclick={() => onPause(job.id)}><Pause size={17} /></button>
                {/if}
                {#if !isCleanupFailed(job) && !isBulkAggregateJob(job) && [JobState.Failed, JobState.Canceled].includes(job.state)}
                  <button class="flex h-8 w-8 items-center justify-center rounded-md border border-transparent bg-transparent text-muted-foreground transition-colors hover:border-border hover:bg-muted hover:text-foreground" title="Retry" aria-label="Retry" onclick={() => onRetry(job.id)}><RotateCw size={17} /></button>
                {/if}
                {#if !isBulkAggregateJob(job) && canSwapFailedDownloadToBrowser(job)}
                  <button class="flex h-8 w-8 items-center justify-center rounded-md border border-transparent bg-transparent text-muted-foreground transition-colors hover:border-border hover:bg-muted hover:text-foreground" title="Swap" aria-label="Swap" onclick={() => onSwapFailedToBrowser(job.id)}><ExternalLink size={17} /></button>
                {/if}
                {#if !isCompletedBulkAggregate(job)}
                  <button
                    class="flex h-8 w-8 items-center justify-center rounded-md border border-transparent bg-transparent text-muted-foreground transition-colors hover:border-border hover:bg-muted hover:text-foreground"
                    title="More actions"
                    aria-label="More actions"
                    onclick={(event) => openActionsMenu(job, event)}
                  >
                    <MoreHorizontal size={18} />
                  </button>
                {/if}
                {/if}
              </div>
            </div>
            {#if isBulkTable && canExpandBulkAggregate(job) && expandedBulkRowIds.has(job.id)}
              {@const bulkMembers = bulkMembersForJob(job)}
              {@const memberWindow = bulkMemberVirtualQueue(job, bulkMembers.length)}
              {@const renderedBulkMembers = memberWindow.enabled ? bulkMembers.slice(memberWindow.startIndex, memberWindow.endIndex) : bulkMembers}
              <div class="overflow-hidden border-t border-border/45 bg-background/55 px-2 py-1" style={`height: ${bulkExpansionHeight(bulkMembers.length)}px;`}>
                <div
                  class="ml-8 h-full overflow-y-auto overscroll-contain border-y border-border/50 bg-card/70"
                  style={`max-height: ${bulkMemberPanelHeight(bulkMembers.length)}px;`}
                  onscroll={(event) => updateBulkMemberPanelMetrics(job.id, event.currentTarget)}
                >
                  {#if memberWindow.enabled}
                    <div style={`height: ${memberWindow.topPadding}px;`}></div>
                  {/if}
                  {#each renderedBulkMembers as bulkMember (bulkMember.id)}
                    {@const memberMetrics = progressMetricsByJobId[bulkMember.id]}
                    {@const memberAverageSpeed = memberMetrics?.averageSpeed ?? bulkMember.speed}
                    {@const memberPresentation = queueStatusPresentation(bulkMember)}
                    {@const memberIncluded = !excludedBulkMemberIds.has(bulkMember.id)}
                    <div class={`grid ${BULK_QUEUE_TABLE_GRID_CLASS} items-center gap-0 border-t border-border/40 px-2 py-1 text-[11px] leading-4 first:border-t-0 ${memberIncluded ? '' : 'opacity-55'}`} style={`height: ${BULK_MEMBER_ROW_HEIGHT}px;`}>
                      <div class="flex min-w-0 items-center gap-1.5 pr-3">
                        {#if isBulkReviewGroup(job)}
                          <input
                            type="checkbox"
                            class="h-3 w-3 shrink-0 accent-primary disabled:cursor-not-allowed disabled:opacity-45"
                            checked={memberIncluded}
                            disabled={!canExcludeBulkReviewMember(bulkMember)}
                            title={memberIncluded ? 'Include file' : 'Exclude file'}
                            aria-label={memberIncluded ? `Include ${bulkMember.filename}` : `Exclude ${bulkMember.filename}`}
                            onclick={(event) => event.stopPropagation()}
                            oninput={(event) => setBulkMemberIncluded(bulkMember.id, event.currentTarget.checked)}
                          />
                        {/if}
                        <div class="flex min-w-0 flex-1 items-center gap-2">
                          <span class="truncate font-medium text-foreground" title={bulkMember.filename}>{bulkMember.filename}</span>
                          {@render QueueStatusBadge(memberPresentation, 'compact')}
                          <span class="truncate text-muted-foreground" title={bulkMember.url}>{getHost(bulkMember.url)}</span>
                        </div>
                      </div>
                      <div class={queueDateCellClass()} title={formatFullJobDate(bulkMember.createdAt)}>
                        {formatJobDate(bulkMember.createdAt)}
                      </div>
                      <div class={queueMetricCellClass()}>
                        {formatQueueSpeed(bulkMember, memberAverageSpeed)}
                      </div>
                      <div class={queueTableCellClass('center')} title={formatQueueSizeTitle(bulkMember, formatBytes)}>
                        {formatQueueSize(bulkMember, formatBytes)}
                      </div>
                      <div class="flex items-center justify-end">
                        {#if bulkMember.state === JobState.Paused && memberIncluded}
                          <button type="button" class="h-6 rounded px-2 text-[11px] font-medium text-primary transition hover:bg-primary/10 disabled:cursor-not-allowed disabled:opacity-45" title="Resume file" aria-label="Resume file" disabled={pendingActionIds.has(bulkMember.id)} onclick={(event) => { event.stopPropagation(); onResume(bulkMember.id); }}>Resume</button>
                        {/if}
                      </div>
                    </div>
                  {/each}
                  {#if memberWindow.enabled}
                    <div style={`height: ${memberWindow.bottomPadding}px;`}></div>
                  {/if}
                </div>
              </div>
            {/if}
          {/each}
        </div>

        {#if virtualQueue.enabled}
          <div style={`height: ${virtualQueue.bottomPadding}px;`}></div>
        {/if}
      </div>
    </div>

    {#if selectedJob && showDetailsOnClick}
      <aside class="details-pane relative flex shrink-0 flex-col overflow-hidden border-t border-border bg-card/95 px-4 pb-2 pt-3 shadow-[0_-10px_24px_rgba(0,0,0,0.22)]" style={`height: ${detailsHeight}px;`}>
        <button type="button" class="absolute left-0 right-0 top-0 flex h-3 cursor-row-resize items-center justify-center text-muted-foreground hover:text-foreground focus:outline-none focus-visible:text-primary" title="Resize details" aria-label="Resize details" onpointerdown={(event) => startDetailsResize(event.clientY)}>
          <GripHorizontal size={16} />
        </button>
        <div class="mb-2 flex items-start justify-between gap-4">
          <div class="flex min-w-0 items-start gap-3">
            <FileBadge
              filename={selectedJob.filename}
              transferKind={selectedJob.transferKind}
              activityState={fileBadgeActivityState(selectedJob, false)}
            />
            <div class="min-w-0">
              <div class="truncate text-sm font-semibold" title={selectedJob.filename}>{torrentDisplayName(selectedJob)}</div>
              {@render DetailsHeaderMetrics(selectedJob)}
            </div>
          </div>
          <button class="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground" title="Close details" onclick={clearJobSelection}><X size={14} /></button>
        </div>
        <div class="min-h-0 flex-1 overflow-auto border-t border-border/45 pt-2">
          {#if detailsLevel === 'compact'}
            {@render DetailsCompactLine(selectedJob)}
          {:else}
            {@render DetailsGrid(selectedJob, detailsLevel)}
          {/if}
        </div>
      </aside>
    {/if}
  </section>
{/if}

{#if contextMenu}
  {@const job = jobs.find((candidate) => candidate.id === contextMenu?.jobId)}
  {#if job}
    <button class="fixed inset-0 z-30 cursor-default" aria-label="Close context menu" onclick={() => contextMenu = null}></button>
    <div class="fixed z-[70] w-48 overflow-hidden rounded-md border border-border bg-card py-1 shadow-2xl" role="menu" tabindex="-1" style={`left: ${contextMenu.x}px; top: ${contextMenu.y}px;`} onclick={(event) => event.stopPropagation()} onkeydown={(event) => event.stopPropagation()}>
      {@render RowMenu(job)}
    </div>
  {/if}
{/if}

{#if renamePromptJob}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/35">
    <form class="w-[420px] rounded-md border border-border bg-card p-4 shadow-2xl" onsubmit={confirmRename}>
      <h2 class="text-sm font-semibold">Rename download</h2>
      <input class="mt-3 w-full rounded-md border border-input bg-background px-3 py-2 text-sm" bind:value={renameValue} />
      <div class="mt-4 flex justify-end gap-2">
        <button type="button" class="rounded-md border border-border px-3 py-1.5 text-xs font-semibold hover:bg-muted" onclick={() => renamePromptJob = null}>Cancel</button>
        <button class="rounded-md bg-primary px-3 py-1.5 text-xs font-semibold text-primary-foreground">Rename</button>
      </div>
    </form>
  </div>
{/if}

{#if deletePromptJobs.length > 0}
  {@const prompt = getDeletePromptContent(deletePromptJobs.length)}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/35">
    <section class="w-[440px] rounded-md border border-border bg-card p-4 shadow-2xl">
      <h2 class="text-sm font-semibold">{prompt.title}</h2>
      <p class="mt-2 text-xs text-muted-foreground">{prompt.description}</p>
      <label class="mt-3 flex items-center gap-2 text-xs">
        <input type="checkbox" bind:checked={deleteFromDisk} />
        {prompt.checkboxLabel}
      </label>
      <div class="mt-4 flex justify-end gap-2">
        <button class="rounded-md border border-border px-3 py-1.5 text-xs font-semibold hover:bg-muted" onclick={() => deletePromptJobs = []}>Cancel</button>
        <button class="rounded-md bg-destructive px-3 py-1.5 text-xs font-semibold text-destructive-foreground" onclick={confirmDelete}>{prompt.confirmLabel}</button>
      </div>
    </section>
  </div>
{/if}

{#snippet InlineNameProgress(
  job: DownloadJob,
  presentation: ReturnType<typeof queueStatusPresentation>,
  artifactMissing: boolean,
  blurIdentity: boolean,
  size: QueueRowSize,
)}
  {@const showProgress = shouldShowNameProgress(job)}
  {@const progress = clampQueueProgress(job.progress)}
  {@const density = inlineNameDensity(size)}
  <div class={`relative -ml-2 min-w-0 flex-1 overflow-hidden rounded-sm ${density.container}`}>
    {#if showProgress}
      <div
        class={`pointer-events-none absolute ${density.progressInset} left-0 z-0 rounded-[inherit] opacity-25 ${nameProgressClass(presentation.tone)}`}
        style={`width: ${progress}%;`}
        aria-hidden="true"
      ></div>
    {/if}
    <div class={`relative z-10 flex min-w-0 items-center ${density.titleGap}`}>
      <div
        class={`truncate font-semibold text-foreground ${density.titleText} ${artifactMissing ? 'text-muted-foreground' : ''} ${blurIdentity ? 'opacity-70 blur-[0.7px]' : ''}`}
        title={job.filename}
      >
        {job.filename}
      </div>
      {@render QueueStatusBadge(presentation, size)}
    </div>
    <div class={`relative z-10 min-w-0 text-muted-foreground ${density.metaText} ${blurIdentity ? 'opacity-70 blur-[0.7px]' : ''}`}>
      {#if isBulkAggregateJob(job)}
        <div class="truncate">{job.bulkMemberIds.length} files</div>
      {:else if job.transferKind === 'torrent'}
        {@render TorrentDetailLine(job)}
      {:else if isBrowserAdoptedTransferKind(job.transferKind)}
        <div class="truncate" title="This file was completed by the browser and adopted into the queue.">Completed in browser</div>
      {:else}
        <div class="truncate" title={job.url}>{getHost(job.url)}</div>
      {/if}
    </div>
  </div>
{/snippet}

{#snippet TorrentDetailLine(job: DownloadJob)}
  {@const metrics = torrentDetailMetrics(job)}
  {@const title = torrentDetailTitle(metrics)}
  {#if metrics.length === 0}
    <div class="truncate" title={job.url}>{torrentActivitySummary(job)}</div>
  {:else}
    <div class="flex min-w-0 items-center gap-2 overflow-hidden" {title}>
      {#each metrics as metric (metric.kind)}
        <span class={`inline-flex shrink-0 items-center gap-1 text-[11px] font-medium leading-4 ${torrentMetricTextClass(metric.kind)}`}>
          {@render TorrentMetricIcon(metric.kind)}
          <span>{torrentMetricValue(metric)}</span>
        </span>
      {/each}
    </div>
  {/if}
{/snippet}

{#snippet TorrentMetricIcon(kind: ReturnType<typeof torrentDetailMetrics>[number]['kind'])}
  {@const Icon = kind === 'peers' ? Download : Upload}
  <Icon aria-hidden="true" size={12} strokeWidth={2.4} class={torrentMetricIconClass(kind)} />
{/snippet}

{#snippet QueueStatusBadge(presentation: ReturnType<typeof queueStatusPresentation>, size: QueueRowSize)}
  <span class={`shrink-0 rounded border font-semibold ${statusBadgeDensity(size)} ${statusBadgeClass(presentation.tone)}`}>
    {presentation.label}
  </span>
{/snippet}

{#snippet RowMenu(job: QueueDisplayJob)}
  {@const menuJobs = selectedJobsFor(job)}
  {@const removableJobs = menuJobs.filter((candidate) => !isBulkAggregateJob(candidate) && canRemoveDownloadImmediately(candidate))}
  {@const canRetry = canRetryJob(job)}
  {@const canCancel = ![JobState.Completed, JobState.Canceled, JobState.Failed].includes(job.state)}
  {#if isRemoving(job)}
    {@render MenuItem(Trash2, 'Removing files', () => undefined, true, true)}
  {:else if isCleanupFailed(job)}
    {@render MenuItem(Trash2, 'Delete from disk', () => openDeleteFromDiskPrompt(job), true)}
  {:else if isBulkAggregateJob(job)}
    {#if isCompletedBulkAggregate(job)}
      {@render MenuItem(FolderOpen, 'Show', () => onReveal(job.id))}
      {@render MenuItem(RotateCw, 'Retry', () => onRetryBulkMembers(job.id), false, job.bulkRetryableMemberCount <= 0)}
      {@render MenuItem(Trash2, 'Delete from disk', () => openDeleteFromDiskPrompt(job), true)}
    {:else if isFailedBulkAggregate(job)}
      {@render MenuItem(ExternalLink, 'Show Popup', () => onShowPopup(job.id))}
      {@render MenuItem(RotateCw, 'Retry', () => onRetryBulkMembers(job.id), false, job.bulkRetryableMemberCount <= 0)}
      {@render MenuItem(RotateCcw, 'Fix folder', () => onRetryArchive(job.id), false, !job.bulkArchiveFixable)}
      {@render MenuItem(Trash2, 'Delete', () => openDeletePrompt(job), true)}
      {@render MenuItem(Trash2, 'Delete from disk', () => openDeleteFromDiskPrompt(job), true)}
    {:else if isCanceledBulkAggregate(job)}
      {@render MenuItem(RotateCw, 'Retry', () => onRetryBulkMembers(job.id), false, job.bulkRetryableMemberCount <= 0)}
      {@render MenuItem(Trash2, 'Delete', () => openDeletePrompt(job), true)}
      {@render MenuItem(Trash2, 'Delete from disk', () => openDeleteFromDiskPrompt(job), true)}
    {:else}
      {@render MenuItem(ExternalLink, 'Show Popup', () => onShowPopup(job.id))}
      {@render MenuItem(RotateCw, 'Retry', () => onRetryBulkMembers(job.id), false, job.bulkRetryableMemberCount <= 0)}
      {#if canShowBulkPrimaryAction(job)}
        {@render MenuItem(Play, bulkPrimaryActionLabel(job), () => runBulkPrimaryAction(job), false, bulkPrimaryActionDisabled(job) || isActionPending(job))}
      {/if}
      {#if isActive(job)}{@render MenuItem(Pause, 'Pause', () => onPause(job.id), false, isActionPending(job))}{/if}
      {#if canCancel}{@render MenuItem(X, 'Cancel', () => onCancel(job.id), false, isActionPending(job))}{/if}
    {/if}
  {:else}
    {#if canOpenJobFile(job)}{@render MenuItem(FileText, 'Open File', () => onOpen(job.id))}{/if}
    {@render MenuItem(FolderOpen, 'Open Folder', () => onReveal(job.id))}
    {#if canShowProgressPopup(job)}{@render MenuItem(ExternalLink, 'Show Popup', () => onShowPopup(job.id))}{/if}
    {#if canRetry}{@render MenuItem(RotateCw, 'Retry', () => onRetry(job.id))}{/if}
    {#if canRetry}{@render MenuItem(RotateCcw, 'Restart', () => onRestart(job.id))}{/if}
    {#if job.state === JobState.Paused}{@render MenuItem(Play, 'Resume', () => onResume(job.id), false, isActionPending(job))}{/if}
    {#if isActive(job)}{@render MenuItem(Pause, 'Pause', () => onPause(job.id), false, isActionPending(job))}{/if}
    {#if canSwapFailedDownloadToBrowser(job)}{@render MenuItem(ExternalLink, 'Open in browser', () => onSwapFailedToBrowser(job.id))}{/if}
    {#if canCancel}{@render MenuItem(X, 'Cancel', () => onCancel(job.id), false, isActionPending(job))}{/if}
    {#if removableJobs.length > 0}
      {@render MenuItem(Pencil, 'Rename', () => openRename(job))}
      {@render MenuItem(Trash2, menuJobs.length === 1 ? deleteActionLabelForJob(job) : getDeleteContextMenuLabel(menuJobs.length), () => openDeletePrompt(job), true)}
    {/if}
  {/if}
{/snippet}

{#snippet MenuItem(icon: IconComponent, label: string, onClick: () => void, destructive = false, disabled = false)}
  {@const Icon = icon}
  <button
    class={`flex h-9 w-full items-center gap-2 px-3 text-left text-sm transition-colors ${disabled ? 'cursor-not-allowed opacity-45' : 'hover:bg-muted'} ${destructive ? 'text-destructive' : 'text-foreground'}`}
    {disabled}
    onclick={() => { onClick(); closeMenus(); }}
  >
    <span class={destructive ? 'text-destructive' : 'text-muted-foreground'}><Icon size={16} /></span>
    <span class="min-w-0 flex-1 truncate">{label}</span>
  </button>
{/snippet}

{#snippet DetailsHeaderMetrics(job: DownloadJob)}
  {@const metrics = progressMetricsByJobId[job.id]}
  {@const averageSpeed = metrics?.averageSpeed ?? job.speed}
  {@const timeRemaining = metrics?.timeRemaining ?? job.eta}
  <div class="mt-1 flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
    <span>Status <span class="ml-1 text-foreground">{queueStatusPresentation(job).label}</span></span>
    <span>Size <span class="ml-1 text-foreground">{formatQueueSize(job, formatBytes)}</span></span>
    <span>Speed <span class="ml-1 text-foreground">{formatQueueSpeed(job, averageSpeed)}</span></span>
    {#if showsEtaMetrics(job)}
      <span>ETA <span class="ml-1 text-foreground">{formatQueueTime(job, timeRemaining)}</span></span>
    {/if}
  </div>
{/snippet}

{#snippet DetailsCompactLine(job: DownloadJob)}
  {@const metrics = progressMetricsByJobId[job.id]}
  {@const averageSpeed = metrics?.averageSpeed ?? job.speed}
  {@const timeRemaining = metrics?.timeRemaining ?? job.eta}
  <div class="flex min-w-0 flex-wrap items-center gap-x-5 gap-y-1 text-xs">
    <span class="min-w-0 truncate text-muted-foreground" title={job.targetPath || 'No destination recorded yet.'}>
      Path <span class="ml-1 text-foreground">{job.targetPath || 'No destination recorded yet.'}</span>
    </span>
    <span class="text-muted-foreground">State <span class="ml-1 text-foreground">{queueStatusPresentation(job).label}</span></span>
    <span class="text-muted-foreground">Size <span class="ml-1 text-foreground">{formatQueueSize(job, formatBytes)}</span></span>
    <span class="text-muted-foreground">Speed <span class="ml-1 text-foreground">{formatQueueSpeed(job, averageSpeed)}</span></span>
    {#if showsEtaMetrics(job)}
      <span class="text-muted-foreground">ETA <span class="ml-1 text-foreground">{formatQueueTime(job, timeRemaining)}</span></span>
    {/if}
    {#if job.transferKind === 'torrent'}
      <span class="text-muted-foreground">Ratio <span class="ml-1 text-primary">{formatTorrentRatio(job)}</span></span>
    {/if}
  </div>
{/snippet}

{#snippet DetailsGrid(job: DownloadJob, level: DetailsLevel)}
  {@const metrics = progressMetricsByJobId[job.id]}
  {@const averageSpeed = metrics?.averageSpeed ?? job.speed}
  {@const timeRemaining = metrics?.timeRemaining ?? job.eta}
  {#if level === 'expanded'}
    <div class="grid min-h-0 gap-y-3 text-xs lg:grid-cols-[minmax(320px,1.35fr)_minmax(220px,0.85fr)_minmax(220px,0.8fr)] lg:divide-x lg:divide-border/35">
      <div class="min-w-0 lg:pr-5">
        {@render DetailSectionLabel('File')}
        {@render CompactDetailItem(HardDrive, 'Path', job.targetPath || 'No destination recorded yet.')}
        {@render CompactDetailItem(Globe, 'Source', job.url)}
        {@render CompactDetailItem(Clock3, 'Created', formatFullJobDate(job.createdAt))}
      </div>
      <div class="min-w-0 lg:px-5">
        {@render DetailSectionLabel('Transfer')}
        {@render CompactDetailItem(Clock3, 'State', queueStatusPresentation(job).label)}
        {@render CompactDetailItem(Download, 'Size', formatQueueSize(job, formatBytes))}
        {@render CompactDetailItem(Upload, 'Speed', formatQueueSpeed(job, averageSpeed))}
        {#if showsEtaMetrics(job)}
          {@render CompactDetailItem(Clock3, 'ETA', formatQueueTime(job, timeRemaining), job.transferKind === 'torrent')}
        {/if}
      </div>
      <div class="min-w-0 lg:pl-5">
        {@render DetailSectionLabel(job.transferKind === 'torrent' ? 'Torrent' : 'Network')}
        {@render CompactDetailItem(Users, 'Peers', job.torrent?.peers ? String(job.torrent.peers) : '--')}
        {@render CompactDetailItem(Upload, 'Ratio', job.transferKind === 'torrent' ? formatTorrentRatio(job) : '--', job.transferKind === 'torrent')}
        {#if job.transferKind === 'torrent'}
          {#each torrentDetailMetrics(job) as metric}
            {@render CompactDetailItem(metric.kind === 'peers' ? Users : Upload, metric.label, torrentMetricValue(metric), metric.kind !== 'peers')}
          {/each}
        {:else}
          {@render CompactDetailItem(Globe, 'Host', getHost(job.url))}
        {/if}
      </div>
    </div>
  {:else}
    <div class="grid grid-cols-[repeat(auto-fit,minmax(230px,1fr))] gap-x-6 gap-y-1 text-xs">
      {@render CompactDetailItem(HardDrive, 'Path', job.targetPath || 'No destination recorded yet.')}
      {@render CompactDetailItem(Globe, 'Source', job.url)}
      {@render CompactDetailItem(Clock3, 'State', queueStatusPresentation(job).label)}
      {@render CompactDetailItem(Download, 'Size', formatQueueSize(job, formatBytes))}
      {@render CompactDetailItem(Upload, 'Speed', formatQueueSpeed(job, averageSpeed))}
      {#if showsEtaMetrics(job)}
        {@render CompactDetailItem(Clock3, 'ETA', formatQueueTime(job, timeRemaining), job.transferKind === 'torrent')}
      {/if}
      {#if job.transferKind === 'torrent'}
        {@render CompactDetailItem(Users, 'Peers', job.torrent?.peers ? String(job.torrent.peers) : '--')}
        {@render CompactDetailItem(Upload, 'Ratio', formatTorrentRatio(job), true)}
      {/if}
    </div>
  {/if}
{/snippet}

{#snippet DetailSectionLabel(label: string)}
  <div class="mb-1 flex items-center gap-2 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
    <span>{label}</span>
    <span class="h-px flex-1 bg-border/35"></span>
  </div>
{/snippet}

{#snippet CompactDetailItem(icon: IconComponent, label: string, value: string, accent = false)}
  {@const Icon = icon}
  <div class="grid grid-cols-[minmax(84px,110px)_minmax(0,1fr)] items-baseline gap-3 border-t border-border/25 py-1 first:border-t-0">
    <div class="flex items-center gap-1.5 text-[11px] text-muted-foreground [&>svg]:h-3.5 [&>svg]:w-3.5">
      <Icon size={13} />
      <span class="truncate">{label}</span>
    </div>
    <div class={`truncate text-xs ${accent ? 'text-primary' : 'text-foreground'}`} title={value}>{value}</div>
  </div>
{/snippet}
