import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ConnectionState, JobState } from './types';
import type { DownloadJob, Settings, ToastMessage } from './types';
import { QueueView } from './QueueView';
import { SettingsPage } from './SettingsPage';
import { ToastArea } from './ToastArea';
import { AddDownloadModal, type AddDownloadOutcome } from './AddDownloadModal';
import { Titlebar } from './Titlebar';
import { compareDownloadsForSort, nextSortModeForColumn, type SortMode } from './downloadSorting';
import { DEFAULT_ACCENT_COLOR, applyAppearance } from './appearance';
import {
  DOWNLOAD_CATEGORIES,
  type DownloadCategory,
} from './downloadCategories';
import {
  categoryView,
  filterJobsForView,
  getQueueCounts,
  getTorrentFooterStats,
  isTorrentView,
  type TorrentFooterStats,
  type ViewState,
} from './downloadViews';
import {
  calculateDownloadProgressMetricsByJobId,
  recordProgressSample,
  type ProgressSample,
} from './downloadProgressMetrics';
import { getErrorMessage } from './errors';
import { loadInitialAppData } from './appBootstrap';
import {
  browseDirectory,
  cancelJob,
  deleteJob,
  deleteJobs,
  exportDiagnosticsReport,
  getDiagnostics,
  getAppSnapshot,
  openBatchProgressWindow,
  openInstallDocs,
  openJobFile,
  openProgressWindow,
  pauseAllJobs,
  pauseJob,
  revealJobInFolder,
  removeJob,
  renameJob,
  resumeAllJobs,
  resumeJob,
  restartJob,
  retryFailedJobs,
  retryJob,
  runHostRegistrationFix,
  saveSettings,
  subscribeToStateChanged,
  subscribeToSelectedJobRequested,
  testExtensionHandoff,
} from './backend';
import type { AddJobsResult } from './backend';
import {
  Box,
  ChevronDown,
  ChevronRight,
  CheckCircle2,
  Download,
  FileArchive,
  FileAudio,
  FileImage,
  FileText,
  FileVideo,
  Filter,
  Folder,
  Gauge,
  Magnet,
  Pause,
  Play,
  Plus,
  RotateCw,
  Search,
  Settings as SettingsIcon,
  Upload,
  Wifi,
  WifiOff,
} from 'lucide-react';
import type { DiagnosticsSnapshot } from './types';
import type { DesktopSnapshot } from './backend';
import { canRetryFailedDownloads } from './queueCommands';
import {
  shouldNotifyDiagnosticsRefreshFailure,
  type DiagnosticsRefreshOptions,
} from './diagnosticsRefresh';
import { formatDiagnosticsReport } from './diagnosticsReport';

const DEFAULT_DOWNLOAD_DIRECTORY = 'C:\\Users\\You\\Downloads';
const activeStates = [JobState.Starting, JobState.Downloading, JobState.Seeding, JobState.Paused];

