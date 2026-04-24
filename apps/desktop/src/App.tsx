import React, { useEffect, useMemo, useState } from 'react';
import { ConnectionState, JobState } from './types';
import type { DownloadJob, Settings, ToastMessage } from './types';
import { QueueView } from './QueueView';
import { SettingsPage } from './SettingsPage';
import { ToastArea } from './ToastArea';
import { AddDownloadModal } from './AddDownloadModal';
import { Titlebar } from './Titlebar';
import {
  browseDirectory,
  cancelJob,
  exportDiagnosticsReport,
  getDiagnostics,
  getAppSnapshot,
  openInstallDocs,
  openJobFile,
  pauseJob,
  revealJobInFolder,
  removeJob,
  resumeJob,
  retryJob,
  runHostRegistrationFix,
  saveSettings,
  subscribeToStateChanged,
} from './backend';
import {
  CheckCircle2,
  Clock3,
  Download,
  Filter,
  FolderOpen,
  Gauge,
  Pause,
  Play,
  Plus,
  Search,
  Settings as SettingsIcon,
  Trash2,
  Wifi,
  WifiOff,
} from 'lucide-react';
import type { DiagnosticsSnapshot } from './types';

type ViewState = 'all' | 'active' | 'queued' | 'completed' | 'settings';
type SortMode = 'status' | 'name' | 'progress' | 'size';

const activeStates = [JobState.Starting, JobState.Downloading, JobState.Paused];
const finishedStates = [JobState.Completed, JobState.Canceled, JobState.Failed];

