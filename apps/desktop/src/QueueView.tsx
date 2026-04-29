import React, { useEffect, useRef, useState } from 'react';
import {
  isJobArtifactMissing,
  selectJobRange,
  shouldBlurJobIdentity,
  shouldOpenJobFileOnDoubleClick,
} from './queueInteractions';
import { getDeleteContextMenuLabel, getDeletePromptContent } from './deletePrompts';
import type { DownloadProgressMetrics } from './downloadProgressMetrics';
import {
  canRemoveDownloadImmediately,
  canShowProgressPopup,
  canSwapFailedDownloadToBrowser,
  defaultDeleteFromDiskForJobs,
  deleteActionLabelForJob,
} from './queueCommands';
import {
  clampQueueProgress,
  fileBadgeActivityState,
  formatQueueSize,
  formatQueueSizeTitle,
  isTorrentCheckingFiles,
  isTorrentMetadataPending,
  isTorrentSeedingRestore,
  queueTableColumnsForView,
  queueStatusPresentation,
  shouldShowNameProgress,
  torrentActivitySummary,
  torrentDetailMetrics,
  torrentDisplayName,
  type FileBadgeActivityState,
  type TorrentDetailMetric,
  type TorrentDetailMetricKind,
  type QueueStatusTone,
} from './queueRowPresentation';
import { JobState } from './types';
import type { DownloadJob } from './types';
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
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
  ExternalLink,
  FolderOpen,
  Globe,
  GripHorizontal,
  HardDrive,
  LoaderCircle,
  MoreHorizontal,
  Magnet,
  Pause,
  Pencil,
  Play,
  RotateCcw,
  RotateCw,
  Trash2,
  Upload,
  Users,
  X,
} from 'lucide-react';
import { sortModeDirection, sortModeKey, type SortColumn, type SortMode } from './downloadSorting';
import type { QueueRowSize } from './types';

const DETAILS_MIN_HEIGHT = 104;
const DETAILS_CLOSE_THRESHOLD = 84;
const DETAILS_DEFAULT_HEIGHT = 128;
const DETAILS_EXPANDED_HEIGHT = 220;
const DETAILS_MAX_HEIGHT = 300;
const TABLE_MIN_HEIGHT = 180;
const COMPLETED_BADGE_DURATION_MS = 1200;

interface QueueViewProps {
  jobs: DownloadJob[];
  view: string;
  sortMode: SortMode;
  showDetailsOnClick: boolean;
  queueRowSize: QueueRowSize;
  onSortChange: (column: SortColumn) => void;
  progressMetricsByJobId: Record<string, DownloadProgressMetrics>;
  selectedJobId: string | null;
  onSelect: (id: string) => void;
  onClearSelection: () => void;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRestart: (id: string) => void;
  onDelete: (id: string, deleteFromDisk: boolean) => void;
  onDeleteMany: (ids: string[], deleteFromDisk: boolean) => void;
  onRename: (id: string, filename: string) => void;
  onOpen: (id: string) => void;
  onReveal: (id: string) => void;
  onShowPopup: (id: string) => void;
  onSwapFailedToBrowser: (id: string) => void;
}

