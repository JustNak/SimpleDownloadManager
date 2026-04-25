import React, { useEffect, useMemo, useState } from 'react';
import { ConnectionState, JobState } from './types';
import type { DownloadJob, Settings, ToastMessage } from './types';
import { QueueView } from './QueueView';
import { SettingsPage } from './SettingsPage';
import { ToastArea } from './ToastArea';
import { AddDownloadModal } from './AddDownloadModal';
import { Titlebar } from './Titlebar';
import { getErrorMessage } from './errors';
import { loadInitialAppData } from './appBootstrap';
import {
  browseDirectory,
  cancelJob,
  deleteJob,
  exportDiagnosticsReport,
  getDiagnostics,
  getAppSnapshot,
  openInstallDocs,
  openJobFile,
  pauseAllJobs,
  pauseJob,
  revealJobInFolder,
  removeJob,
  renameJob,
  resumeAllJobs,
  resumeJob,
  restartJob,
  retryJob,
  runHostRegistrationFix,
  saveSettings,
  subscribeToStateChanged,
  subscribeToSelectedJobRequested,
  testExtensionHandoff,
} from './backend';
import type { AddJobResult } from './backend';
import {
  AlertTriangle,
  CheckCircle2,
  Clock3,
  Download,
  Filter,
  Gauge,
  Pause,
  Play,
  Plus,
  Search,
  Settings as SettingsIcon,
  Wifi,
  WifiOff,
} from 'lucide-react';
import type { DiagnosticsSnapshot } from './types';
import type { DesktopSnapshot } from './backend';

type ViewState = 'all' | 'attention' | 'active' | 'queued' | 'completed' | 'settings';
type SortMode = 'status' | 'name' | 'progress' | 'size';

const DEFAULT_ACCENT_COLOR = '#3b82f6';
const DEFAULT_DOWNLOAD_DIRECTORY = 'C:\\Users\\You\\Downloads';
const activeStates = [JobState.Starting, JobState.Downloading, JobState.Paused];
const finishedStates = [JobState.Completed, JobState.Canceled];