export default function App() {
  const [connectionState, setConnectionState] = useState<ConnectionState>(ConnectionState.Checking);
  const [jobs, setJobs] = useState<DownloadJob[]>([]);
  const [settings, setSettings] = useState<Settings>({
    downloadDirectory: 'C:/Downloads',
    maxConcurrentDownloads: 3,
    notificationsEnabled: true,
    theme: 'system',
  });
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const [view, setView] = useState<ViewState>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [sortMode, setSortMode] = useState<SortMode>('status');
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsSnapshot | null>(null);

  useEffect(() => {
    let isMounted = true;
    let dispose: (() => void | Promise<void>) | undefined;

    async function initialize() {
      try {
        const [snapshot, diagnosticSnapshot] = await Promise.all([getAppSnapshot(), getDiagnostics()]);
        if (isMounted) {
          setConnectionState(snapshot.connectionState);
          setJobs(snapshot.jobs);
          setSettings(snapshot.settings);
          setDiagnostics(diagnosticSnapshot);
        }

        dispose = await subscribeToStateChanged((nextSnapshot) => {
          setConnectionState(nextSnapshot.connectionState);
          setJobs(nextSnapshot.jobs);
          setSettings(nextSnapshot.settings);
          void refreshDiagnostics();
        });
      } catch (error) {
        if (!isMounted) return;
        setConnectionState(ConnectionState.Error);
        addToast({
          type: 'error',
          title: 'Backend Error',
          message: error instanceof Error ? error.message : 'Failed to load desktop state.',
          autoClose: false,
        });
      }
    }

    void initialize();

    return () => {
      isMounted = false;
      void dispose?.();
    };
  }, []);

  useEffect(() => {
    function applyTheme() {
      const shouldUseDark =
        settings.theme === 'dark' ||
        (settings.theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);

      document.documentElement.classList.toggle('dark', shouldUseDark);
    }

    applyTheme();
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    media.addEventListener('change', applyTheme);
    return () => media.removeEventListener('change', applyTheme);
  }, [settings.theme]);

  function addToast(toast: Omit<ToastMessage, 'id'>) {
    setToasts((prev) => [...prev, { ...toast, id: crypto.randomUUID() }]);
  }

  function removeToast(id: string) {
    setToasts((prev) => prev.filter((toast) => toast.id !== id));
  }

  async function refreshDiagnostics() {
    try {
      setDiagnostics(await getDiagnostics());
    } catch (error) {
      addToast({ type: 'error', title: 'Diagnostics Failed', message: getErrorMessage(error) });
    }
  }

  async function handlePause(id: string) {
    try {
      await pauseJob(id);
    } catch (error) {
      addToast({ type: 'error', title: 'Pause Failed', message: getErrorMessage(error) });
    }
  }

  async function handleResume(id: string) {
    try {
      await resumeJob(id);
    } catch (error) {
      addToast({ type: 'error', title: 'Resume Failed', message: getErrorMessage(error) });
    }
  }

  async function handleCancel(id: string) {
    try {
      await cancelJob(id);
    } catch (error) {
      addToast({ type: 'error', title: 'Cancel Failed', message: getErrorMessage(error) });
    }
  }

  async function handleRetry(id: string) {
    try {
      await retryJob(id);
      addToast({ type: 'info', title: 'Retrying Download', message: 'The download was added back to the queue.' });
    } catch (error) {
      addToast({ type: 'error', title: 'Retry Failed', message: getErrorMessage(error) });
    }
  }

  async function handleRemove(id: string) {
    try {
      await removeJob(id);
      if (selectedJobId === id) setSelectedJobId(null);
    } catch (error) {
      addToast({ type: 'error', title: 'Remove Failed', message: getErrorMessage(error) });
    }
  }

  async function handleOpenFile(id: string) {
    try {
      await openJobFile(id);
    } catch (error) {
      addToast({ type: 'error', title: 'Open Failed', message: getErrorMessage(error) });
    }
  }

  async function handleReveal(id: string) {
    try {
      await revealJobInFolder(id);
    } catch (error) {
      addToast({ type: 'error', title: 'Reveal Failed', message: getErrorMessage(error) });
    }
  }

  async function handleOpenInstallDocs() {
    try {
      await openInstallDocs();
    } catch (error) {
      addToast({ type: 'error', title: 'Open Docs Failed', message: getErrorMessage(error) });
    }
  }

  async function handleRunHostRegistrationFix() {
    try {
      await runHostRegistrationFix();
      await refreshDiagnostics();
      addToast({ type: 'success', title: 'Registration Complete', message: 'Native host registration was refreshed.' });
    } catch (error) {
      addToast({ type: 'error', title: 'Registration Failed', message: getErrorMessage(error) });
    }
  }

  async function handleCopyDiagnostics() {
    if (!diagnostics) {
      addToast({ type: 'warning', title: 'Diagnostics Unavailable', message: 'Refresh diagnostics before copying the report.' });
      return;
    }

    try {
      await navigator.clipboard.writeText(formatDiagnosticsReport(diagnostics));
      addToast({ type: 'success', title: 'Diagnostics Copied', message: 'The diagnostics report was copied to the clipboard.' });
    } catch (error) {
      addToast({ type: 'error', title: 'Copy Failed', message: getErrorMessage(error) });
    }
  }

  async function handleExportDiagnostics() {
    try {
      const exportedPath = await exportDiagnosticsReport();
      if (!exportedPath) {
        addToast({ type: 'info', title: 'Export Cancelled', message: 'No diagnostics report was saved.' });
        return;
      }

      addToast({ type: 'success', title: 'Report Exported', message: `Saved diagnostics to ${exportedPath}.` });
    } catch (error) {
      addToast({ type: 'error', title: 'Export Failed', message: getErrorMessage(error) });
    }
  }

  async function handleSaveSettings(newSettings: Settings) {
    try {
      const savedSettings = await saveSettings(newSettings);
      setSettings(savedSettings);
      await refreshDiagnostics();
      setView('all');
      addToast({ type: 'success', title: 'Settings Saved', message: 'Preferences updated successfully.' });
    } catch (error) {
      addToast({ type: 'error', title: 'Save Failed', message: getErrorMessage(error) });
    }
  }

  async function handleBrowseDirectory(): Promise<string | null> {
    try {
      const selectedDirectory = await browseDirectory();
      return selectedDirectory;
    } catch (error) {
      addToast({ type: 'error', title: 'Browse Failed', message: getErrorMessage(error) });
      return null;
    }
  }

  const counts = useMemo(() => {
    return {
      all: jobs.length,
      active: jobs.filter((job) => activeStates.includes(job.state)).length,
      queued: jobs.filter((job) => job.state === JobState.Queued).length,
      completed: jobs.filter((job) => finishedStates.includes(job.state)).length,
    };
  }, [jobs]);

  const displayedJobs = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    const filtered = jobs.filter((job) => {
      if (view === 'settings') return false;
      if (view === 'active' && !activeStates.includes(job.state)) return false;
      if (view === 'queued' && job.state !== JobState.Queued) return false;
      if (view === 'completed' && !finishedStates.includes(job.state)) return false;
      if (!query) return true;
      return `${job.filename} ${job.url} ${job.targetPath ?? ''}`.toLowerCase().includes(query);
    });

    return [...filtered].sort((a, b) => {
      if (sortMode === 'name') return a.filename.localeCompare(b.filename);
      if (sortMode === 'progress') return b.progress - a.progress;
      if (sortMode === 'size') return b.totalBytes - a.totalBytes;
      return statusRank(a.state) - statusRank(b.state) || a.filename.localeCompare(b.filename);
    });
  }, [jobs, searchQuery, sortMode, view]);

  useEffect(() => {
    if (view === 'settings') return;
    if (displayedJobs.length === 0) {
      setSelectedJobId(null);
      return;
    }
    if (!selectedJobId || !displayedJobs.some((job) => job.id === selectedJobId)) {
      setSelectedJobId(displayedJobs[0].id);
    }
  }, [displayedJobs, selectedJobId, view]);

  const selectedJob = jobs.find((job) => job.id === selectedJobId) ?? null;
  const totalDownloadSpeed = jobs
    .filter((job) => job.state === JobState.Downloading)
    .reduce((total, job) => total + job.speed, 0);

  return (
    <div className="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
      <Titlebar />

      <div className="flex min-h-0 flex-1 overflow-hidden">
        <aside className="download-sidebar flex w-[252px] shrink-0 flex-col justify-between border-r border-border bg-sidebar px-3 py-3">
          <nav className="flex flex-col gap-1">
            <NavItem icon={<Download size={21} />} label="All Downloads" count={counts.all} active={view === 'all'} onClick={() => setView('all')} />
            <NavItem icon={<Gauge size={21} />} label="Active" count={counts.active} active={view === 'active'} onClick={() => setView('active')} />
            <NavItem icon={<Clock3 size={21} />} label="Queued" count={counts.queued} active={view === 'queued'} onClick={() => setView('queued')} />
            <NavItem icon={<CheckCircle2 size={21} />} label="Completed" count={counts.completed} active={view === 'completed'} onClick={() => setView('completed')} />
          </nav>

          <div className="space-y-3">
            <div className="h-px bg-border" />
            <NavItem icon={<SettingsIcon size={21} />} label="Settings" active={view === 'settings'} onClick={() => setView('settings')} />
          </div>
        </aside>

        <main className="flex min-w-0 flex-1 flex-col overflow-hidden bg-surface">
          {view === 'settings' ? (
            <div className="min-h-0 flex-1 overflow-y-auto">
              <SettingsPage
                settings={settings}
                diagnostics={diagnostics}
                onSave={handleSaveSettings}
                onBrowseDirectory={handleBrowseDirectory}
                onCancel={() => setView('all')}
                onRefreshDiagnostics={refreshDiagnostics}
                onOpenInstallDocs={handleOpenInstallDocs}
                onRunHostRegistrationFix={handleRunHostRegistrationFix}
                onCopyDiagnostics={handleCopyDiagnostics}
                onExportDiagnostics={handleExportDiagnostics}
              />
            </div>
          ) : (
            <>
              <CommandBar
                selectedJob={selectedJob}
                searchQuery={searchQuery}
                sortMode={sortMode}
                onSearchChange={setSearchQuery}
                onSortChange={setSortMode}
                onAdd={() => setIsAddModalOpen(true)}
                onResume={() => selectedJob && void handleResume(selectedJob.id)}
                onPause={() => selectedJob && void handlePause(selectedJob.id)}
                onRemove={() => selectedJob && void handleRemove(selectedJob.id)}
                onReveal={() => selectedJob && void handleReveal(selectedJob.id)}
                onCycleFilter={() => setView(nextFilterView(view))}
              />

              <QueueView
                jobs={displayedJobs}
                view={view}
                selectedJobId={selectedJobId}
                onSelect={setSelectedJobId}
                onPause={handlePause}
                onResume={handleResume}
                onCancel={handleCancel}
                onRetry={handleRetry}
                onRemove={handleRemove}
                onOpen={handleOpenFile}
                onReveal={handleReveal}
              />

              <StatusBar
                activeCount={counts.active}
                downloadSpeed={totalDownloadSpeed}
                connectionState={connectionState}
                connectionSlots={settings.maxConcurrentDownloads}
              />
            </>
          )}
        </main>
      </div>

      <ToastArea toasts={toasts} onDismiss={removeToast} />

      {isAddModalOpen && <AddDownloadModal onClose={() => setIsAddModalOpen(false)} />}
    </div>
  );
}