export function QueueView({
  jobs,
  view,
  sortMode,
  showDetailsOnClick,
  queueRowSize,
  onSortChange,
  progressMetricsByJobId,
  selectedJobId,
  onSelect,
  onClearSelection,
  onPause,
  onResume,
  onCancel,
  onRetry,
  onRestart,
  onDelete,
  onDeleteMany,
  onRename,
  onOpen,
  onReveal,
  onShowPopup,
  onSwapFailedToBrowser,
}: QueueViewProps) {
  const selectedJob = selectedJobId ? jobs.find((job) => job.id === selectedJobId) ?? null : null;
  const [openMenuJobId, setOpenMenuJobId] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{ jobId: string; x: number; y: number } | null>(null);
  const [renamePromptJob, setRenamePromptJob] = useState<DownloadJob | null>(null);
  const [renameBaseName, setRenameBaseName] = useState('');
  const [renameExtension, setRenameExtension] = useState('');
  const [deletePromptJobs, setDeletePromptJobs] = useState<DownloadJob[]>([]);
  const [deleteFromDisk, setDeleteFromDisk] = useState(false);
  const [detailsHeight, setDetailsHeight] = useState(DETAILS_DEFAULT_HEIGHT);
  const [isResizingDetails, setIsResizingDetails] = useState(false);
  const [selectedJobIds, setSelectedJobIds] = useState<Set<string>>(() => new Set());
  const [isSelectingByDrag, setIsSelectingByDrag] = useState(false);
  const [recentlyCompletedJobIds, setRecentlyCompletedJobIds] = useState<Set<string>>(() => new Set());
  const queueRootRef = useRef<HTMLElement | null>(null);
  const resizeStart = useRef<{ y: number; height: number; containerHeight: number; proposedHeight: number } | null>(null);
  const selectedJobIdsRef = useRef(selectedJobIds);
  const visibleJobIdsRef = useRef<string[]>([]);
  const selectionDragRef = useRef<{ anchorId: string; selected: boolean; baseSelection: Set<string> } | null>(null);
  const previousJobStatesRef = useRef<Map<string, DownloadJob['state']>>(new Map());
  const completedBadgeTimersRef = useRef<Map<string, number>>(new Map());

  const contextMenuJob = contextMenu ? jobs.find((job) => job.id === contextMenu.jobId) ?? null : null;
  const contextMenuSelectionJobs = contextMenuJob
    ? selectedJobIds.has(contextMenuJob.id) && selectedJobIds.size > 1
      ? jobs.filter((job) => selectedJobIds.has(job.id))
      : [contextMenuJob]
    : [];
  const contextMenuRemovableJobs = contextMenuSelectionJobs.filter(canRemoveDownloadImmediately);
  const visibleJobIds = jobs.map((job) => job.id);
  const allVisibleSelected = jobs.length > 0 && jobs.every((job) => selectedJobIds.has(job.id));
  const hasVisibleSelection = jobs.some((job) => selectedJobIds.has(job.id));
  const tableColumns = queueTableColumnsForView(view);
  const isTorrentTable = tableColumns[2] === 'Seed';

  function selectSingleJob(jobId: string) {
    const next = new Set([jobId]);
    selectedJobIdsRef.current = next;
    setSelectedJobIds(next);
    onSelect(jobId);
  }

  function clearJobSelection() {
    const next = new Set<string>();
    selectedJobIdsRef.current = next;
    setSelectedJobIds(next);
    onClearSelection();
  }

  function toggleSingleJobSelection(jobId: string) {
    const selectedIds = selectedJobIdsRef.current;
    const isOnlySelectedJob = selectedJobId === jobId && selectedIds.size === 1 && selectedIds.has(jobId);
    if (isOnlySelectedJob) {
      clearJobSelection();
      return;
    }

    selectSingleJob(jobId);
  }

  function setJobSelection(jobId: string, selected: boolean) {
    const next = new Set(selectedJobIdsRef.current);
    if (selected) {
      next.add(jobId);
    } else {
      next.delete(jobId);
    }

    selectedJobIdsRef.current = next;
    setSelectedJobIds(next);
    if (next.size === 0) {
      onClearSelection();
      return;
    }

    if (selected || selectedJobId === jobId) {
      onSelect(selected ? jobId : Array.from(next)[0]);
    }
  }

  function setAllVisibleSelected(selected: boolean) {
    const next = selected ? new Set(visibleJobIds) : new Set<string>();
    selectedJobIdsRef.current = next;
    setSelectedJobIds(next);
    const firstSelected = Array.from(next)[0];
    if (firstSelected) {
      onSelect(firstSelected);
    } else {
      onClearSelection();
    }
  }

  function applySelectionRange(anchorId: string, currentId: string, selected: boolean, baseSelection = selectedJobIdsRef.current) {
    const range = selectJobRange(visibleJobIds.length > 0 ? visibleJobIds : visibleJobIdsRef.current, anchorId, currentId);
    if (range.length === 0) return;

    const next = new Set(baseSelection);
    for (const jobId of range) {
      if (selected) {
        next.add(jobId);
      } else {
        next.delete(jobId);
      }
    }

    selectedJobIdsRef.current = next;
    setSelectedJobIds(next);
    if (selected) {
      onSelect(currentId);
    } else if (next.size === 0) {
      onClearSelection();
    }
  }

  function startSelectionDrag(jobId: string, event: React.PointerEvent<HTMLInputElement>) {
    if (event.button !== 0) return;

    event.stopPropagation();
    const selected = !selectedJobIdsRef.current.has(jobId);
    selectionDragRef.current = { anchorId: jobId, selected, baseSelection: new Set(selectedJobIdsRef.current) };
    setIsSelectingByDrag(true);
  }

  function openDeletePromptForJobs(promptJobs: DownloadJob[]) {
    const removableJobs = promptJobs.filter(canRemoveDownloadImmediately);
    if (removableJobs.length === 0) return;
    setDeletePromptJobs(removableJobs);
    setDeleteFromDisk(defaultDeleteFromDiskForJobs(removableJobs));
  }

  function continueSelectionDrag(jobId: string) {
    const drag = selectionDragRef.current;
    if (!drag) return;
    applySelectionRange(drag.anchorId, jobId, drag.selected, drag.baseSelection);
  }

  useEffect(() => {
    selectedJobIdsRef.current = selectedJobIds;
  }, [selectedJobIds]);

  useEffect(() => {
    const previousStates = previousJobStatesRef.current;
    const nextStates = new Map<string, DownloadJob['state']>();
    const newlyCompletedIds: string[] = [];
    const currentJobIds = new Set<string>();
    const currentCompletedJobIds = new Set<string>();

    for (const job of jobs) {
      currentJobIds.add(job.id);
      nextStates.set(job.id, job.state);
      if (job.state === JobState.Completed) {
        currentCompletedJobIds.add(job.id);
      }
      const previousState = previousStates.get(job.id);
      if (previousState && previousState !== JobState.Completed && job.state === JobState.Completed) {
        newlyCompletedIds.push(job.id);
      }
    }

    previousJobStatesRef.current = nextStates;

    setRecentlyCompletedJobIds((current) => {
      const next = new Set([...current].filter((jobId) => currentCompletedJobIds.has(jobId)));
      for (const jobId of newlyCompletedIds) {
        next.add(jobId);
      }
      return setsEqual(current, next) ? current : next;
    });

    for (const [jobId, timer] of completedBadgeTimersRef.current) {
      if (!currentCompletedJobIds.has(jobId)) {
        window.clearTimeout(timer);
        completedBadgeTimersRef.current.delete(jobId);
      }
    }

    for (const jobId of newlyCompletedIds) {
      const existingTimer = completedBadgeTimersRef.current.get(jobId);
      if (existingTimer) {
        window.clearTimeout(existingTimer);
      }

      const timer = window.setTimeout(() => {
        completedBadgeTimersRef.current.delete(jobId);
        setRecentlyCompletedJobIds((current) => {
          if (!current.has(jobId)) return current;
          const next = new Set(current);
          next.delete(jobId);
          return next;
        });
      }, COMPLETED_BADGE_DURATION_MS);

      completedBadgeTimersRef.current.set(jobId, timer);
    }
  }, [jobs]);

  useEffect(() => () => {
    for (const timer of completedBadgeTimersRef.current.values()) {
      window.clearTimeout(timer);
    }
    completedBadgeTimersRef.current.clear();
  }, []);

  useEffect(() => {
    visibleJobIdsRef.current = visibleJobIds;

    const visibleIds = new Set(visibleJobIds);
    setSelectedJobIds((current) => {
      const next = new Set([...current].filter((jobId) => visibleIds.has(jobId)));
      if (selectedJobId && visibleIds.has(selectedJobId)) {
        next.add(selectedJobId);
      }
      return setsEqual(current, next) ? current : next;
    });
  }, [jobs, selectedJobId]);

  useEffect(() => {
    if (!isSelectingByDrag) return;

    const stopSelectionDrag = () => {
      selectionDragRef.current = null;
      setIsSelectingByDrag(false);
    };

    window.addEventListener('pointerup', stopSelectionDrag);
    window.addEventListener('pointercancel', stopSelectionDrag);
    return () => {
      window.removeEventListener('pointerup', stopSelectionDrag);
      window.removeEventListener('pointercancel', stopSelectionDrag);
    };
  }, [isSelectingByDrag]);

  function startDetailsResize(clientY: number) {
    if (resizeStart.current) return;

    const containerHeight = queueRootRef.current?.clientHeight ?? window.innerHeight;
    resizeStart.current = {
      y: clientY,
      height: detailsHeight,
      containerHeight,
      proposedHeight: detailsHeight,
    };
    setIsResizingDetails(true);
  }

  useEffect(() => {
    if (!openMenuJobId && !contextMenu) return;

    const closeMenu = () => {
      setOpenMenuJobId(null);
      setContextMenu(null);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeMenu();
    };

    document.addEventListener('click', closeMenu);
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      document.removeEventListener('click', closeMenu);
      document.removeEventListener('keydown', closeOnEscape);
    };
  }, [openMenuJobId, contextMenu]);

  useEffect(() => {
    const queueRoot = queueRootRef.current;
    if (!queueRoot) return;

    const resizeObserver = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const maxHeight = getDetailsMaxHeight(entry.contentRect.height);
      setDetailsHeight((height) => clamp(height, DETAILS_MIN_HEIGHT, maxHeight));
    });

    resizeObserver.observe(queueRoot);
    return () => resizeObserver.disconnect();
  }, []);

  useEffect(() => {
    if (!isResizingDetails) return;

    const resizeFromClientY = (clientY: number) => {
      const start = resizeStart.current;
      if (!start) return;
      const nextHeight = start.height + start.y - clientY;
      const maxHeight = getDetailsMaxHeight(start.containerHeight);
      start.proposedHeight = nextHeight;
      if (nextHeight < DETAILS_CLOSE_THRESHOLD) {
        resizeStart.current = null;
        setIsResizingDetails(false);
        onClearSelection();
        return;
      }

      setDetailsHeight(snapDetailsHeight(nextHeight, maxHeight));
    };

    const resizePointer = (event: PointerEvent) => resizeFromClientY(event.clientY);
    const resizeMouse = (event: MouseEvent) => resizeFromClientY(event.clientY);

    const stopResize = () => {
      const start = resizeStart.current;
      resizeStart.current = null;
      setIsResizingDetails(false);

      if (!start) return;
      if (start.proposedHeight < DETAILS_CLOSE_THRESHOLD) {
        onClearSelection();
        return;
      }

      const maxHeight = getDetailsMaxHeight(start.containerHeight);
      setDetailsHeight(snapDetailsHeight(start.proposedHeight, maxHeight));
    };

    window.addEventListener('pointermove', resizePointer);
    window.addEventListener('pointerup', stopResize);
    window.addEventListener('pointercancel', stopResize);
    window.addEventListener('mousemove', resizeMouse);
    window.addEventListener('mouseup', stopResize);
    return () => {
      window.removeEventListener('pointermove', resizePointer);
      window.removeEventListener('pointerup', stopResize);
      window.removeEventListener('pointercancel', stopResize);
      window.removeEventListener('mousemove', resizeMouse);
      window.removeEventListener('mouseup', stopResize);
    };
  }, [isResizingDetails, onClearSelection]);

  if (jobs.length === 0) {
    const emptyTitle = emptyStateTitle(view);

    return (
      <div className="flex min-h-0 flex-1 items-center justify-center bg-surface p-8">
        <div className="max-w-sm text-center">
          <div className="mx-auto mb-5 flex h-16 w-16 items-center justify-center rounded-md border border-border bg-card text-primary">
            <Download size={32} />
          </div>
          <h2 className="mb-2 text-lg font-semibold text-foreground">{emptyTitle}</h2>
          <p className="text-sm leading-6 text-muted-foreground">
            Downloads from the browser extension or the New Download command will appear in this list.
          </p>
        </div>
      </div>
    );
  }

  return (
    <section ref={queueRootRef} className="flex min-h-0 flex-1 flex-col bg-surface">
      <div className="min-h-0 flex-1 overflow-auto">
        <div className="download-table min-w-[980px] overflow-visible border-b border-t border-border bg-card">
          <div className="grid grid-cols-[minmax(420px,2.8fr)_150px_110px_100px_150px_72px] border-b border-border bg-header px-3 py-1.5 text-xs font-medium text-muted-foreground">
            <div className="flex items-center gap-3">
              <SelectionCheckbox
                checked={allVisibleSelected}
                indeterminate={!allVisibleSelected && hasVisibleSelection}
                title={allVisibleSelected ? 'Clear selection' : 'Select all downloads'}
                onChange={setAllVisibleSelected}
              />
              <SortableColumnHeader column="name" sortMode={sortMode} onSortChange={onSortChange}>
                {tableColumns[0]}
              </SortableColumnHeader>
            </div>
            <SortableColumnHeader column="date" sortMode={sortMode} onSortChange={onSortChange} align={isTorrentTable ? 'center' : 'left'} className={torrentColumnAlignClass(isTorrentTable)}>
              {tableColumns[1]}
            </SortableColumnHeader>
            <div title={isTorrentTable ? 'Seed upload speed' : undefined} className={torrentColumnAlignClass(isTorrentTable)}>{tableColumns[2]}</div>
            <div title={isTorrentTable ? 'Share ratio' : undefined}>{tableColumns[3]}</div>
            <SortableColumnHeader column="size" sortMode={sortMode} onSortChange={onSortChange}>
              {tableColumns[4]}
            </SortableColumnHeader>
            <div className="text-right">{tableColumns[5]}</div>
          </div>

          <div className="divide-y divide-border/70">
            {jobs.map((job) => {
              const selected = job.id === selectedJob?.id;
              const multiSelected = selectedJobIds.has(job.id);
              const rowSelected = selected || multiSelected;
              const artifactMissing = isJobArtifactMissing(job);
              const blurIdentity = shouldBlurJobIdentity(job);
              const progressMetrics = progressMetricsByJobId[job.id];
              const averageSpeed = progressMetrics?.averageSpeed ?? job.speed;
              const timeRemaining = progressMetrics?.timeRemaining ?? job.eta;
              const statusPresentation = queueStatusPresentation(job);
              return (
                <div
                  key={job.id}
                  onClick={() => {
                    toggleSingleJobSelection(job.id);
                    setOpenMenuJobId(null);
                    setContextMenu(null);
                  }}
                  onDoubleClick={(event) => {
                    if (!shouldOpenJobFileOnDoubleClick(job, event.button)) return;
                    event.preventDefault();
                    selectSingleJob(job.id);
                    setOpenMenuJobId(null);
                    setContextMenu(null);
                    onOpen(job.id);
                  }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    const isSelectedMultiContext = selectedJobIdsRef.current.size > 1 && selectedJobIdsRef.current.has(job.id);
                    if (!isSelectedMultiContext) {
                      selectSingleJob(job.id);
                    }
                    setOpenMenuJobId(null);
                    setContextMenu(getContextMenuPosition(job.id, event.clientX, event.clientY));
                  }}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault();
                      toggleSingleJobSelection(job.id);
                    }
                  }}
                  onPointerEnter={() => continueSelectionDrag(job.id)}
                  role="button"
                  tabIndex={0}
                  className={`grid w-full grid-cols-[minmax(420px,2.8fr)_150px_110px_100px_150px_72px] items-center gap-0 px-3 text-left transition ${queueRowSizeClass(queueRowSize)} ${
                    rowSelected ? 'bg-selected outline outline-1 outline-primary/30' : 'bg-card hover:bg-row-hover'
                  } ${artifactMissing ? 'opacity-45 grayscale' : ''}`}
                >
                  <div className="flex min-w-0 items-center gap-3 pr-4">
                    <FileBadge
                      filename={job.filename}
                      transferKind={job.transferKind}
                      selected={multiSelected}
                      selectionTitle={multiSelected ? `Deselect ${job.filename}` : `Select ${job.filename}`}
                      onSelectionChange={(checked) => setJobSelection(job.id, checked)}
                      onSelectionPointerDown={(event) => startSelectionDrag(job.id, event)}
                      muted={artifactMissing}
                      blurred={blurIdentity}
                      rowSize={queueRowSize}
                      activityState={fileBadgeActivityState(job, recentlyCompletedJobIds.has(job.id))}
                    />
                    <InlineNameProgress
                      job={job}
                      statusPresentation={statusPresentation}
                      artifactMissing={artifactMissing}
                      blurIdentity={blurIdentity}
                      rowSize={queueRowSize}
                    />
                  </div>

                  <div className={queueDateCellClass(isTorrentTable)} title={formatFullJobDate(job.createdAt)}>
                    {formatJobDate(job.createdAt)}
                  </div>

                  <div className={queueMetricCellClass(isTorrentTable)}>
                    {isTorrentTable ? formatTorrentSeedMetric(job) : formatQueueSpeed(job, averageSpeed)}
                  </div>
                  <div className="tabular-nums text-muted-foreground">
                    {isTorrentTable ? formatTorrentRatio(job) : formatQueueTime(job, timeRemaining)}
                  </div>
                  <div className="tabular-nums text-muted-foreground" title={formatQueueSizeTitle(job, formatBytes)}>
                    {formatQueueSize(job, formatBytes)}
                  </div>

                  <div
                    className="relative flex items-center justify-end gap-1"
                    onClick={(event) => event.stopPropagation()}
                    onDoubleClick={(event) => event.stopPropagation()}
                  >
                    <RowActions
                      job={job}
                      menuOpen={openMenuJobId === job.id}
                      onToggleMenu={() => setOpenMenuJobId((current) => current === job.id ? null : job.id)}
                      onCloseMenu={() => setOpenMenuJobId(null)}
                      onPause={onPause}
                      onResume={onResume}
                      onCancel={onCancel}
                      onRetry={onRetry}
                      onRestart={onRestart}
                      onRequestDelete={() => openDeletePromptForJobs([job])}
                      onReveal={onReveal}
                      onShowPopup={onShowPopup}
                      onSwapFailedToBrowser={onSwapFailedToBrowser}
                    />
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {contextMenu && contextMenuJob ? (
        <FileContextMenu
          job={contextMenuJob}
          deleteCount={contextMenuRemovableJobs.length}
          x={contextMenu.x}
          y={contextMenu.y}
          onOpen={(id) => {
            setContextMenu(null);
            onOpen(id);
          }}
          onReveal={(id) => {
            setContextMenu(null);
            onReveal(id);
          }}
          onShowPopup={(id) => {
            setContextMenu(null);
            onShowPopup(id);
          }}
          onRename={(job) => {
            setContextMenu(null);
            setRenamePromptJob(job);
            const parsedName = splitFilename(job.filename);
            setRenameBaseName(parsedName.baseName);
            setRenameExtension(parsedName.extension);
          }}
          onDelete={() => {
            setContextMenu(null);
            openDeletePromptForJobs(contextMenuSelectionJobs);
          }}
        />
      ) : null}

      {selectedJob && showDetailsOnClick ? (
        <DownloadDetailsPane
          job={selectedJob}
          onPause={onPause}
          onResume={onResume}
          onCancel={onCancel}
          onRetry={onRetry}
          onRestart={onRestart}
          onOpen={onOpen}
          onReveal={onReveal}
          onClose={onClearSelection}
          height={detailsHeight}
          onResizeStart={(event) => {
            event.preventDefault();
            startDetailsResize(event.clientY);
            event.currentTarget.setPointerCapture(event.pointerId);
          }}
          onMouseResizeStart={(event) => {
            event.preventDefault();
            startDetailsResize(event.clientY);
          }}
          onResizeEnd={(event) => {
            if (event.currentTarget.hasPointerCapture(event.pointerId)) {
              event.currentTarget.releasePointerCapture(event.pointerId);
            }
          }}
        />
      ) : null}

      {renamePromptJob ? (
        <RenamePrompt
          job={renamePromptJob}
          baseName={renameBaseName}
          extension={renameExtension}
          onBaseNameChange={setRenameBaseName}
          onExtensionChange={setRenameExtension}
          onCancel={() => setRenamePromptJob(null)}
          onRename={() => {
            const filename = buildFilename(renameBaseName, renameExtension);
            if (!filename) return;
            onRename(renamePromptJob.id, filename);
            setRenamePromptJob(null);
          }}
        />
      ) : null}

      {deletePromptJobs.length > 0 ? (
        <DeletePrompt
          jobs={deletePromptJobs}
          deleteFromDisk={deleteFromDisk}
          onDeleteFromDiskChange={setDeleteFromDisk}
          onCancel={() => setDeletePromptJobs([])}
          onDelete={() => {
            const ids = deletePromptJobs.map((job) => job.id);
            if (ids.length === 1) {
              onDelete(ids[0], deleteFromDisk);
            } else {
              onDeleteMany(ids, deleteFromDisk);
              selectedJobIdsRef.current = new Set();
              setSelectedJobIds(new Set());
              onClearSelection();
            }
            setDeletePromptJobs([]);
          }}
        />
      ) : null}
    </section>
  );
}