export default function App() {
  const [connectionState, setConnectionState] = useState<ConnectionState>(ConnectionState.Checking);
  const [jobs, setJobs] = useState<DownloadJob[]>([]);
  const [settings, setSettings] = useState<Settings>({
    downloadDirectory: DEFAULT_DOWNLOAD_DIRECTORY,
    maxConcurrentDownloads: 3,
    autoRetryAttempts: 3,
    speedLimitKibPerSecond: 0,
    downloadPerformanceMode: 'balanced',
    torrent: {
      enabled: true,
      seedMode: 'forever',
      seedRatioLimit: 1,
      seedTimeLimitMinutes: 60,
      uploadLimitKibPerSecond: 0,
    },
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
  const [sortMode, setSortMode] = useState<SortMode>('date:desc');
  const [isDownloadSectionExpanded, setIsDownloadSectionExpanded] = useState(true);
  const [isTorrentSectionExpanded, setIsTorrentSectionExpanded] = useState(true);
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsSnapshot | null>(null);
  const [settingsDraft, setSettingsDraft] = useState<Settings | null>(null);
  const [settingsDirty, setSettingsDirty] = useState(false);
  const [pendingSettingsView, setPendingSettingsView] = useState<ViewState | null>(null);
  const [isUnsavedSettingsPromptOpen, setIsUnsavedSettingsPromptOpen] = useState(false);
  const [isSavingSettings, setIsSavingSettings] = useState(false);
  const progressSamplesRef = useRef<ProgressSample[]>([]);
  const appearanceSettings = settingsDraft ?? settings;

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
          void refreshDiagnostics({ silent: true });
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
      applyAppearance(appearanceSettings);
    }

    applyTheme();
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    media.addEventListener('change', applyTheme);
    return () => media.removeEventListener('change', applyTheme);
  }, [appearanceSettings.accentColor, appearanceSettings.theme]);

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
    progressSamplesRef.current = snapshot.jobs.reduce(
      (samples, job) => recordProgressSample(samples, job),
      progressSamplesRef.current,
    );
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

  const removeToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((toast) => toast.id !== id));
  }, []);

  async function refreshDiagnostics(options: DiagnosticsRefreshOptions = {}) {
    try {
      setDiagnostics(await getDiagnostics());
    } catch (error) {
      if (shouldNotifyDiagnosticsRefreshFailure(options)) {
        addToast({ type: 'error', title: 'Diagnostics Failed', message: getErrorMessage(error) });
      }
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

  async function handleRetryFailedJobs() {
    try {
      await retryFailedJobs();
      addToast({ type: 'info', title: 'Retrying Failed Downloads', message: 'Failed downloads were added back to the queue.' });
    } catch (error) {
      addToast({ type: 'error', title: 'Retry Failed Downloads Failed', message: getErrorMessage(error) });
    }
  }

  async function handleDeleteMany(ids: string[], deleteFromDisk: boolean) {
    const uniqueIds = [...new Set(ids)].filter(Boolean);
    if (uniqueIds.length === 0) return;

    try {
      await deleteJobs(uniqueIds, deleteFromDisk);
      if (selectedJobId && uniqueIds.includes(selectedJobId)) setSelectedJobId(null);
      addToast({
        type: 'success',
        title: 'Downloads Deleted',
        message: deleteFromDisk
          ? `Removed ${uniqueIds.length} downloads from the list and deleted their files from disk.`
          : `Removed ${uniqueIds.length} downloads from the download list.`,
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

  function handleAddDownloadResult(outcome: AddDownloadOutcome) {
    if (outcome.primaryResult) {
      setSelectedJobId(outcome.primaryResult.jobId);
    }

    void openProgressIntent(outcome.intent);

    if (outcome.mode === 'single' || outcome.mode === 'torrent') {
      const result = outcome.primaryResult;
      if (!result) return;
      if (result.status === 'duplicate_existing_job') {
        setView(outcome.mode === 'torrent' ? 'torrents' : 'all');
        addToast({
          type: 'info',
          title: 'Already in Queue',
          message: `${result.filename} is already in the download list.`,
        });
        return;
      }

      setView(outcome.mode === 'torrent' ? 'torrents' : 'all');
      addToast({
        type: 'success',
        title: outcome.mode === 'torrent' ? 'Torrent Added' : 'Download Added',
        message: `${result.filename} was added to the queue.`,
      });
      return;
    }

    const batchResult = outcome.result as AddJobsResult;
    if (batchResult.queuedCount === 0) {
      setView('all');
      addToast({
        type: 'info',
        title: 'Already in Queue',
        message: `${batchResult.duplicateCount} ${batchResult.duplicateCount === 1 ? 'download is' : 'downloads are'} already in the list.`,
      });
      return;
    }

    setView('all');
    addToast({
      type: 'success',
      title: outcome.mode === 'bulk' ? 'Bulk Download Added' : 'Downloads Added',
      message: `${batchResult.queuedCount} ${batchResult.queuedCount === 1 ? 'download was' : 'downloads were'} added to the queue.`,
    });
  }

  async function openProgressIntent(intent: AddDownloadOutcome['intent']) {
    if (!intent) return;
    try {
      if (intent.type === 'single') {
        await openProgressWindow(intent.jobId);
      } else {
        await openBatchProgressWindow(intent.context);
      }
    } catch (error) {
      addToast({
        type: 'warning',
        title: 'Progress Popup Failed',
        message: getErrorMessage(error, 'The download was queued, but the progress popup could not be opened.'),
      });
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
    return getQueueCounts(jobs);
  }, [jobs]);

  const torrentFooterStats = useMemo(() => {
    return getTorrentFooterStats(jobs);
  }, [jobs]);

  const displayedJobs = useMemo(() => {
    const filtered = filterJobsForView(jobs, view, searchQuery);

    return [...filtered].sort((a, b) => compareDownloadsForSort(a, b, sortMode));
  }, [jobs, searchQuery, sortMode, view]);

  const progressMetricsByJobId = useMemo(
    () => calculateDownloadProgressMetricsByJobId(jobs, progressSamplesRef.current),
    [jobs],
  );

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

  const canPauseAny = jobs.some((job) => [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding].includes(job.state));
  const canResumeAny = jobs.some((job) => [JobState.Paused, JobState.Failed, JobState.Canceled].includes(job.state));
  const canRetryFailed = canRetryFailedDownloads(jobs);
  const isTorrentStatusView = isTorrentView(view);
  const totalDownloadSpeed = jobs
    .filter((job) => job.state === JobState.Downloading)
    .reduce((total, job) => total + (progressMetricsByJobId[job.id]?.averageSpeed ?? job.speed), 0);

  return (
    <div className="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
      <Titlebar>
        {view !== 'settings' ? (
          <CommandBar
            searchQuery={searchQuery}
            onSearchChange={setSearchQuery}
            onAdd={() => setIsAddModalOpen(true)}
            onResumeAll={() => void handleResumeAll()}
            onPauseAll={() => void handlePauseAll()}
            onRetryFailed={() => void handleRetryFailedJobs()}
            canResumeAll={canResumeAny}
            canPauseAll={canPauseAny}
            canRetryFailed={canRetryFailed}
            onCycleFilter={() => requestViewChange(nextFilterView(view))}
          />
        ) : null}
      </Titlebar>

      <div className="flex min-h-0 flex-1 overflow-hidden">
        <aside className="download-sidebar flex w-[220px] shrink-0 flex-col overflow-hidden border-r border-border bg-sidebar px-2 py-2">
          <nav className="min-h-0 flex-1 overflow-y-auto overscroll-contain pr-1 flex flex-col gap-0.5">
            <div className="flex items-center gap-1">
              <SectionCollapseButton
                expanded={isDownloadSectionExpanded}
                collapseLabel="Collapse downloads section"
                expandLabel="Expand downloads section"
                onToggle={() => setIsDownloadSectionExpanded((expanded) => !expanded)}
              />
              <div className="min-w-0 flex-1">
                <NavItem icon={<Download size={18} />} label="All Downloads" count={counts.all} active={view === 'all'} onClick={() => requestViewChange('all')} />
              </div>
            </div>
            {isDownloadSectionExpanded ? (
              <>
                <div className="mb-1 ml-3 mt-0.5 border-l border-border/80 pl-2">
                  {DOWNLOAD_CATEGORIES.map((category) => (
                    <NavItem
                      key={category.id}
                      icon={categoryIcon(category.iconName, 15)}
                      label={category.label}
                      count={counts.categories[category.id]}
                      active={view === categoryView(category.id)}
                      onClick={() => requestViewChange(categoryView(category.id))}
                      branch
                    />
                  ))}
                </div>
                <NavItem icon={<Gauge size={18} />} label="Active" count={counts.active} active={view === 'active'} onClick={() => requestViewChange('active')} />
                <NavItem icon={<CheckCircle2 size={18} />} label="Completed" count={counts.completed} active={view === 'completed'} onClick={() => requestViewChange('completed')} />
              </>
            ) : null}
            <div className="mt-2 border-t border-border/70 pt-2">
              <div className="px-3 pb-1 text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">
                Torrents
              </div>
              <div className="flex items-center gap-1">
                <SectionCollapseButton
                  expanded={isTorrentSectionExpanded}
                  collapseLabel="Collapse torrents section"
                  expandLabel="Expand torrents section"
                  onToggle={() => setIsTorrentSectionExpanded((expanded) => !expanded)}
                />
                <div className="min-w-0 flex-1">
                  <NavItem icon={<Magnet size={18} />} label="All Torrents" count={counts.torrents.all} active={view === 'torrents'} onClick={() => requestViewChange('torrents')} />
                </div>
              </div>
              {isTorrentSectionExpanded ? (
                <div className="mb-1 ml-3 mt-0.5 border-l border-border/80 pl-2">
                  <NavItem icon={<Gauge size={15} />} label="Active" count={counts.torrents.active} active={view === 'torrent-active'} onClick={() => requestViewChange('torrent-active')} branch />
                  <NavItem icon={<Upload size={15} />} label="Seeding" count={counts.torrents.seeding} active={view === 'torrent-seeding'} onClick={() => requestViewChange('torrent-seeding')} branch />
                  <NavItem icon={<CheckCircle2 size={15} />} label="Completed" count={counts.torrents.completed} active={view === 'torrent-completed'} onClick={() => requestViewChange('torrent-completed')} branch />
                </div>
              ) : null}
            </div>
          </nav>

          <div className="shrink-0 space-y-2">
            <div className="h-px bg-border" />
            <NavItem icon={<SettingsIcon size={18} />} label="Settings" active={view === 'settings'} onClick={() => requestViewChange('settings')} />
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
                sortMode={sortMode}
                onSortChange={(column) => setSortMode((current) => nextSortModeForColumn(current, column))}
                progressMetricsByJobId={progressMetricsByJobId}
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
                onDeleteMany={handleDeleteMany}
                onRename={handleRename}
                onOpen={handleOpenFile}
                onReveal={handleReveal}
              />

              <StatusBar
                mode={isTorrentStatusView ? 'torrents' : 'downloads'}
                activeCount={counts.active}
                downloadSpeed={totalDownloadSpeed}
                torrentStats={torrentFooterStats}
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
  onSearchChange,
  onAdd,
  onResumeAll,
  onPauseAll,
  onRetryFailed,
  canResumeAll,
  canPauseAll,
  canRetryFailed,
  onCycleFilter,
}: {
  searchQuery: string;
  onSearchChange: (value: string) => void;
  onAdd: () => void;
  onResumeAll: () => void;
  onPauseAll: () => void;
  onRetryFailed: () => void;
  canResumeAll: boolean;
  canPauseAll: boolean;
  canRetryFailed: boolean;
  onCycleFilter: () => void;
}) {
  return (
    <div className="command-bar flex h-full min-w-0 flex-1 items-center justify-between gap-3">
      <div className="flex min-w-0 shrink-0 items-center gap-1.5">
        <ToolbarButton icon={<Plus size={17} />} label="New Download" onClick={onAdd} strong />
        <div className="mx-1.5 h-5 w-px bg-border" />
        <ToolbarButton icon={<Play size={16} />} label="Resume All" onClick={onResumeAll} disabled={!canResumeAll} />
        <ToolbarButton icon={<Pause size={16} />} label="Pause All" onClick={onPauseAll} disabled={!canPauseAll} />
        <ToolbarButton icon={<RotateCw size={16} />} label="Retry Failed" onClick={onRetryFailed} disabled={!canRetryFailed} />
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

function SectionCollapseButton({
  expanded,
  collapseLabel,
  expandLabel,
  onToggle,
}: {
  expanded: boolean;
  collapseLabel: string;
  expandLabel: string;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={expanded ? collapseLabel : expandLabel}
      aria-expanded={expanded}
      onClick={onToggle}
      className="flex h-9 w-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition hover:bg-muted hover:text-foreground"
    >
      {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
    </button>
  );
}

function NavItem({
  icon,
  label,
  count,
  active,
  onClick,
  branch = false,
}: {
  icon: React.ReactNode;
  label: string;
  count?: number;
  active: boolean;
  onClick: () => void;
  branch?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      className={`group relative flex w-full items-center gap-2 rounded-md text-left text-xs font-medium transition ${
        active ? 'bg-primary-soft text-primary shadow-[inset_3px_0_0_var(--color-primary)]' : 'text-foreground hover:bg-muted'
      } ${branch ? 'h-7 px-2 text-[11px]' : 'h-9 px-2.5'}`}
    >
      <span className="shrink-0">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {typeof count === 'number' ? (
        <span className={`rounded-full px-2 py-0.5 text-[11px] leading-4 ${active ? 'bg-primary/10 text-primary' : 'bg-muted text-muted-foreground'}`}>
          {count}
        </span>
      ) : null}
    </button>
  );
}

function StatusBar({
  mode,
  activeCount,
  downloadSpeed,
  torrentStats,
  connectionState,
  connectionSlots,
}: {
  mode: 'downloads' | 'torrents';
  activeCount: number;
  downloadSpeed: number;
  torrentStats: TorrentFooterStats;
  connectionState: ConnectionState;
  connectionSlots: number;
}) {
  const isConnected = connectionState === ConnectionState.Connected;
  const seedDisplay = torrentStats.seedSpeed > 0
    ? `${formatBytes(torrentStats.seedSpeed)}/s`
    : `Up ${formatBytes(torrentStats.uploadedBytes)}`;

  return (
    <footer className="status-bar flex h-10 shrink-0 items-center justify-between border-t border-border bg-command px-6 text-xs text-muted-foreground">
      <div className="flex items-center gap-4">
        {mode === 'torrents' ? (
          <>
            <span className="flex items-center gap-2">
              <Magnet size={16} className="text-primary" />
              {torrentStats.all} torrents
            </span>
            <span className="h-4 w-px bg-border" />
            <span className="flex items-center gap-2 text-foreground">
              <Upload size={16} className="text-fuchsia-400" />
              Seed {seedDisplay}
            </span>
            <span className="h-4 w-px bg-border" />
            <span className="flex items-center gap-2 text-muted-foreground">
              Ratio {formatTorrentStatusRatio(torrentStats.averageRatio)}
            </span>
          </>
        ) : (
          <>
            <span className="flex items-center gap-2">
              <Gauge size={16} className="text-primary" />
              {activeCount} active downloads
            </span>
            <span className="h-4 w-px bg-border" />
            <span className="flex items-center gap-2 text-foreground">
              <Download size={16} className="text-primary" />
              {formatBytes(downloadSpeed)}/s
            </span>
          </>
        )}
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
  if (view === 'torrents') return 'torrent-active';
  if (view === 'torrent-active') return 'torrent-seeding';
  if (view === 'torrent-seeding') return 'torrent-completed';
  if (view === 'torrent-completed') return 'torrents';
  if (view === 'all') return 'active';
  if (view === 'active') return 'completed';
  return 'all';
}

function categoryIcon(category: DownloadCategory, size: number): React.ReactNode {
  switch (category) {
    case 'document':
      return <FileText size={size} />;
    case 'program':
      return <Box size={size} />;
    case 'picture':
      return <FileImage size={size} />;
    case 'video':
      return <FileVideo size={size} />;
    case 'compressed':
      return <FileArchive size={size} />;
    case 'music':
      return <FileAudio size={size} />;
    default:
      return <Folder size={size} />;
  }
}

function formatBytes(bytes: number, decimals = 1) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(decimals))} ${sizes[i]}`;
}

function formatTorrentStatusRatio(ratio: number) {
  if (!Number.isFinite(ratio) || ratio <= 0) return '--';
  return `${ratio.toFixed(2)}x`;
}

function formatConnectionState(state: ConnectionState) {
  return state.replaceAll('_', ' ').replace(/\b\w/g, (value) => value.toUpperCase());
}