function CommandBar({
  selectedJob,
  searchQuery,
  sortMode,
  onSearchChange,
  onSortChange,
  onAdd,
  onResume,
  onPause,
  onRemove,
  onReveal,
  onCycleFilter,
}: {
  selectedJob: DownloadJob | null;
  searchQuery: string;
  sortMode: SortMode;
  onSearchChange: (value: string) => void;
  onSortChange: (value: SortMode) => void;
  onAdd: () => void;
  onResume: () => void;
  onPause: () => void;
  onRemove: () => void;
  onReveal: () => void;
  onCycleFilter: () => void;
}) {
  const canResume = selectedJob ? [JobState.Paused, JobState.Failed, JobState.Canceled].includes(selectedJob.state) : false;
  const canPause = selectedJob ? [JobState.Queued, JobState.Starting, JobState.Downloading].includes(selectedJob.state) : false;
  const hasSelection = Boolean(selectedJob);

  return (
    <div className="command-bar flex h-20 shrink-0 items-center justify-between gap-4 border-b border-border bg-command px-6">
      <div className="flex min-w-0 items-center gap-2">
        <ToolbarButton icon={<Plus size={20} />} label="New Download" onClick={onAdd} strong />
        <div className="mx-3 h-8 w-px bg-border" />
        <ToolbarButton icon={<Play size={18} />} label="Resume" onClick={onResume} disabled={!canResume} />
        <ToolbarButton icon={<Pause size={18} />} label="Pause" onClick={onPause} disabled={!canPause} />
        <ToolbarButton icon={<Trash2 size={18} />} label="Remove" onClick={onRemove} disabled={!hasSelection} />
        <ToolbarButton icon={<FolderOpen size={18} />} label="Open Folder" onClick={onReveal} disabled={!selectedJob?.targetPath} />
      </div>

      <div className="flex min-w-[440px] max-w-[560px] flex-1 items-center justify-end gap-3">
        <label className="relative min-w-0 flex-1">
          <Search size={20} className="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <input
            value={searchQuery}
            onChange={(event) => onSearchChange(event.target.value)}
            className="h-11 w-full rounded-md border border-input bg-background pl-11 pr-4 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
            placeholder="Search downloads..."
          />
        </label>
        <select
          value={sortMode}
          onChange={(event) => onSortChange(event.target.value as SortMode)}
          className="h-11 rounded-md border border-input bg-background px-4 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
          aria-label="Sort downloads"
        >
          <option value="status">Sort by: Status</option>
          <option value="name">Sort by: Name</option>
          <option value="progress">Sort by: Progress</option>
          <option value="size">Sort by: Size</option>
        </select>
        <button
          onClick={onCycleFilter}
          className="flex h-11 w-11 items-center justify-center rounded-md border border-transparent text-muted-foreground transition hover:border-input hover:bg-muted hover:text-foreground"
          title="Cycle filter"
          aria-label="Cycle filter"
        >
          <Filter size={22} />
        </button>
      </div>
    </div>
  );
}

