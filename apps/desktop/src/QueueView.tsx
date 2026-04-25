import React, { useEffect, useRef, useState } from 'react';
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
  GripHorizontal,
  HardDrive,
  MoreHorizontal,
  Pause,
  Pencil,
  Play,
  RotateCcw,
  RotateCw,
  Trash2,
  X,
} from 'lucide-react';

const DETAILS_MIN_HEIGHT = 148;
const DETAILS_CLOSE_THRESHOLD = 118;
const DETAILS_DEFAULT_HEIGHT = 204;
const DETAILS_EXPANDED_HEIGHT = 320;
const DETAILS_MAX_HEIGHT = 420;
const TABLE_MIN_HEIGHT = 180;

interface QueueViewProps {
  jobs: DownloadJob[];
  view: string;
  selectedJobId: string | null;
  onSelect: (id: string) => void;
  onClearSelection: () => void;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRestart: (id: string) => void;
  onRemove: (id: string) => void;
  onDelete: (id: string, deleteFromDisk: boolean) => void;
  onRename: (id: string, filename: string) => void;
  onOpen: (id: string) => void;
  onReveal: (id: string) => void;
}

export function QueueView({
  jobs,
  view,
  selectedJobId,
  onSelect,
  onClearSelection,
  onPause,
  onResume,
  onCancel,
  onRetry,
  onRestart,
  onRemove,
  onDelete,
  onRename,
  onOpen,
  onReveal,
}: QueueViewProps) {
  const selectedJob = selectedJobId ? jobs.find((job) => job.id === selectedJobId) ?? null : null;
  const [openMenuJobId, setOpenMenuJobId] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<{ jobId: string; x: number; y: number } | null>(null);
  const [renamePromptJob, setRenamePromptJob] = useState<DownloadJob | null>(null);
  const [renameBaseName, setRenameBaseName] = useState('');
  const [renameExtension, setRenameExtension] = useState('');
  const [deletePromptJob, setDeletePromptJob] = useState<DownloadJob | null>(null);
  const [deleteFromDisk, setDeleteFromDisk] = useState(false);
  const [detailsHeight, setDetailsHeight] = useState(DETAILS_DEFAULT_HEIGHT);
  const [isResizingDetails, setIsResizingDetails] = useState(false);
  const queueRootRef = useRef<HTMLElement | null>(null);
  const resizeStart = useRef<{ y: number; height: number; containerHeight: number; proposedHeight: number } | null>(null);

  const contextMenuJob = contextMenu ? jobs.find((job) => job.id === contextMenu.jobId) ?? null : null;

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
        <div className="download-table min-w-[960px] overflow-visible border-b border-t border-border bg-card">
          <div className="grid grid-cols-[minmax(280px,2.2fr)_150px_180px_110px_100px_150px_72px] border-b border-border bg-header px-5 py-2 text-xs font-medium text-muted-foreground">
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
                  onClick={() => {
                    onSelect(job.id);
                    setOpenMenuJobId(null);
                    setContextMenu(null);
                  }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    onSelect(job.id);
                    setOpenMenuJobId(null);
                    setContextMenu(getContextMenuPosition(job.id, event.clientX, event.clientY));
                  }}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault();
                      onSelect(job.id);
                    }
                  }}
                  role="button"
                  tabIndex={0}
                  className={`grid min-h-[58px] w-full grid-cols-[minmax(280px,2.2fr)_150px_180px_110px_100px_150px_72px] items-center gap-0 px-5 py-2 text-left text-sm transition ${
                    selected ? 'bg-selected outline outline-1 outline-primary/30' : 'bg-card hover:bg-row-hover'
                  }`}
                >
                  <div className="flex min-w-0 items-center gap-3 pr-4">
                    <FileBadge filename={job.filename} />
                    <div className="min-w-0">
                      <div className="truncate text-sm font-semibold text-foreground" title={job.filename}>
                        {job.filename}
                      </div>
                      <div className="mt-0.5 truncate text-xs text-muted-foreground" title={job.url}>
                        {getHost(job.url)}
                      </div>
                    </div>
                  </div>

                  <div className={`font-medium ${statusClass(job.state)}`}>{statusText(job)}</div>

                  <div className="pr-6">
                    <div className="mb-1.5 text-sm font-medium text-foreground">{formatProgress(job)}</div>
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

                  <div className="relative flex items-center justify-end gap-1" onClick={(event) => event.stopPropagation()}>
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

      {contextMenu && contextMenuJob ? (
        <FileContextMenu
          job={contextMenuJob}
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
          onRename={(job) => {
            setContextMenu(null);
            setRenamePromptJob(job);
            const parsedName = splitFilename(job.filename);
            setRenameBaseName(parsedName.baseName);
            setRenameExtension(parsedName.extension);
          }}
          onDelete={(job) => {
            setContextMenu(null);
            setDeletePromptJob(job);
            setDeleteFromDisk(false);
          }}
        />
      ) : null}

      {selectedJob ? (
        <DownloadDetailsPane
          job={selectedJob}
          onPause={onPause}
          onResume={onResume}
          onCancel={onCancel}
          onRetry={onRetry}
          onRestart={onRestart}
          onRemove={onRemove}
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

      {deletePromptJob ? (
        <DeletePrompt
          job={deletePromptJob}
          deleteFromDisk={deleteFromDisk}
          onDeleteFromDiskChange={setDeleteFromDisk}
          onCancel={() => setDeletePromptJob(null)}
          onDelete={() => {
            onDelete(deletePromptJob.id, deleteFromDisk);
            setDeletePromptJob(null);
          }}
        />
      ) : null}
    </section>
  );
}