function FileContextMenu({
  job,
  deleteCount,
  x,
  y,
  onOpen,
  onReveal,
  onShowPopup,
  onRename,
  onDelete,
}: {
  job: DownloadJob;
  deleteCount: number;
  x: number;
  y: number;
  onOpen: (id: string) => void;
  onReveal: (id: string) => void;
  onShowPopup: (id: string) => void;
  onRename: (job: DownloadJob) => void;
  onDelete: () => void;
}) {
  const canDelete = canRemoveDownloadImmediately(job);
  const deleteLabel = deleteCount === 1
    ? deleteActionLabelForJob(job)
    : getDeleteContextMenuLabel(deleteCount);

  return (
    <div
      className="fixed z-[70] w-48 overflow-hidden rounded-md border border-border bg-card py-1 shadow-2xl"
      style={{ left: x, top: y }}
      onClick={(event) => event.stopPropagation()}
      role="menu"
      aria-label={`${job.filename} actions`}
    >
      <MenuItem icon={<FileText size={16} />} label="Open File" onClick={() => onOpen(job.id)} />
      <MenuItem icon={<FolderOpen size={16} />} label="Open Folder" onClick={() => onReveal(job.id)} />
      {canShowProgressPopup(job) ? (
        <MenuItem icon={<ExternalLink size={16} />} label="Show Popup" onClick={() => onShowPopup(job.id)} />
      ) : null}
      {canDelete ? (
        <>
          <MenuItem icon={<Pencil size={16} />} label="Rename" onClick={() => onRename(job)} />
          <MenuItem icon={<Trash2 size={16} />} label={deleteLabel} onClick={onDelete} destructive />
        </>
      ) : null}
    </div>
  );
}