function ToolbarButton({
  icon,
  label,
  onClick,
  disabled = false,
  strong = false,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  disabled?: boolean;
  strong?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`flex h-10 items-center gap-2 rounded-md px-3 text-sm font-medium transition ${
        strong
          ? 'text-foreground hover:bg-muted'
          : 'text-muted-foreground hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-muted-foreground'
      }`}
    >
      {icon}
      <span>{label}</span>
    </button>
  );
}

function NavItem({
  icon,
  label,
  count,
  active,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  count?: number;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`group relative flex h-14 w-full items-center gap-3 rounded-md px-4 text-left text-[15px] font-medium transition ${
        active ? 'bg-primary-soft text-primary shadow-[inset_3px_0_0_var(--color-primary)]' : 'text-foreground hover:bg-muted'
      }`}
    >
      <span className="shrink-0">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {typeof count === 'number' ? (
        <span className={`rounded-full px-2.5 py-1 text-xs ${active ? 'bg-primary/10 text-primary' : 'bg-muted text-muted-foreground'}`}>
          {count}
        </span>
      ) : null}
    </button>
  );
}

function StatusBar({
  activeCount,
  downloadSpeed,
  connectionState,
  connectionSlots,
}: {
  activeCount: number;
  downloadSpeed: number;
  connectionState: ConnectionState;
  connectionSlots: number;
}) {
  const isConnected = connectionState === ConnectionState.Connected;

  return (
    <footer className="status-bar flex h-[68px] shrink-0 items-center justify-between border-t border-border bg-command px-10 text-sm text-muted-foreground">
      <div className="flex items-center gap-5">
        <span className="flex items-center gap-2">
          <Gauge size={20} className="text-primary" />
          {activeCount} active downloads
        </span>
        <span className="h-6 w-px bg-border" />
        <span className="flex items-center gap-2 text-foreground">
          <Download size={20} className="text-primary" />
          {formatBytes(downloadSpeed)}/s
        </span>
        <span className="text-muted-foreground">( down {formatBytes(downloadSpeed)}/s</span>
        <span className="text-muted-foreground">up 0 B/s )</span>
      </div>

      <div className="flex items-center gap-4">
        <span className={`flex items-center gap-2 ${isConnected ? 'text-foreground' : 'text-destructive'}`}>
          {isConnected ? <Wifi size={20} /> : <WifiOff size={20} />}
          {formatConnectionState(connectionState)}
        </span>
        <span className="text-muted-foreground">Connections: {connectionSlots}</span>
      </div>
    </footer>
  );
}