export default function App() {
  const [connectionState, setConnectionState] = useState<ConnectionState>(ConnectionState.Checking);
  const [jobs, setJobs] = useState<DownloadJob[]>([]);
  const [settings, setSettings] = useState<Settings>({
    downloadDirectory: DEFAULT_DOWNLOAD_DIRECTORY,
    maxConcurrentDownloads: 3,
    autoRetryAttempts: 3,
    speedLimitKibPerSecond: 0,
    notificationsEnabled: true,
    theme: 'system',
    accentColor: DEFAULT_ACCENT_COLOR,
    startOnStartup: false,
    startupLaunchMode: 'open',
    extensionIntegration: {
      enabled: true,
      downloadHandoffMode: 'ask',
      listenPort: 1420,
      contextMenuEnabled: true,
      showProgressAfterHandoff: true,
      showBadgeStatus: true,
      excludedHosts: [],
      ignoredFileExtensions: [],
    },
  });
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const [view, setView] = useState<ViewState>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [sortMode, setSortMode] = useState<SortMode>('status');
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsSnapshot | null>(null);
  const [settingsDraft, setSettingsDraft] = useState<Settings | null>(null);
  const [settingsDirty, setSettingsDirty] = useState(false);
  const [pendingSettingsView, setPendingSettingsView] = useState<ViewState | null>(null);
  const [isUnsavedSettingsPromptOpen, setIsUnsavedSettingsPromptOpen] = useState(false);
  const [isSavingSettings, setIsSavingSettings] = useState(false);

  useEffect(() => {
    let isMounted = true;
    let dispose: (() => void | Promise<void>) | undefined;

    async function initialize() {
      try {
        const initialData = await loadInitialAppData(getAppSnapshot, getDiagnostics);
        if (!isMounted) return;

        if (!initialData.snapshot) {
          throw initialData.snapshotError ?? new Error('Failed to load desktop state.');
        }

        applyDesktopSnapshot(initialData.snapshot);
        if (initialData.diagnostics) {
          setDiagnostics(initialData.diagnostics);
        } else if (initialData.diagnosticsError) {
          addToast({
            type: 'warning',
            title: 'Diagnostics Unavailable',
            message: getErrorMessage(initialData.diagnosticsError, 'Download state loaded, but diagnostics could not be refreshed.'),
          });
        }

        dispose = await subscribeToStateChanged((nextSnapshot) => {
          applyDesktopSnapshot(nextSnapshot);
          void refreshDiagnostics();
        });
      } catch (error) {
        if (isMounted) {
          setConnectionState(ConnectionState.Error);
          addToast({
            type: 'error',
            title: 'Backend Error',
            message: getErrorMessage(error, 'Failed to load desktop state.'),
            autoClose: false,
          });
        }
      }
    }

    void initialize();

    return () => {
      isMounted = false;
      void dispose?.();
    };
  }, []);

  useEffect(() => {
    let dispose: (() => void | Promise<void>) | undefined;

    async function subscribe() {
      dispose = await subscribeToSelectedJobRequested((jobId) => {
        setView('all');
        setSelectedJobId(jobId);
      });
    }

    void subscribe();
    return () => {
      void dispose?.();
    };
  }, []);

  useEffect(() => {
    const refresh = () => {
      void refreshSnapshotFromBackend();
    };
    const refreshWhenVisible = () => {
      if (document.visibilityState === 'visible') {
        refresh();
      }
    };

    window.addEventListener('focus', refresh);
    document.addEventListener('visibilitychange', refreshWhenVisible);
    return () => {
      window.removeEventListener('focus', refresh);
      document.removeEventListener('visibilitychange', refreshWhenVisible);
    };
  }, []);

  useEffect(() => {
    function applyTheme() {
      const shouldUseOled = settings.theme === 'oled_dark';
      const shouldUseDark =
        shouldUseOled ||
        settings.theme === 'dark' ||
        (settings.theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);

      document.documentElement.classList.toggle('dark', shouldUseDark);
      document.documentElement.classList.toggle('oled-dark', shouldUseOled);
      applyAccentColor(settings.accentColor);
    }

    applyTheme();
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    media.addEventListener('change', applyTheme);
    return () => media.removeEventListener('change', applyTheme);
  }, [settings.accentColor, settings.theme]);

  function requestViewChange(nextView: ViewState) {
    if (nextView === view) return;

    if (view === 'settings' && settingsDirty) {
      setPendingSettingsView(nextView);
      setIsUnsavedSettingsPromptOpen(true);
      return;
    }

    setView(nextView);
  }

  function applyDesktopSnapshot(snapshot: DesktopSnapshot) {
    setConnectionState(snapshot.connectionState);
    setJobs(snapshot.jobs);
    setSettings(snapshot.settings);
  }

  async function refreshSnapshotFromBackend() {
    try {
      applyDesktopSnapshot(await getAppSnapshot());
    } catch (error) {
      setConnectionState(ConnectionState.Error);
      addToast({
        type: 'error',
        title: 'Refresh Failed',
        message: getErrorMessage(error, 'Failed to refresh desktop state.'),
      });
    }
  }

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

  async function handlePauseAll() {
    try {
      await pauseAllJobs();
      addToast({ type: 'info', title: 'Queue Paused', message: 'Active and queued downloads were paused.' });
    } catch (error) {
      addToast({ type: 'error', title: 'Pause Queue Failed', message: getErrorMessage(error) });
    }
  }

  async function handleResumeAll() {
    try {
      await resumeAllJobs();
      addToast({ type: 'info', title: 'Queue Resumed', message: 'Paused and interrupted downloads were queued again.' });
    } catch (error) {
      addToast({ type: 'error', title: 'Resume Queue Failed', message: getErrorMessage(error) });
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

  async function handleRestart(id: string) {
    try {
      await restartJob(id);
      addToast({ type: 'info', title: 'Restarting Download', message: 'Partial progress was cleared and the download was queued again.' });
    } catch (error) {
      addToast({ type: 'error', title: 'Restart Failed', message: getErrorMessage(error) });
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

  async function handleDelete(id: string, deleteFromDisk: boolean) {
    try {
      await deleteJob(id, deleteFromDisk);
      if (selectedJobId === id) setSelectedJobId(null);
      addToast({
        type: 'success',
        title: 'Download Deleted',
        message: deleteFromDisk ? 'Removed from the list and deleted from disk.' : 'Removed from the download list.',
      });
    } catch (error) {
      addToast({ type: 'error', title: 'Delete Failed', message: getErrorMessage(error) });
    }
  }

  async function handleRename(id: string, filename: string) {
    try {
      await renameJob(id, filename);
      addToast({ type: 'success', title: 'Download Renamed', message: `Renamed to ${filename}.` });
    } catch (error) {
      addToast({ type: 'error', title: 'Rename Failed', message: getErrorMessage(error) });
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

  async function handleTestExtensionHandoff() {
    try {
      await testExtensionHandoff();
      addToast({ type: 'info', title: 'Test Handoff Started', message: 'A browser-style download prompt was opened.' });
    } catch (error) {
      addToast({ type: 'error', title: 'Test Handoff Failed', message: getErrorMessage(error) });
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

  async function handleSaveSettings(newSettings: Settings, nextView: ViewState = 'all'): Promise<boolean> {
    setIsSavingSettings(true);
    try {
      const savedSettings = await saveSettings(newSettings);
      setSettings(savedSettings);
      setSettingsDraft(null);
      setSettingsDirty(false);
      setPendingSettingsView(null);
      setIsUnsavedSettingsPromptOpen(false);
      await refreshDiagnostics();
      setView(nextView);
      addToast({ type: 'success', title: 'Settings Saved', message: 'Preferences updated successfully.' });
      return true;
    } catch (error) {
      addToast({ type: 'error', title: 'Save Failed', message: getErrorMessage(error) });
      return false;
    } finally {
      setIsSavingSettings(false);
    }
  }

  function discardSettingsChanges() {
    const nextView = pendingSettingsView ?? 'all';
    setSettingsDraft(null);
    setSettingsDirty(false);
    setPendingSettingsView(null);
    setIsUnsavedSettingsPromptOpen(false);
    setView(nextView);
  }

  async function saveSettingsAndLeave() {
    const nextSettings = settingsDraft ?? settings;
    const nextView = pendingSettingsView ?? 'all';
    await handleSaveSettings(nextSettings, nextView);
  }

  function handleAddDownloadResult(result: AddJobResult) {
    setSelectedJobId(result.jobId);

    if (result.status === 'duplicate_existing_job') {
      setView('all');
      addToast({
        type: 'info',
        title: 'Already in Queue',
        message: `${result.filename} is already in the download list.`,
      });
      return;
    }

    setView('queued');
    addToast({
      type: 'success',
      title: 'Download Added',
      message: `${result.filename} was added to the queue.`,
    });
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
      attention: jobs.filter(jobNeedsAttention).length,
      queued: jobs.filter((job) => job.state === JobState.Queued).length,
      completed: jobs.filter((job) => finishedStates.includes(job.state)).length,
    };
  }, [jobs]);

  const displayedJobs = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    const filtered = jobs.filter((job) => {
      if (view === 'settings') return false;
      if (view === 'attention' && !jobNeedsAttention(job)) return false;
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
    if (selectedJobId && !displayedJobs.some((job) => job.id === selectedJobId)) {
      setSelectedJobId(null);
    }
  }, [displayedJobs, selectedJobId, view]);

  const canPauseAny = jobs.some((job) => [JobState.Queued, JobState.Starting, JobState.Downloading].includes(job.state));
  const canResumeAny = jobs.some((job) => [JobState.Paused, JobState.Failed, JobState.Canceled].includes(job.state));
  const totalDownloadSpeed = jobs
    .filter((job) => job.state === JobState.Downloading)
    .reduce((total, job) => total + job.speed, 0);

  return (
    <div className="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
      <Titlebar>
        {view !== 'settings' ? (
          <CommandBar
            searchQuery={searchQuery}
            sortMode={sortMode}
            onSearchChange={setSearchQuery}
            onSortChange={setSortMode}
            onAdd={() => setIsAddModalOpen(true)}
            onResumeAll={() => void handleResumeAll()}
            onPauseAll={() => void handlePauseAll()}
            canResumeAll={canResumeAny}
            canPauseAll={canPauseAny}
            onCycleFilter={() => requestViewChange(nextFilterView(view))}
          />
        ) : null}
      </Titlebar>

      <div className="flex min-h-0 flex-1 overflow-hidden">
        <aside className="download-sidebar flex w-[252px] shrink-0 flex-col justify-between border-r border-border bg-sidebar px-3 py-3">
          <nav className="flex flex-col gap-1">
            <NavItem icon={<Download size={21} />} label="All Downloads" count={counts.all} active={view === 'all'} onClick={() => requestViewChange('all')} />
            <NavItem icon={<AlertTriangle size={21} />} label="Needs Attention" count={counts.attention} active={view === 'attention'} onClick={() => requestViewChange('attention')} />
            <NavItem icon={<Gauge size={21} />} label="Active" count={counts.active} active={view === 'active'} onClick={() => requestViewChange('active')} />
            <NavItem icon={<Clock3 size={21} />} label="Queued" count={counts.queued} active={view === 'queued'} onClick={() => requestViewChange('queued')} />
            <NavItem icon={<CheckCircle2 size={21} />} label="Completed" count={counts.completed} active={view === 'completed'} onClick={() => requestViewChange('completed')} />
          </nav>

          <div className="space-y-3">
            <div className="h-px bg-border" />
            <NavItem icon={<SettingsIcon size={21} />} label="Settings" active={view === 'settings'} onClick={() => requestViewChange('settings')} />
          </div>
        </aside>

        <main className="flex min-w-0 flex-1 flex-col overflow-hidden bg-surface">
          {view === 'settings' ? (
            <div className="min-h-0 flex-1 overflow-y-auto">
              <SettingsPage
                settings={settings}
                diagnostics={diagnostics}
                onSave={(newSettings) => handleSaveSettings(newSettings, 'all')}
                onBrowseDirectory={handleBrowseDirectory}
                onCancel={() => requestViewChange('all')}
                onDirtyChange={setSettingsDirty}
                onDraftChange={setSettingsDraft}
                onRefreshDiagnostics={refreshDiagnostics}
                onOpenInstallDocs={handleOpenInstallDocs}
                onRunHostRegistrationFix={handleRunHostRegistrationFix}
                onTestExtensionHandoff={handleTestExtensionHandoff}
                onCopyDiagnostics={handleCopyDiagnostics}
                onExportDiagnostics={handleExportDiagnostics}
              />
            </div>
          ) : (
            <>
              <QueueView
                jobs={displayedJobs}
                view={view}
                selectedJobId={selectedJobId}
                onSelect={setSelectedJobId}
                onClearSelection={() => setSelectedJobId(null)}
                onPause={handlePause}
                onResume={handleResume}
                onCancel={handleCancel}
                onRetry={handleRetry}
                onRestart={handleRestart}
                onRemove={handleRemove}
                onDelete={handleDelete}
                onRename={handleRename}
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

      {isAddModalOpen && (
        <AddDownloadModal
          onClose={() => setIsAddModalOpen(false)}
          onAdded={handleAddDownloadResult}
        />
      )}

      {isUnsavedSettingsPromptOpen && (
        <UnsavedSettingsPrompt
          isSaving={isSavingSettings}
          onDiscard={discardSettingsChanges}
          onSave={() => void saveSettingsAndLeave()}
        />
      )}
    </div>
  );
}

function UnsavedSettingsPrompt({
  isSaving,
  onDiscard,
  onSave,
}: {
  isSaving: boolean;
  onDiscard: () => void;
  onSave: () => void;
}) {
  return (
    <div className="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 px-4">
      <section
        role="dialog"
        aria-modal="true"
        aria-labelledby="unsaved-settings-title"
        className="w-full max-w-md rounded-md border border-border bg-card shadow-2xl"
      >
        <div className="border-b border-border bg-header px-5 py-4">
          <h2 id="unsaved-settings-title" className="text-base font-semibold text-foreground">
            Unsaved Settings
          </h2>
          <p className="mt-1 text-sm leading-5 text-muted-foreground">
            You changed application settings. Save them before leaving, or discard the draft.
          </p>
        </div>
        <div className="flex justify-end gap-2 px-5 py-4">
          <button
            type="button"
            onClick={onDiscard}
            disabled={isSaving}
            className="h-10 rounded-md border border-input bg-background px-4 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
          >
            Discard Changes
          </button>
          <button
            type="button"
            onClick={onSave}
            disabled={isSaving}
            className="h-10 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {isSaving ? 'Saving...' : 'Save Changes'}
          </button>
        </div>
      </section>
    </div>
  );
}

function CommandBar({
  searchQuery,
  sortMode,
  onSearchChange,
  onSortChange,
  onAdd,
  onResumeAll,
  onPauseAll,
  canResumeAll,
  canPauseAll,
  onCycleFilter,
}: {
  searchQuery: string;
  sortMode: SortMode;
  onSearchChange: (value: string) => void;
  onSortChange: (value: SortMode) => void;
  onAdd: () => void;
  onResumeAll: () => void;
  onPauseAll: () => void;
  canResumeAll: boolean;
  canPauseAll: boolean;
  onCycleFilter: () => void;
}) {
  return (
    <div className="command-bar flex h-full min-w-0 flex-1 items-center justify-between gap-3">
      <div className="flex min-w-0 shrink-0 items-center gap-1.5">
        <ToolbarButton icon={<Plus size={17} />} label="New Download" onClick={onAdd} strong />
        <div className="mx-1.5 h-5 w-px bg-border" />
        <ToolbarButton icon={<Play size={16} />} label="Resume All" onClick={onResumeAll} disabled={!canResumeAll} />
        <ToolbarButton icon={<Pause size={16} />} label="Pause All" onClick={onPauseAll} disabled={!canPauseAll} />
      </div>

      <div className="flex min-w-[320px] max-w-[620px] flex-1 items-center justify-end gap-2">
        <label className="relative min-w-0 flex-1">
          <Search size={16} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <input
            value={searchQuery}
            onChange={(event) => onSearchChange(event.target.value)}
            className="h-8 w-full rounded-md border border-input bg-background pl-9 pr-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
            placeholder="Search downloads..."
          />
        </label>
        <select
          value={sortMode}
          onChange={(event) => onSortChange(event.target.value as SortMode)}
          className="h-8 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
          aria-label="Sort downloads"
        >
          <option value="status">Sort by: Status</option>
          <option value="name">Sort by: Name</option>
          <option value="progress">Sort by: Progress</option>
          <option value="size">Sort by: Size</option>
        </select>
        <button
          onClick={onCycleFilter}
          className="flex h-8 w-8 items-center justify-center rounded-md border border-transparent text-muted-foreground transition hover:border-input hover:bg-muted hover:text-foreground"
          title="Cycle filter"
          aria-label="Cycle filter"
        >
          <Filter size={18} />
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
      className={`flex h-8 items-center gap-2 whitespace-nowrap rounded-md px-2.5 text-sm font-medium transition ${
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
      className={`group relative flex h-12 w-full items-center gap-3 rounded-md px-3.5 text-left text-sm font-medium transition ${
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
    <footer className="status-bar flex h-10 shrink-0 items-center justify-between border-t border-border bg-command px-6 text-xs text-muted-foreground">
      <div className="flex items-center gap-4">
        <span className="flex items-center gap-2">
          <Gauge size={16} className="text-primary" />
          {activeCount} active downloads
        </span>
        <span className="h-4 w-px bg-border" />
        <span className="flex items-center gap-2 text-foreground">
          <Download size={16} className="text-primary" />
          {formatBytes(downloadSpeed)}/s
        </span>
      </div>

      <div className="flex items-center gap-3">
        <span className={`flex items-center gap-2 ${isConnected ? 'text-foreground' : 'text-destructive'}`}>
          {isConnected ? <Wifi size={16} /> : <WifiOff size={16} />}
          {formatConnectionState(connectionState)}
        </span>
        <span className="text-muted-foreground">Slots: {connectionSlots}</span>
      </div>
    </footer>
  );
}

function nextFilterView(view: ViewState): ViewState {
  if (view === 'all') return 'attention';
  if (view === 'attention') return 'active';
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

function applyAccentColor(rawColor: string | undefined) {
  const accent = normalizeAccentColor(rawColor);
  const foreground = readableForegroundForHex(accent);
  const root = document.documentElement;

  root.style.setProperty('--color-primary', accent);
  root.style.setProperty('--color-ring', accent);
  root.style.setProperty('--color-primary-foreground', foreground);
  root.style.setProperty('--color-primary-soft', `color-mix(in oklch, ${accent} 20%, var(--color-background))`);
  root.style.setProperty('--color-accent', `color-mix(in oklch, ${accent} 20%, var(--color-background))`);
  root.style.setProperty('--color-accent-foreground', accent);
  root.style.setProperty('--color-selected', `color-mix(in oklch, ${accent} 24%, var(--color-background))`);
}

function normalizeAccentColor(rawColor: string | undefined) {
  const color = rawColor?.trim() ?? '';
  return /^#[0-9a-f]{6}$/i.test(color) ? color.toLowerCase() : DEFAULT_ACCENT_COLOR;
}

function readableForegroundForHex(hex: string) {
  const red = Number.parseInt(hex.slice(1, 3), 16);
  const green = Number.parseInt(hex.slice(3, 5), 16);
  const blue = Number.parseInt(hex.slice(5, 7), 16);
  const luminance = (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255;
  return luminance > 0.58 ? '#0a0f14' : '#ffffff';
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

function jobNeedsAttention(job: DownloadJob): boolean {
  if (job.state === JobState.Failed || job.failureCategory) return true;
  const isUnfinished = ![JobState.Completed, JobState.Canceled].includes(job.state);
  const hasPartialProgress = job.downloadedBytes > 0 || job.progress > 0;
  return isUnfinished && hasPartialProgress && job.resumeSupport === 'unsupported';
}

function formatDiagnosticsReport(diagnostics: DiagnosticsSnapshot): string {
  const lines = [
    'Simple Download Manager Diagnostics',
    `Connection State: ${diagnostics.connectionState}`,
    `Last Host Contact: ${diagnostics.lastHostContactSecondsAgo ?? 'never'} seconds ago`,
    `Queue Total: ${diagnostics.queueSummary.total}`,
    `Queue Active: ${diagnostics.queueSummary.active}`,
    `Queue Needs Attention: ${diagnostics.queueSummary.attention}`,
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