function FileContextMenu({
  job,
  x,
  y,
  onOpen,
  onReveal,
  onRename,
  onDelete,
}: {
  job: DownloadJob;
  x: number;
  y: number;
  onOpen: (id: string) => void;
  onReveal: (id: string) => void;
  onRename: (job: DownloadJob) => void;
  onDelete: (job: DownloadJob) => void;
}) {
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
      <MenuItem icon={<Pencil size={16} />} label="Rename" onClick={() => onRename(job)} />
      <MenuItem icon={<Trash2 size={16} />} label="Delete" onClick={() => onDelete(job)} destructive />
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
  job,
  deleteFromDisk,
  onDeleteFromDiskChange,
  onCancel,
  onDelete,
}: {
  job: DownloadJob;
  deleteFromDisk: boolean;
  onDeleteFromDiskChange: (value: boolean) => void;
  onCancel: () => void;
  onDelete: () => void;
}) {
  return (
    <div className="fixed inset-0 z-[80] flex items-center justify-center bg-black/45 p-4">
      <div className="w-full max-w-md rounded-md border border-border bg-card shadow-2xl">
        <div className="border-b border-border px-5 py-4">
          <h2 className="text-base font-semibold text-foreground">Delete Download</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            Remove this download from the list. Disk deletion requires explicit confirmation below.
          </p>
        </div>
        <div className="space-y-4 px-5 py-4">
          <div className="truncate text-sm font-medium text-foreground" title={job.filename}>
            {job.filename}
          </div>
          <label className="flex cursor-pointer items-start gap-3 py-1 text-sm">
            <input
              type="checkbox"
              checked={deleteFromDisk}
              onChange={(event) => onDeleteFromDiskChange(event.target.checked)}
              className="mt-0.5 h-4 w-4 accent-primary"
            />
            <span>
              <span className="block font-medium text-foreground">Delete file from disk</span>
              <span className="mt-1 block break-all text-muted-foreground">
                {job.targetPath || 'No file path is recorded for this download.'}
              </span>
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
            Delete
          </button>
        </div>
      </div>
    </div>
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
  onRemove,
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
  onRemove: (id: string) => void;
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
  const compact = height <= DETAILS_MIN_HEIGHT + 8;
  const detailItems = [
    { icon: <Globe size={16} />, label: 'Source URL:', value: job.url, accent: true },
    { icon: <FolderOpen size={16} />, label: 'Destination:', value: job.targetPath || 'No destination recorded yet.' },
    { icon: <HardDrive size={16} />, label: 'File Size:', value: job.totalBytes > 0 ? `${formatBytes(job.totalBytes)} (${job.totalBytes.toLocaleString()} bytes)` : 'Unknown' },
    { icon: <Download size={16} />, label: 'Downloaded:', value: `${formatBytes(job.downloadedBytes)} (${job.downloadedBytes.toLocaleString()} bytes)` },
    { icon: <Clock3 size={16} />, label: 'Remaining', value: job.state === JobState.Downloading ? formatTime(job.eta) : '--' },
    { icon: <Check size={16} />, label: 'Status:', value: statusText(job) },
    { icon: <RotateCw size={16} />, label: 'Resume:', value: formatResumeSupport(job.resumeSupport) },
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
      className={`details-pane relative shrink-0 border-t border-border bg-card ${compact ? 'overflow-hidden' : 'overflow-auto'}`}
      style={{ height }}
    >
      <div
        className="absolute left-0 right-0 top-0 z-10 flex h-3 cursor-ns-resize items-center justify-center bg-card/95 text-muted-foreground transition hover:text-foreground"
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
        className="absolute right-3 top-4 z-20 flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition hover:bg-muted hover:text-foreground"
        title="Hide details"
        aria-label="Hide details"
      >
        <X size={16} />
      </button>

      <div className={`${compact ? 'flex h-full min-w-0 items-center gap-4 px-6 py-4 pt-5' : 'flex min-w-0 gap-5 px-6 py-5 pt-6'}`}>
        <div className={`${compact ? 'flex w-16 shrink-0 justify-center' : 'flex w-20 shrink-0 justify-center'}`}>
          <FileBadge filename={job.filename} large />
        </div>

        <div className="min-w-0 flex-1 pr-9">
          <h3 className={`${compact ? 'mb-2 text-sm' : 'mb-3 text-base'} truncate font-semibold text-foreground`} title={job.filename}>
            {job.filename}
          </h3>
          {compact ? (
            <div className="overflow-x-auto overflow-y-hidden pb-1">
              <div className="grid min-w-[1180px] grid-flow-col grid-rows-2 gap-x-5 gap-y-2 text-sm">
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
  onRemove,
  onReveal,
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
  onRemove: (id: string) => void;
  onReveal: (id: string) => void;
}) {
  const canPause = [JobState.Queued, JobState.Starting, JobState.Downloading].includes(job.state);
  const canResume = job.state === JobState.Paused;
  const canRetry = [JobState.Failed, JobState.Canceled].includes(job.state);
  const canCancel = ![JobState.Completed, JobState.Canceled, JobState.Failed].includes(job.state);

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
      <IconButton title="More actions" onClick={onToggleMenu}><MoreHorizontal size={18} /></IconButton>

      {menuOpen ? (
        <div
          className="absolute right-0 top-9 z-50 w-44 overflow-hidden rounded-md border border-border bg-card py-1 shadow-2xl"
          onClick={(event) => event.stopPropagation()}
        >
          {job.targetPath ? (
            <MenuItem icon={<FolderOpen size={16} />} label="Show in folder" onClick={() => runMenuAction(onReveal)} />
          ) : null}
          {canRetry ? (
            <MenuItem icon={<RotateCcw size={16} />} label="Restart" onClick={() => runMenuAction(onRestart)} />
          ) : null}
          {canCancel ? (
            <MenuItem icon={<X size={16} />} label="Cancel" onClick={() => runMenuAction(onCancel)} />
          ) : null}
          <MenuItem icon={<Trash2 size={16} />} label="Remove" onClick={() => runMenuAction(onRemove)} destructive />
        </div>
      ) : null}
    </>
  );
}

function FileBadge({ filename, large = false }: { filename: string; large?: boolean }) {
  const ext = filename.split('.').pop()?.toLowerCase() || '';
  const iconSize = large ? 28 : 20;
  const icon = getFileIcon(ext, iconSize);
  const label = ext ? ext.slice(0, 4).toUpperCase() : 'FILE';

  return (
    <div className={`file-badge relative flex shrink-0 items-center justify-center rounded-sm border border-border bg-background ${large ? 'h-[76px] w-14' : 'h-10 w-9'}`}>
      <div className="absolute right-0 top-0 h-2.5 w-2.5 border-b border-l border-border bg-surface" />
      <div className="text-primary">{icon}</div>
      {large ? <div className="absolute bottom-1.5 text-[10px] font-semibold text-muted-foreground">{label}</div> : null}
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

function formatProgress(job: DownloadJob) {
  if (job.state === JobState.Queued) return '0%';
  if (job.state === JobState.Canceled) return '--';
  return `${job.progress.toFixed(0)}%`;
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
    default:
      return 'No downloads';
  }
}

function statusText(job: DownloadJob) {
  if (job.state === JobState.Failed && job.failureCategory) {
    return `${formatFailureCategory(job.failureCategory)} Error`;
  }

  switch (job.state) {
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

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function getContextMenuPosition(jobId: string, x: number, y: number) {
  const menuWidth = 192;
  const menuHeight = 148;
  return {
    jobId,
    x: clamp(x, 8, Math.max(8, window.innerWidth - menuWidth - 8)),
    y: clamp(y, 8, Math.max(8, window.innerHeight - menuHeight - 8)),
  };
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
    <div className="min-w-0">
      <div className="mb-0.5 flex items-center gap-2 text-xs text-muted-foreground">
        {icon}
        <span>{label}</span>
      </div>
      <div className={`truncate text-sm ${accent ? 'text-primary' : 'text-foreground'}`} title={value}>
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