function nextFilterView(view: ViewState): ViewState {
  if (view === 'all') return 'active';
  if (view === 'active') return 'queued';
  if (view === 'queued') return 'completed';
  return 'all';
}

function statusRank(state: JobState) {
  switch (state) {
    case JobState.Downloading:
      return 0;
    case JobState.Starting:
      return 1;
    case JobState.Queued:
      return 2;
    case JobState.Paused:
      return 3;
    case JobState.Failed:
      return 4;
    case JobState.Completed:
      return 5;
    case JobState.Canceled:
      return 6;
    default:
      return 7;
  }
}

function formatBytes(bytes: number, decimals = 1) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(decimals))} ${sizes[i]}`;
}

function formatConnectionState(state: ConnectionState) {
  return state.replaceAll('_', ' ').replace(/\b\w/g, (value) => value.toUpperCase());
}

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : 'Unexpected error.';
}

function formatDiagnosticsReport(diagnostics: DiagnosticsSnapshot): string {
  const lines = [
    'Simple Download Manager Diagnostics',
    `Connection State: ${diagnostics.connectionState}`,
    `Last Host Contact: ${diagnostics.lastHostContactSecondsAgo ?? 'never'} seconds ago`,
    `Queue Total: ${diagnostics.queueSummary.total}`,
    `Queue Active: ${diagnostics.queueSummary.active}`,
    `Queue Queued: ${diagnostics.queueSummary.queued}`,
    `Queue Downloading: ${diagnostics.queueSummary.downloading}`,
    `Queue Completed: ${diagnostics.queueSummary.completed}`,
    `Queue Failed: ${diagnostics.queueSummary.failed}`,
    `Host Registration Status: ${diagnostics.hostRegistration.status}`,
    '',
    'Host Registration Entries:',
  ];

  for (const entry of diagnostics.hostRegistration.entries) {
    lines.push(`- ${entry.browser}`);
    lines.push(`  Registry: ${entry.registryPath}`);
    lines.push(`  Manifest: ${entry.manifestPath ?? 'missing'}`);
    lines.push(`  Manifest Exists: ${entry.manifestExists}`);
    lines.push(`  Host Binary: ${entry.hostBinaryPath ?? 'missing'}`);
    lines.push(`  Host Binary Exists: ${entry.hostBinaryExists}`);
  }

  return lines.join('\n');
}