function RenamePrompt({
  job,
  baseName,
  extension,
  onBaseNameChange,
  onExtensionChange,
  onCancel,
  onRename,
}: {
  job: DownloadJob;
  baseName: string;
  extension: string;
  onBaseNameChange: (value: string) => void;
  onExtensionChange: (value: string) => void;
  onCancel: () => void;
  onRename: () => void;
}) {
  const previewFilename = buildFilename(baseName, extension);
  const canRename = previewFilename.length > 0;

  return (
    <div className="fixed inset-0 z-[80] flex items-center justify-center bg-black/45 p-4">
      <form
        className="w-full max-w-md rounded-md border border-border bg-card shadow-2xl"
        onSubmit={(event) => {
          event.preventDefault();
          if (canRename) onRename();
        }}
      >
        <div className="border-b border-border px-5 py-4">
          <h2 className="text-base font-semibold text-foreground">Rename Download</h2>
          <p className="mt-1 truncate text-sm text-muted-foreground" title={job.filename}>
            {job.filename}
          </p>
        </div>
        <div className="px-5 py-4">
          <div className="grid grid-cols-[minmax(0,1fr)_112px] gap-3">
            <div className="min-w-0">
              <label className="mb-2 block text-sm font-medium text-foreground" htmlFor="rename-download-name">
                Name
              </label>
              <input
                id="rename-download-name"
                value={baseName}
                onChange={(event) => onBaseNameChange(event.target.value)}
                autoFocus
                className="h-9 w-full rounded-md border border-border bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary"
              />
            </div>
            <div>
              <label className="mb-2 block text-sm font-medium text-foreground" htmlFor="rename-download-extension">
                Extension
              </label>
              <input
                id="rename-download-extension"
                value={extension}
                onChange={(event) => onExtensionChange(normalizeExtensionInput(event.target.value))}
                placeholder="zip"
                className="h-9 w-full rounded-md border border-border bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary"
              />
            </div>
          </div>
          <p className="mt-3 truncate text-xs text-muted-foreground" title={previewFilename || 'Enter a file name.'}>
            Result: {previewFilename || 'Enter a file name.'}
          </p>
        </div>
        <div className="flex justify-end gap-2 border-t border-border px-5 py-4">
          <button type="button" onClick={onCancel} className="h-9 rounded-md px-4 text-sm font-semibold text-foreground hover:bg-muted">
            Cancel
          </button>
          <button
            type="submit"
            disabled={!canRename}
            className="h-9 rounded-md bg-primary px-4 text-sm font-semibold text-primary-foreground transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            Rename
          </button>
        </div>
      </form>
    </div>
  );
}

function DeletePrompt({
  jobs,
  deleteFromDisk,
  onDeleteFromDiskChange,
  onCancel,
  onDelete,
}: {
  jobs: DownloadJob[];
  deleteFromDisk: boolean;
  onDeleteFromDiskChange: (value: boolean) => void;
  onCancel: () => void;
  onDelete: () => void;
}) {
  const content = getDeletePromptContent(jobs.length);
  const primaryJob = jobs[0];
  const multiDelete = jobs.length > 1;

  return (
    <div className="fixed inset-0 z-[80] flex items-center justify-center bg-black/45 p-4">
      <div className="w-full max-w-md rounded-md border border-border bg-card shadow-2xl">
        <div className="border-b border-border px-5 py-4">
          <h2 className="text-base font-semibold text-foreground">{content.title}</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            {content.description}
          </p>
        </div>
        <div className="space-y-4 px-5 py-4">
          {multiDelete ? (
            <>
              <div className="text-sm font-medium text-foreground">{content.selectedSummary}</div>
              <div className="max-h-40 overflow-y-auto rounded-md border border-border bg-background/60">
                {jobs.map((job) => (
                  <div key={job.id} className="border-b border-border/60 px-3 py-2 last:border-b-0">
                    <div className="truncate text-sm font-medium text-foreground" title={job.filename}>
                      {job.filename}
                    </div>
                    <div className="mt-0.5 break-all text-xs text-muted-foreground">
                      {job.targetPath || content.missingPathLabel}
                    </div>
                  </div>
                ))}
              </div>
            </>
          ) : (
            <div className="truncate text-sm font-medium text-foreground" title={primaryJob.filename}>
              {primaryJob.filename}
            </div>
          )}
          <label className="flex cursor-pointer items-start gap-3 py-1 text-sm">
            <input
              type="checkbox"
              checked={deleteFromDisk}
              onChange={(event) => onDeleteFromDiskChange(event.target.checked)}
              className="mt-0.5 h-4 w-4 accent-primary"
            />
            <span>
              <span className="block font-medium text-foreground">{content.checkboxLabel}</span>
              {multiDelete ? (
                <span className="mt-1 block text-muted-foreground">
                  Applies to recorded target paths for the selected downloads.
                </span>
              ) : (
                <span className="mt-1 block break-all text-muted-foreground">
                  {primaryJob.targetPath || content.missingPathLabel}
                </span>
              )}
            </span>
          </label>
        </div>
        <div className="flex justify-end gap-2 border-t border-border px-5 py-4">
          <button type="button" onClick={onCancel} className="h-9 rounded-md px-4 text-sm font-semibold text-foreground hover:bg-muted">
            Cancel
          </button>
          <button
            type="button"
            onClick={onDelete}
            className="h-9 rounded-md bg-destructive px-4 text-sm font-semibold text-destructive-foreground transition hover:opacity-90"
          >
            {content.confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

function SortableColumnHeader({
  column,
  sortMode,
  onSortChange,
  align = 'left',
  className = '',
  children,
}: {
  column: SortColumn;
  sortMode: SortMode;
  onSortChange: (column: SortColumn) => void;
  align?: 'left' | 'center' | 'right';
  className?: string;
  children: React.ReactNode;
}) {
  const active = sortModeKey(sortMode) === column;
  const direction = sortModeDirection(sortMode);
  const Icon = active ? direction === 'asc' ? ArrowUp : ArrowDown : ArrowUpDown;
  const alignmentClass = align === 'right'
    ? 'justify-end'
    : align === 'center'
      ? 'justify-center'
      : 'justify-start';

  return (
    <button
      type="button"
      onClick={() => onSortChange(column)}
      className={`inline-flex min-w-0 items-center gap-1.5 rounded-sm text-xs font-semibold transition hover:text-foreground focus:outline-none focus:ring-2 focus:ring-primary/25 ${
        active ? 'text-primary' : 'text-muted-foreground'
      } ${alignmentClass} ${className}`}
      aria-label={`Sort by ${String(children)} ${active && direction === 'asc' ? 'descending' : 'ascending'}`}
    >
      <span className="truncate">{children}</span>
      <Icon size={12} strokeWidth={2.4} aria-hidden="true" />
    </button>
  );
}

function splitFilename(filename: string) {
  const dotIndex = filename.lastIndexOf('.');
  if (dotIndex <= 0 || dotIndex === filename.length - 1) {
    return { baseName: filename, extension: '' };
  }

  return {
    baseName: filename.slice(0, dotIndex),
    extension: filename.slice(dotIndex + 1),
  };
}

function normalizeExtensionInput(value: string) {
  return value.replace(/^\.+/, '').replace(/[<>:"/\\|?*\u0000-\u001F\s]/g, '').trim();
}

function buildFilename(baseName: string, extension: string) {
  const name = baseName.trim();
  const normalizedExtension = normalizeExtensionInput(extension);
  if (!name) return '';
  return normalizedExtension ? `${name}.${normalizedExtension}` : name;
}

function DownloadDetailsPane({
  job,
  onPause,
  onResume,
  onCancel,
  onRetry,
  onRestart,
  onOpen,
  onReveal,
  onClose,
  height,
  onResizeStart,
  onMouseResizeStart,
  onResizeEnd,
}: {
  job: DownloadJob;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRestart: (id: string) => void;
  onOpen: (id: string) => void;
  onReveal: (id: string) => void;
  onClose: () => void;
  height: number;
  onResizeStart: (event: React.PointerEvent<HTMLDivElement>) => void;
  onMouseResizeStart: (event: React.MouseEvent<HTMLDivElement>) => void;
  onResizeEnd: (event: React.PointerEvent<HTMLDivElement>) => void;
}) {
  const sourceLabel = job.source
    ? `${job.source.browser} ${job.source.entryPoint.replaceAll('_', ' ')}`
    : 'Manual URL';
  const compact = height <= DETAILS_DEFAULT_HEIGHT + 8;
  const detailItems = [
    { icon: <Globe size={16} />, label: 'Source URL:', value: job.url, accent: true },
    { icon: <FolderOpen size={16} />, label: 'Destination:', value: job.targetPath || 'No destination recorded yet.' },
    { icon: <HardDrive size={16} />, label: 'File Size:', value: job.totalBytes > 0 ? `${formatBytes(job.totalBytes)} (${job.totalBytes.toLocaleString()} bytes)` : 'Unknown' },
    { icon: <Download size={16} />, label: 'Downloaded:', value: `${formatBytes(job.downloadedBytes)} (${job.downloadedBytes.toLocaleString()} bytes)` },
    { icon: <Clock3 size={16} />, label: 'Remaining', value: job.state === JobState.Downloading ? formatTime(job.eta) : '--' },
    { icon: <Check size={16} />, label: 'Status:', value: statusText(job) },
    { icon: <RotateCw size={16} />, label: 'Resume:', value: formatResumeSupport(job.resumeSupport) },
    ...(job.transferKind === 'torrent'
      ? [
          { icon: <Magnet size={16} />, label: 'Torrent:', value: torrentDisplayName(job) },
          { icon: <Upload size={16} />, label: 'Uploaded:', value: `${formatBytes(job.torrent?.uploadedBytes ?? 0)} (${formatTorrentRatio(job)})` },
          { icon: <Users size={16} />, label: 'Peers:', value: formatTorrentPeers(job) },
        ]
      : []),
    ...(typeof job.retryAttempts === 'number' && job.retryAttempts > 0
      ? [{ icon: <RotateCw size={16} />, label: 'Retries:', value: `${job.retryAttempts} automatic ${job.retryAttempts === 1 ? 'retry' : 'retries'}` }]
      : []),
    ...(job.failureCategory
      ? [{ icon: <X size={16} />, label: 'Failure:', value: formatFailureCategory(job.failureCategory) }]
      : []),
    { icon: <Globe size={16} />, label: 'Source:', value: sourceLabel },
  ];

  return (
    <aside
      className={`details-pane relative shrink-0 border-t border-border bg-surface ${compact ? 'overflow-hidden' : 'overflow-auto'}`}
      style={{ height }}
    >
      <div
        className="absolute left-0 right-0 top-0 z-10 flex h-3 cursor-ns-resize items-center justify-center bg-surface/95 text-muted-foreground transition hover:text-foreground"
        onPointerDown={onResizeStart}
        onMouseDown={onMouseResizeStart}
        onPointerUp={onResizeEnd}
        onPointerCancel={onResizeEnd}
        title="Resize details"
        aria-label="Resize details"
      >
        <GripHorizontal size={16} />
      </div>

      <button
        onClick={onClose}
        className="absolute right-2 top-3 z-20 flex h-6 w-6 items-center justify-center rounded-md text-muted-foreground transition hover:bg-muted hover:text-foreground"
        title="Hide details"
        aria-label="Hide details"
      >
        <X size={14} />
      </button>

      <div className={`${compact ? 'flex h-full min-w-0 items-center gap-3 px-4 py-3 pt-4' : 'flex min-w-0 gap-4 px-5 py-4 pt-5'}`}>
        <div className={`${compact ? 'flex w-14 shrink-0 justify-center' : 'flex w-[72px] shrink-0 justify-center'}`}>
          <FileBadge filename={job.filename} transferKind={job.transferKind} large />
        </div>

        <div className="min-w-0 flex-1 pr-8">
          <h3 className={`${compact ? 'mb-1.5 text-sm' : 'mb-2.5 text-base'} truncate font-semibold text-foreground`} title={job.filename}>
            {job.filename}
          </h3>
          {compact ? (
            <div className="overflow-x-auto overflow-y-hidden pb-1">
              <div className="grid min-w-[1080px] grid-flow-col auto-cols-[minmax(260px,1fr)] grid-rows-2 gap-x-3 gap-y-2 text-xs">
                {detailItems.map((item) => (
                  <CompactDetailItem key={item.label} icon={item.icon} label={item.label} value={item.value} accent={item.accent} />
                ))}
              </div>
            </div>
          ) : (
            <div className="grid max-w-[820px] grid-cols-[108px_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
              {detailItems.map((item) => (
                <React.Fragment key={item.label}>
                  <DetailLabel icon={item.icon} label={item.label} />
                  <DetailValue value={item.value} accent={item.accent} />
                </React.Fragment>
              ))}
            </div>
          )}
        </div>
      </div>
    </aside>
  );
}

function InlineNameProgress({
  job,
  statusPresentation,
  artifactMissing,
  blurIdentity,
  rowSize,
}: {
  job: DownloadJob;
  statusPresentation: ReturnType<typeof queueStatusPresentation>;
  artifactMissing: boolean;
  blurIdentity: boolean;
  rowSize: QueueRowSize;
}) {
  const showProgress = shouldShowNameProgress(job);
  const progress = clampQueueProgress(job.progress);
  const density = inlineNameDensity(rowSize);

  return (
    <div className={`relative -ml-2 min-w-0 flex-1 overflow-hidden rounded-sm ${density.container}`}>
      {showProgress ? (
        <div
          className={`pointer-events-none absolute ${density.progressInset} left-0 z-0 rounded-[inherit] blur-md ${nameProgressClass(statusPresentation.tone)}`}
          style={{ width: `${progress}%` }}
          aria-hidden="true"
        />
      ) : null}
      <div className={`relative z-10 flex min-w-0 items-center ${density.titleGap}`}>
        <div
          className={`truncate font-semibold text-foreground ${density.titleText} ${artifactMissing ? 'text-muted-foreground' : ''} ${
            blurIdentity ? 'opacity-70 blur-[0.7px]' : ''
          }`}
          title={job.filename}
        >
          {job.filename}
        </div>
        <QueueStatusBadge presentation={statusPresentation} rowSize={rowSize} />
      </div>
      <div className={`relative z-10 min-w-0 text-muted-foreground ${density.metaText} ${blurIdentity ? 'opacity-70 blur-[0.7px]' : ''}`}>
        {job.transferKind === 'torrent' ? (
          <TorrentDetailLine job={job} />
        ) : (
          <div className="truncate" title={job.url}>
            {getHost(job.url)}
          </div>
        )}
      </div>
    </div>
  );
}

function TorrentDetailLine({ job }: { job: DownloadJob }) {
  const metrics = torrentDetailMetrics(job);
  const title = torrentDetailTitle(metrics);

  if (!metrics.length) {
    return (
      <div className="truncate" title={job.url}>
        {torrentActivitySummary(job)}
      </div>
    );
  }

  return (
    <div className="flex min-w-0 items-center gap-2 overflow-hidden" title={title}>
      {metrics.map((metric) => (
        <span
          key={metric.kind}
          className={`inline-flex shrink-0 items-center gap-1 text-[11px] font-medium leading-4 ${torrentMetricTextClass(metric.kind)}`}
        >
          <TorrentMetricIcon kind={metric.kind} />
          <span>{torrentMetricValue(metric)}</span>
        </span>
      ))}
    </div>
  );
}

function TorrentMetricIcon({ kind }: { kind: TorrentDetailMetricKind }) {
  const Icon = kind === 'peers' ? Download : Upload;

  return <Icon aria-hidden="true" size={12} strokeWidth={2.4} className={torrentMetricIconClass(kind)} />;
}

function QueueStatusBadge({
  presentation,
  rowSize,
}: {
  presentation: ReturnType<typeof queueStatusPresentation>;
  rowSize: QueueRowSize;
}) {
  const density = statusBadgeDensity(rowSize);

  return (
    <span className={`shrink-0 rounded border font-semibold ${density} ${statusBadgeClass(presentation.tone)}`}>
      {presentation.label}
    </span>
  );
}

function RowActions({
  job,
  menuOpen,
  onToggleMenu,
  onCloseMenu,
  onPause,
  onResume,
  onCancel,
  onRetry,
  onRestart,
  onRequestDelete,
  onReveal,
  onShowPopup,
  onSwapFailedToBrowser,
}: {
  job: DownloadJob;
  menuOpen: boolean;
  onToggleMenu: () => void;
  onCloseMenu: () => void;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRestart: (id: string) => void;
  onRequestDelete: () => void;
  onReveal: (id: string) => void;
  onShowPopup: (id: string) => void;
  onSwapFailedToBrowser: (id: string) => void;
}) {
  const canPause = [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding].includes(job.state);
  const canResume = job.state === JobState.Paused;
  const canRetry = [JobState.Failed, JobState.Canceled].includes(job.state);
  const canCancel = ![JobState.Completed, JobState.Canceled, JobState.Failed].includes(job.state);
  const canRemove = canRemoveDownloadImmediately(job);

  const runMenuAction = (action: (id: string) => void) => {
    onCloseMenu();
    action(job.id);
  };

  return (
    <>
      {canPause ? (
        <IconButton title="Pause" onClick={() => onPause(job.id)}><Pause size={17} /></IconButton>
      ) : null}
      {canResume ? (
        <IconButton title="Resume" onClick={() => onResume(job.id)}><Play size={17} /></IconButton>
      ) : null}
      {canRetry ? (
        <IconButton title="Retry" onClick={() => onRetry(job.id)}><RotateCw size={17} /></IconButton>
      ) : null}
      {canSwapFailedDownloadToBrowser(job) ? (
        <IconButton title="Swap" onClick={() => onSwapFailedToBrowser(job.id)}><ExternalLink size={17} /></IconButton>
      ) : null}
      <IconButton title="More actions" onClick={onToggleMenu}><MoreHorizontal size={18} /></IconButton>

      {menuOpen ? (
        <div
          className="absolute right-0 top-9 z-50 w-44 overflow-hidden rounded-md border border-border bg-card py-1 shadow-2xl"
          onClick={(event) => event.stopPropagation()}
        >
          {canShowProgressPopup(job) ? (
            <MenuItem icon={<ExternalLink size={16} />} label="Show Popup" onClick={() => runMenuAction(onShowPopup)} />
          ) : null}
          {job.targetPath ? (
            <MenuItem icon={<FolderOpen size={16} />} label="Show in folder" onClick={() => runMenuAction(onReveal)} />
          ) : null}
          {canRetry ? (
            <MenuItem icon={<RotateCcw size={16} />} label="Restart" onClick={() => runMenuAction(onRestart)} />
          ) : null}
          {canSwapFailedDownloadToBrowser(job) ? (
            <MenuItem icon={<ExternalLink size={16} />} label="Swap" onClick={() => runMenuAction(onSwapFailedToBrowser)} />
          ) : null}
          {canCancel ? (
            <MenuItem icon={<X size={16} />} label="Cancel" onClick={() => runMenuAction(onCancel)} />
          ) : null}
          {canRemove ? (
            <MenuItem
              icon={<Trash2 size={16} />}
              label={deleteActionLabelForJob(job)}
              onClick={() => {
                onCloseMenu();
                onRequestDelete();
              }}
              destructive
            />
          ) : null}
        </div>
      ) : null}
    </>
  );
}

function FileBadge({
  filename,
  transferKind = 'http',
  large = false,
  rowSize = 'medium',
  selected = false,
  selectionTitle,
  onSelectionChange,
  onSelectionPointerDown,
  muted = false,
  blurred = false,
  activityState = 'none',
}: {
  filename: string;
  transferKind?: DownloadJob['transferKind'];
  large?: boolean;
  rowSize?: QueueRowSize;
  selected?: boolean;
  selectionTitle?: string;
  onSelectionChange?: (checked: boolean) => void;
  onSelectionPointerDown?: (event: React.PointerEvent<HTMLInputElement>) => void;
  muted?: boolean;
  blurred?: boolean;
  activityState?: FileBadgeActivityState;
}) {
  const ext = filename.split('.').pop()?.toLowerCase() || '';
  const density = fileBadgeDensity(rowSize);
  const iconSize = large ? 28 : density.iconSize;
  const icon = transferKind === 'torrent' ? <Magnet size={iconSize} /> : getFileIcon(ext, iconSize);
  const label = transferKind === 'torrent' ? 'P2P' : ext ? ext.slice(0, 4).toUpperCase() : 'FILE';
  const selectable = !large && onSelectionChange;

  return (
    <div className={`file-badge relative flex shrink-0 items-center justify-center rounded-sm border border-border bg-background ${large ? 'h-[76px] w-14' : density.className}`}>
      <div className={`absolute right-0 top-0 h-2 w-2 border-b border-l border-border bg-surface ${selectable ? 'opacity-0' : ''}`} />
      {selectable ? (
        <SelectionCheckbox
          checked={selected}
          title={selectionTitle ?? 'Select download'}
          onChange={onSelectionChange}
          onPointerDown={onSelectionPointerDown}
          className="absolute -right-1 -top-1 z-20 h-3.5 w-3.5 rounded-[2px]"
        />
      ) : null}
      <div className={`${muted ? 'text-muted-foreground' : 'text-primary'} ${blurred ? 'opacity-70 blur-[0.7px]' : ''}`}>{icon}</div>
      {activityState !== 'none' ? (
        <div
          className={`pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-sm ${
            activityState === 'completed'
              ? 'animate-[queue-complete-check_1.2s_ease-out_forwards] bg-success/15 text-success'
              : 'bg-background/45 text-primary'
          }`}
          aria-hidden="true"
        >
          {activityState === 'buffering' ? (
            <LoaderCircle size={large ? 20 : 14} strokeWidth={2.4} className="animate-spin" />
          ) : (
            <Check size={large ? 20 : 14} strokeWidth={2.6} />
          )}
        </div>
      ) : null}
      {large ? <div className="absolute bottom-1.5 text-[10px] font-semibold text-muted-foreground">{label}</div> : null}
    </div>
  );
}

function SelectionCheckbox({
  checked,
  indeterminate = false,
  title,
  onChange,
  onPointerDown,
  className = '',
}: {
  checked: boolean;
  indeterminate?: boolean;
  title: string;
  onChange: (checked: boolean) => void;
  onPointerDown?: (event: React.PointerEvent<HTMLInputElement>) => void;
  className?: string;
}) {
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (inputRef.current) {
      inputRef.current.indeterminate = indeterminate;
    }
  }, [indeterminate]);

  return (
    <input
      ref={inputRef}
      type="checkbox"
      checked={checked}
      title={title}
      aria-label={title}
      onClick={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
      onPointerDown={(event) => {
        if (onPointerDown) {
          onPointerDown(event);
          return;
        }
        event.stopPropagation();
      }}
      onChange={(event) => {
        event.stopPropagation();
        onChange(event.currentTarget.checked);
      }}
      className={`shrink-0 cursor-pointer accent-primary ${className || 'h-3.5 w-3.5'}`}
    />
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

function MenuItem({
  icon,
  label,
  onClick,
  destructive = false,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  destructive?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex h-9 w-full items-center gap-2 px-3 text-left text-sm transition hover:bg-muted ${
        destructive ? 'text-destructive' : 'text-foreground'
      }`}
    >
      <span className={destructive ? 'text-destructive' : 'text-muted-foreground'}>{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
    </button>
  );
}

function emptyStateTitle(view: string) {
  switch (view) {
    case 'attention':
      return 'No downloads need attention';
    case 'active':
      return 'No active downloads';
    case 'queued':
      return 'No queued downloads';
    case 'completed':
      return 'No completed downloads';
    case 'torrents':
      return 'No torrents';
    case 'torrent-active':
      return 'No active torrents';
    case 'torrent-seeding':
      return 'No seeding torrents';
    case 'torrent-attention':
      return 'No torrents need attention';
    case 'torrent-queued':
      return 'No queued torrents';
    case 'torrent-completed':
      return 'No completed torrents';
    default:
      return 'No downloads';
  }
}

function statusText(job: DownloadJob) {
  if (isTorrentSeedingRestore(job)) return 'Restoring seeding';
  if (isTorrentCheckingFiles(job)) return 'Checking files';
  if (isTorrentMetadataPending(job)) return 'Finding metadata';

  if (job.state === JobState.Failed && job.failureCategory) {
    return `${formatFailureCategory(job.failureCategory)} Error`;
  }

  switch (job.state) {
    case JobState.Seeding:
      return 'Seeding';
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
      return job.state;
  }
}

function formatQueueSpeed(job: DownloadJob, averageSpeed: number) {
  if (job.state === JobState.Downloading) return `${formatBytes(averageSpeed)}/s`;
  if (job.state === JobState.Seeding && job.torrent) return `Up ${formatBytes(job.torrent.uploadedBytes)}`;
  return '--';
}

function formatTorrentSeedMetric(job: DownloadJob) {
  if (!job.torrent) return '--';
  if (job.torrent.uploadedBytes > 0) return formatBytes(job.torrent.uploadedBytes);
  return '--';
}

function torrentColumnAlignClass(isTorrentTable: boolean) {
  return isTorrentTable ? 'w-full text-center' : '';
}

function queueDateCellClass(isTorrentTable: boolean) {
  return isTorrentTable
    ? 'truncate text-center text-muted-foreground tabular-nums'
    : 'truncate pr-4 text-muted-foreground tabular-nums';
}

function queueMetricCellClass(isTorrentTable: boolean) {
  return isTorrentTable
    ? 'text-center tabular-nums text-muted-foreground'
    : 'tabular-nums text-muted-foreground';
}

function formatQueueTime(job: DownloadJob, timeRemaining: number) {
  if (job.state === JobState.Downloading) return formatTime(timeRemaining);
  if (job.state === JobState.Seeding && job.torrent) return formatTorrentRatio(job);
  return '--';
}

function formatTorrentRatio(job: DownloadJob) {
  return typeof job.torrent?.ratio === 'number' ? `${job.torrent.ratio.toFixed(2)}x` : '--';
}

function formatTorrentPeers(job: DownloadJob) {
  const parts = [];
  if (typeof job.torrent?.peers === 'number') parts.push(`${job.torrent.peers} peers`);
  if (typeof job.torrent?.seeds === 'number') parts.push(`${job.torrent.seeds} seeds`);
  return parts.length ? parts.join(' / ') : '--';
}

function torrentDetailTitle(metrics: TorrentDetailMetric[]) {
  return metrics.map((metric) => `${metric.label}: ${torrentMetricValue(metric)}`).join(' / ');
}

function torrentMetricValue(metric: TorrentDetailMetric) {
  if (metric.kind === 'upload') return `${formatBytes(metric.value)}/s`;
  if (metric.kind === 'peers') return `${metric.value} peers`;
  return `${metric.value} seeds`;
}

function torrentMetricTextClass(kind: TorrentDetailMetricKind) {
  return kind === 'peers' ? 'text-sky-300' : 'text-fuchsia-300';
}

function torrentMetricIconClass(kind: TorrentDetailMetricKind) {
  return kind === 'peers' ? 'text-sky-400' : 'text-fuchsia-400';
}

function getHost(rawUrl: string) {
  try {
    return new URL(rawUrl).host;
  } catch {
    return rawUrl;
  }
}

function formatJobDate(timestamp: number | undefined) {
  if (!isValidTimestamp(timestamp)) return '--';

  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(timestamp));
}

function formatFullJobDate(timestamp: number | undefined) {
  if (!isValidTimestamp(timestamp)) return 'No date recorded';

  return new Intl.DateTimeFormat(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(timestamp));
}

function isValidTimestamp(timestamp: number | undefined): timestamp is number {
  return typeof timestamp === 'number' && Number.isFinite(timestamp) && timestamp > 0;
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

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function setsEqual<T>(left: Set<T>, right: Set<T>) {
  if (left.size !== right.size) return false;
  for (const value of left) {
    if (!right.has(value)) return false;
  }
  return true;
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

function queueRowSizeClass(size: QueueRowSize): string {
  switch (size) {
    case 'compact':
      return 'min-h-[28px] py-0 text-xs';
    case 'small':
      return 'min-h-[34px] py-0.5 text-xs';
    case 'large':
      return 'min-h-[54px] py-1.5 text-sm';
    case 'damn':
      return 'min-h-[68px] py-2.5 text-base';
    case 'medium':
    default:
      return 'min-h-[42px] py-1 text-sm';
  }
}

function fileBadgeDensity(size: QueueRowSize): { className: string; iconSize: number } {
  switch (size) {
    case 'compact':
      return { className: 'h-5 w-5', iconSize: 13 };
    case 'small':
      return { className: 'h-6 w-6', iconSize: 15 };
    case 'large':
      return { className: 'h-10 w-10', iconSize: 23 };
    case 'damn':
      return { className: 'h-12 w-12', iconSize: 28 };
    case 'medium':
    default:
      return { className: 'h-7 w-7', iconSize: 18 };
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

function getDetailsMaxHeight(containerHeight: number) {
  if (!Number.isFinite(containerHeight) || containerHeight <= 0) {
    return DETAILS_MAX_HEIGHT;
  }

  return Math.max(
    DETAILS_MIN_HEIGHT,
    Math.min(DETAILS_MAX_HEIGHT, containerHeight - TABLE_MIN_HEIGHT),
  );
}

function CompactDetailItem({
  icon,
  label,
  value,
  accent = false,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  accent?: boolean;
}) {
  return (
    <div className="min-w-0 px-1 py-1">
      <div className="mb-0.5 flex items-center gap-1.5 text-[11px] text-muted-foreground [&>svg]:h-3.5 [&>svg]:w-3.5">
        {icon}
        <span className="truncate">{label}</span>
      </div>
      <div className={`truncate text-xs ${accent ? 'text-primary' : 'text-foreground'}`} title={value}>
        {value}
      </div>
    </div>
  );
}

function snapDetailsHeight(value: number, maxHeight: number) {
  const snapPoints = [
    DETAILS_MIN_HEIGHT,
    Math.min(DETAILS_DEFAULT_HEIGHT, maxHeight),
    Math.min(DETAILS_EXPANDED_HEIGHT, maxHeight),
    maxHeight,
  ]
    .filter((height, index, heights) => height >= DETAILS_MIN_HEIGHT && heights.indexOf(height) === index)
    .sort((a, b) => a - b);

  return snapPoints.reduce((closest, height) => (
    Math.abs(height - value) < Math.abs(closest - value) ? height : closest
  ), snapPoints[0] ?? DETAILS_MIN_HEIGHT);
}

function formatFailureCategory(category: NonNullable<DownloadJob['failureCategory']>) {
  switch (category) {
    case 'network':
      return 'Network';
    case 'http':
      return 'HTTP';
    case 'server':
      return 'Server';
    case 'disk':
      return 'Disk';
    case 'permission':
      return 'Permission';
    case 'resume':
      return 'Resume';
    case 'integrity':
      return 'Integrity';
    default:
      return 'Internal';
  }
}

function formatResumeSupport(resumeSupport: DownloadJob['resumeSupport']) {
  switch (resumeSupport) {
    case 'supported':
      return 'Resumable';
    case 'unsupported':
      return 'Restart required';
    default:
      return 'Unknown';
  }
}
