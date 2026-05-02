<script lang="ts">
  import type { Component } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import {
    Box,
    Check,
    CheckCircle2,
    ChevronDown,
    ChevronRight,
    Download,
    FileArchive,
    FileAudio,
    FileImage,
    FilePlus,
    FileText,
    FileVideo,
    Folder,
    Gauge,
    Magnet,
    MoreHorizontal,
    Pause,
    Play,
    RotateCw,
    Search,
    Settings as SettingsIcon,
    Upload,
    Wifi,
    WifiOff,
  } from '@lucide/svelte';
  import { ConnectionState, JobState } from './types';
  import type { DownloadJob, QueueRowSize, Settings, ToastMessage } from './types';
  import QueueView from './QueueView.svelte';
  import SettingsPage, { SETTINGS_SECTIONS, type SettingsSectionId } from './SettingsPage.svelte';
  import ToastArea from './ToastArea.svelte';
  import AddDownloadModal from './AddDownloadModal.svelte';
  import type { AddDownloadOutcome } from './AddDownloadModal.svelte';
  import Titlebar from './Titlebar.svelte';
  import { compareDownloadsForSort, type SortMode } from './downloadSorting';
  import { DEFAULT_ACCENT_COLOR, applyAppearance } from './appearance';
  import { DOWNLOAD_CATEGORIES, type DownloadCategory } from './downloadCategories';
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
    recordProgressSamples,
    type ProgressSample,
  } from './downloadProgressMetrics';
  import { getErrorMessage } from './errors';
  import { loadInitialAppData } from './appBootstrap';
  import {
    browseDirectory,
    cancelJob,
    checkForUpdate,
    clearTorrentSessionCache,
    deleteJob,
    deleteJobs,
    exportDiagnosticsReport,
    getDiagnostics,
    getAppSnapshot,
    installUpdate,
    openBatchProgressWindow,
    openInstallDocs,
    openJobFile,
    openProgressWindow,
    pauseAllJobs,
    pauseJob,
    revealJobInFolder,
    renameJob,
    resumeAllJobs,
    resumeJob,
    restartJob,
    retryFailedJobs,
    retryJob,
    runHostRegistrationFix,
    saveSettings,
    subscribeToSelectedJobRequested,
    subscribeToStateChanged,
    subscribeToUpdateInstallProgress,
    swapFailedDownloadToBrowser,
    testExtensionHandoff,
  } from './backend';
  import type { AddJobsResult, DesktopSnapshot } from './backend';
  import { canRetryFailedDownloads } from './queueCommands';
  import {
    shouldNotifyDiagnosticsRefreshFailure,
    shouldRefreshDiagnostics,
    type DiagnosticsRefreshOptions,
  } from './diagnosticsRefresh';
  import { formatDiagnosticsReport } from './diagnosticsReport';
  import {
    applyInstallProgressEvent,
    beginUpdateInstall,
    failUpdateCheck,
    failUpdateInstall,
    finishUpdateCheck,
    initialAppUpdateState,
    shouldNotifyUpdateCheckFailure,
    shouldRunStartupUpdateCheck,
    startUpdateCheck,
    type AppUpdateMetadata,
    type AppUpdateState,
    type UpdateCheckMode,
  } from './appUpdates';
  import type { DiagnosticsSnapshot } from './types';
  import { formatBytes } from './popupShared';

  type IconComponent = Component<{ size?: number; class?: string; strokeWidth?: number }>;

  const DEFAULT_DOWNLOAD_DIRECTORY = 'C:\\Users\\You\\Downloads';
  const activeStates = [JobState.Starting, JobState.Downloading, JobState.Seeding, JobState.Paused];

  let connectionState = $state<ConnectionState>(ConnectionState.Checking);
  let jobs = $state<DownloadJob[]>([]);
  let settings = $state<Settings>(defaultSettings());
  let toasts = $state<ToastMessage[]>([]);
  let view = $state<ViewState>('all');
  let searchQuery = $state('');
  let sortMode = $state<SortMode>('date:asc');
  let isDownloadSectionExpanded = $state(true);
  let isTorrentSectionExpanded = $state(true);
  let selectedJobId = $state<string | null>(initialSelectedJobIdFromSearch(window.location.search));
  let isAddModalOpen = $state(false);
  let diagnostics = $state<DiagnosticsSnapshot | null>(null);
  let settingsDraft = $state<Settings | null>(null);
  let settingsDirty = $state(false);
  let pendingSettingsView = $state<ViewState | null>(null);
  let isUnsavedSettingsPromptOpen = $state(false);
  let isSavingSettings = $state(false);
  let activeSettingsSectionId = $state<SettingsSectionId>(SETTINGS_SECTIONS[0].id);
  let updateState = $state<AppUpdateState>(initialAppUpdateState);
  let isUpdatePromptOpen = $state(false);
  let commandMenuOpen = $state(false);
  let commandMenuRoot: HTMLDivElement | null = $state(null);

  let progressSamples: ProgressSample[] = [];
  let startupUpdateCheckStarted = false;
  let pendingVisibleSnapshot: DesktopSnapshot | null = null;
  let lastDiagnosticsRefreshAt = 0;

  const mainWindow = isTauriRuntime() ? getCurrentWindow() : null;
  const appearanceSettings = $derived(settingsDraft ?? settings);
  const counts = $derived(getQueueCounts(jobs));
  const torrentFooterStats = $derived(getTorrentFooterStats(jobs));
  const displayedJobs = $derived.by(() => {
    const filtered = filterJobsForView(jobs, view, searchQuery);
    return [...filtered].sort((a, b) => compareDownloadsForSort(a, b, sortMode));
  });
  const progressMetricsByJobId = $derived(calculateDownloadProgressMetricsByJobId(jobs, progressSamples));
  const canPauseAny = $derived(jobs.some((job) => [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding].includes(job.state)));
  const canResumeAny = $derived(jobs.some((job) => [JobState.Paused, JobState.Failed, JobState.Canceled].includes(job.state)));
  const canRetryFailed = $derived(canRetryFailedDownloads(jobs));
  const isTorrentStatusView = $derived(isTorrentView(view));
  const hasActiveTorrentJobs = $derived(jobs.some(
    (job) =>
      job.transferKind === 'torrent'
      && [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding].includes(job.state),
  ));
  const totalDownloadSpeed = $derived(jobs
    .filter((job) => job.state === JobState.Downloading)
    .reduce((total, job) => total + (progressMetricsByJobId[job.id]?.averageSpeed ?? job.speed), 0));

  $effect(() => {
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
          lastDiagnosticsRefreshAt = Date.now();
          diagnostics = initialData.diagnostics;
        } else if (initialData.diagnosticsError) {
          addToast({
            type: 'warning',
            title: 'Diagnostics Unavailable',
            message: getErrorMessage(initialData.diagnosticsError, 'Download state loaded, but diagnostics could not be refreshed.'),
          });
        }

        dispose = await subscribeToStateChanged((nextSnapshot) => {
          if (applyDesktopSnapshotWhenVisible(nextSnapshot)) {
            void refreshDiagnostics({ silent: true });
          }
        });

        if (shouldRunStartupUpdateCheck(startupUpdateCheckStarted)) {
          startupUpdateCheckStarted = true;
          void handleCheckForUpdates('startup');
        }
      } catch (error) {
        if (isMounted) {
          connectionState = ConnectionState.Error;
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
  });

  $effect(() => {
    let dispose: (() => void | Promise<void>) | undefined;

    async function subscribe() {
      dispose = await subscribeToUpdateInstallProgress((event) => {
        updateState = applyInstallProgressEvent(updateState, event);
      });
    }

    void subscribe();
    return () => {
      void dispose?.();
    };
  });

  $effect(() => {
    let dispose: (() => void | Promise<void>) | undefined;

    async function subscribe() {
      dispose = await subscribeToSelectedJobRequested((jobId) => {
        view = 'all';
        selectedJobId = jobId;
      });
    }

    void subscribe();
    return () => {
      void dispose?.();
    };
  });

  $effect(() => {
    const refresh = (allowBackendRefresh: boolean) => {
      const flushedPendingSnapshot = flushPendingVisibleSnapshot();
      if (!flushedPendingSnapshot && allowBackendRefresh) {
        void refreshSnapshotFromBackend();
      }
      void refreshDiagnostics({ silent: true });
    };
    const refreshOnFocus = () => refresh(true);
    const refreshWhenVisible = () => {
      if (document.visibilityState === 'visible') {
        refresh(false);
      }
    };

    window.addEventListener('focus', refreshOnFocus);
    document.addEventListener('visibilitychange', refreshWhenVisible);
    return () => {
      window.removeEventListener('focus', refreshOnFocus);
      document.removeEventListener('visibilitychange', refreshWhenVisible);
    };
  });

  $effect(() => {
    const nextAppearance = appearanceSettings;
    function applyTheme() {
      applyAppearance(nextAppearance);
    }

    applyTheme();
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    media.addEventListener('change', applyTheme);
    return () => media.removeEventListener('change', applyTheme);
  });

  $effect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'F11') return;
      if (!mainWindow) return;
      event.preventDefault();
      void mainWindow.toggleMaximize().catch(() => undefined);
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  });

  $effect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || view !== 'settings' || isUnsavedSettingsPromptOpen) return;
      event.preventDefault();
      requestViewChange('all');
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  });

  $effect(() => {
    if (!commandMenuOpen) return;

    const closeOnPointerDown = (event: PointerEvent) => {
      if (commandMenuRoot?.contains(event.target as Node)) return;
      commandMenuOpen = false;
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') commandMenuOpen = false;
    };

    document.addEventListener('pointerdown', closeOnPointerDown);
    document.addEventListener('keydown', closeOnEscape);
    return () => {
      document.removeEventListener('pointerdown', closeOnPointerDown);
      document.removeEventListener('keydown', closeOnEscape);
    };
  });

  $effect(() => {
    if (view === 'settings') return;
    if (connectionState === ConnectionState.Checking) return;
    if (displayedJobs.length === 0) {
      selectedJobId = null;
      return;
    }
    if (selectedJobId && !displayedJobs.some((job) => job.id === selectedJobId)) {
      selectedJobId = null;
    }
  });

  function defaultSettings(): Settings {
    return {
      downloadDirectory: DEFAULT_DOWNLOAD_DIRECTORY,
      maxConcurrentDownloads: 3,
      autoRetryAttempts: 3,
      speedLimitKibPerSecond: 0,
      downloadPerformanceMode: 'balanced',
      torrent: {
        enabled: true,
        downloadDirectory: `${DEFAULT_DOWNLOAD_DIRECTORY}\\Torrent`,
        seedMode: 'forever',
        seedRatioLimit: 1,
        seedTimeLimitMinutes: 60,
        uploadLimitKibPerSecond: 0,
        portForwardingEnabled: false,
        portForwardingPort: 42000,
        peerConnectionWatchdogMode: 'diagnose',
      },
      notificationsEnabled: true,
      theme: 'system',
      accentColor: DEFAULT_ACCENT_COLOR,
      showDetailsOnClick: true,
      queueRowSize: 'medium',
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
        authenticatedHandoffEnabled: true,
        authenticatedHandoffHosts: [],
      },
    };
  }

  function initialSelectedJobIdFromSearch(search: string): string | null {
    const selected = new URLSearchParams(search).get('selectJob')?.trim();
    return selected ? selected : null;
  }

  function requestViewChange(nextView: ViewState) {
    if (nextView === view) return;

    if (view === 'settings' && settingsDirty) {
      pendingSettingsView = nextView;
      isUnsavedSettingsPromptOpen = true;
      return;
    }

    view = nextView;
  }

  function applyDesktopSnapshot(snapshot: DesktopSnapshot) {
    progressSamples = recordProgressSamples(progressSamples, snapshot.jobs);
    connectionState = snapshot.connectionState;
    jobs = snapshot.jobs;
    settings = snapshot.settings;
  }

  function applyDesktopSnapshotWhenVisible(snapshot: DesktopSnapshot): boolean {
    if (document.visibilityState !== 'visible') {
      pendingVisibleSnapshot = snapshot;
      return false;
    }

    applyDesktopSnapshot(snapshot);
    return true;
  }

  function flushPendingVisibleSnapshot(): boolean {
    if (!pendingVisibleSnapshot) return false;
    const snapshot = pendingVisibleSnapshot;
    pendingVisibleSnapshot = null;
    applyDesktopSnapshot(snapshot);
    return true;
  }

  async function refreshSnapshotFromBackend() {
    try {
      applyDesktopSnapshot(await getAppSnapshot());
    } catch (error) {
      connectionState = ConnectionState.Error;
      addToast({
        type: 'error',
        title: 'Refresh Failed',
        message: getErrorMessage(error, 'Failed to refresh desktop state.'),
      });
    }
  }

  function addToast(toast: Omit<ToastMessage, 'id'>) {
    toasts = [...toasts, { ...toast, id: crypto.randomUUID() }];
  }

  function removeToast(id: string) {
    toasts = toasts.filter((toast) => toast.id !== id);
  }

  async function refreshDiagnostics(options: DiagnosticsRefreshOptions = {}) {
    const now = Date.now();
    if (!shouldRefreshDiagnostics(now, lastDiagnosticsRefreshAt, options)) {
      return;
    }

    try {
      diagnostics = await getDiagnostics();
      lastDiagnosticsRefreshAt = now;
    } catch (error) {
      if (shouldNotifyDiagnosticsRefreshFailure(options)) {
        addToast({ type: 'error', title: 'Diagnostics Failed', message: getErrorMessage(error) });
      }
    }
  }

  async function handleCheckForUpdates(mode: UpdateCheckMode = 'manual') {
    updateState = startUpdateCheck(updateState, mode);
    try {
      const update = await checkForUpdate();
      updateState = finishUpdateCheck(updateState, update);

      if (update) {
        isUpdatePromptOpen = true;
        addToast({
          type: 'info',
          title: 'Update Available',
          message: `Simple Download Manager ${update.version} is ready to install.`,
          autoClose: false,
        });
        return;
      }

      if (mode === 'manual') {
        addToast({ type: 'success', title: 'No Update Available', message: 'You are running the latest alpha build.' });
      }
    } catch (error) {
      const message = getErrorMessage(error, 'Could not check for updates.');
      updateState = failUpdateCheck(updateState, message);
      if (shouldNotifyUpdateCheckFailure(mode)) {
        addToast({ type: 'error', title: 'Update Check Failed', message });
      }
    }
  }

  async function handleInstallUpdate() {
    updateState = beginUpdateInstall(updateState);
    try {
      await installUpdate();
    } catch (error) {
      const message = getErrorMessage(error, 'Could not install the update.');
      updateState = failUpdateInstall(updateState, message);
      addToast({ type: 'error', title: 'Update Failed', message, autoClose: false });
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

  async function handleDelete(id: string, deleteFromDisk: boolean) {
    try {
      await deleteJob(id, deleteFromDisk);
      if (selectedJobId === id) selectedJobId = null;
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

  async function handleSwapFailedToBrowser(id: string) {
    try {
      await swapFailedDownloadToBrowser(id);
      addToast({ type: 'info', title: 'Swapped to Browser', message: 'The download URL was opened in your browser.' });
    } catch (error) {
      addToast({ type: 'error', title: 'Swap Failed', message: getErrorMessage(error) });
    }
  }

  async function handleDeleteMany(ids: string[], deleteFromDisk: boolean) {
    const uniqueIds = [...new Set(ids)].filter(Boolean);
    if (uniqueIds.length === 0) return;
    if (uniqueIds.length === 1) {
      await handleDelete(uniqueIds[0], deleteFromDisk);
      return;
    }

    try {
      await deleteJobs(uniqueIds, deleteFromDisk);
      if (selectedJobId && uniqueIds.includes(selectedJobId)) selectedJobId = null;
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
      const result = await openJobFile(id);
      if (result.pausedTorrent) {
        addToast({
          type: 'info',
          title: 'Torrent Paused',
          message: externalUseAutoReseedMessage('file', result.autoReseedRetrySeconds ?? 60),
        });
      }
    } catch (error) {
      addToast({ type: 'error', title: 'Open Failed', message: getErrorMessage(error) });
    }
  }

  async function handleReveal(id: string) {
    try {
      const result = await revealJobInFolder(id);
      if (result.pausedTorrent) {
        addToast({
          type: 'info',
          title: 'Torrent Paused',
          message: externalUseAutoReseedMessage('folder', result.autoReseedRetrySeconds ?? 60),
        });
      }
    } catch (error) {
      addToast({ type: 'error', title: 'Reveal Failed', message: getErrorMessage(error) });
    }
  }

  async function handleShowPopup(id: string) {
    try {
      await openProgressWindow(id);
    } catch (error) {
      addToast({
        type: 'warning',
        title: 'Progress Popup Failed',
        message: getErrorMessage(error, 'The progress popup could not be opened.'),
      });
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
    isSavingSettings = true;
    try {
      const savedSettings = await saveSettings(newSettings);
      settings = savedSettings;
      settingsDraft = null;
      settingsDirty = false;
      pendingSettingsView = null;
      isUnsavedSettingsPromptOpen = false;
      await refreshDiagnostics();
      view = nextView;
      addToast({ type: 'success', title: 'Settings Saved', message: 'Preferences updated successfully.' });
      return true;
    } catch (error) {
      addToast({ type: 'error', title: 'Save Failed', message: getErrorMessage(error) });
      return false;
    } finally {
      isSavingSettings = false;
    }
  }

  async function handleQueueRowSizeChange(queueRowSize: QueueRowSize) {
    if (settings.queueRowSize === queueRowSize) return;

    try {
      const savedSettings = await saveSettings({ ...settings, queueRowSize });
      settings = savedSettings;
      settingsDraft = null;
      settingsDirty = false;
      await refreshDiagnostics({ silent: true });
    } catch (error) {
      addToast({ type: 'error', title: 'Row Size Failed', message: getErrorMessage(error, 'Could not update queue row size.') });
    }
  }

  function discardSettingsChanges() {
    const nextView = pendingSettingsView ?? 'all';
    settingsDraft = null;
    settingsDirty = false;
    pendingSettingsView = null;
    isUnsavedSettingsPromptOpen = false;
    view = nextView;
  }

  async function saveSettingsAndLeave() {
    await handleSaveSettings(settingsDraft ?? settings, pendingSettingsView ?? 'all');
  }

  function handleAddDownloadResult(outcome: AddDownloadOutcome) {
    if (outcome.primaryResult) {
      selectedJobId = outcome.primaryResult.jobId;
    }

    void openProgressIntent(outcome.intent);

    if (outcome.mode === 'single' || outcome.mode === 'torrent') {
      const result = outcome.primaryResult;
      if (!result) return;
      if (result.status === 'duplicate_existing_job') {
        view = outcome.mode === 'torrent' ? 'torrents' : 'all';
        addToast({
          type: 'info',
          title: 'Already in Queue',
          message: `${result.filename} is already in the download list.`,
        });
        return;
      }

      view = outcome.mode === 'torrent' ? 'torrents' : 'all';
      addToast({
        type: 'success',
        title: outcome.mode === 'torrent' ? 'Torrent Added' : 'Download Added',
        message: `${result.filename} was added to the queue.`,
      });
      return;
    }

    const batchResult = outcome.result as AddJobsResult;
    if (batchResult.queuedCount === 0) {
      view = 'all';
      addToast({
        type: 'info',
        title: 'Already in Queue',
        message: `${batchResult.duplicateCount} ${batchResult.duplicateCount === 1 ? 'download is' : 'downloads are'} already in the list.`,
      });
      return;
    }

    view = 'all';
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
      return await browseDirectory();
    } catch (error) {
      addToast({ type: 'error', title: 'Browse Failed', message: getErrorMessage(error) });
      return null;
    }
  }

  async function handleClearTorrentSessionCache() {
    try {
      const result = await clearTorrentSessionCache();
      await refreshSnapshotFromBackend();
      addToast({
        type: result.pendingRestart ? 'warning' : 'success',
        title: result.pendingRestart ? 'Cache Clear Scheduled' : 'Cache Session Cleared',
        message: result.pendingRestart
          ? `Torrent session cache is locked and will be cleared on next startup: ${result.sessionPath}`
          : `Torrent session cache cleared: ${result.sessionPath}`,
      });
    } catch (error) {
      addToast({ type: 'error', title: 'Cache Clear Failed', message: getErrorMessage(error) });
    }
  }

  function setSettingsDirtyState(dirty: boolean, draft: Settings | null) {
    settingsDirty = dirty;
    settingsDraft = draft;
  }

  function runCommandMenuAction(action: () => void) {
    commandMenuOpen = false;
    action();
  }

  function categoryIcon(category: DownloadCategory): IconComponent {
    switch (category) {
      case 'document':
        return FileText;
      case 'program':
        return Box;
      case 'picture':
        return FileImage;
      case 'video':
        return FileVideo;
      case 'compressed':
        return FileArchive;
      case 'music':
        return FileAudio;
      default:
        return Folder;
    }
  }

  function externalUseAutoReseedMessage(target: 'file' | 'folder', retrySeconds: number): string {
    if (retrySeconds === 60) {
      return `Torrent paused so Windows can use the ${target}. The app will try to reseed every 60s while Windows is still using it.`;
    }

    return `Torrent paused so Windows can use the ${target}. The app will try to reseed every ${retrySeconds}s while Windows is still using it.`;
  }

  function connectionStatusPresentation(state: ConnectionState) {
    switch (state) {
      case ConnectionState.Connected:
        return { label: 'Connected', className: 'text-foreground', icon: Wifi };
      case ConnectionState.Checking:
        return { label: 'Checking', className: 'text-muted-foreground', icon: RotateCw };
      case ConnectionState.HostMissing:
      case ConnectionState.AppMissing:
      case ConnectionState.AppUnreachable:
      case ConnectionState.Error:
        return { label: formatConnectionState(state), className: 'text-destructive', icon: WifiOff };
    }
  }

  function formatTorrentStatusRatio(ratio: number) {
    if (!Number.isFinite(ratio) || ratio <= 0) return '--';
    return `${ratio.toFixed(2)}x`;
  }

  function formatConnectionState(state: ConnectionState) {
    return state.replaceAll('_', ' ').replace(/\b\w/g, (value) => value.toUpperCase());
  }

  function isTauriRuntime(): boolean {
    return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
  }

  const queueRowSizeOptions: { value: QueueRowSize; label: string }[] = [
    { value: 'compact', label: 'Compact' },
    { value: 'small', label: 'Small' },
    { value: 'medium', label: 'Medium' },
    { value: 'large', label: 'Large' },
    { value: 'damn', label: 'DAMN' },
  ];
</script>

<div class="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
  <Titlebar>
    {#if view !== 'settings'}
      <div class="command-bar flex h-full min-w-0 flex-1 items-center justify-between gap-3">
        <div class="flex min-w-0 shrink-0 items-center gap-1.5">
          {@render ToolbarButton(FilePlus, 'New Download', () => isAddModalOpen = true, false, true)}
          <div class="mx-1.5 h-5 w-px bg-border"></div>
          <div class="relative" bind:this={commandMenuRoot}>
            {@render ToolbarButton(MoreHorizontal, 'More', () => commandMenuOpen = !commandMenuOpen, false, false, 'Queue actions and row size')}

            {#if commandMenuOpen}
              <div
                class="absolute left-0 top-9 z-[70] w-56 overflow-hidden rounded-md border border-border bg-popover py-1 shadow-2xl"
                onclick={(event) => event.stopPropagation()}
                onkeydown={(event) => event.stopPropagation()}
                onpointerdown={(event) => event.stopPropagation()}
                role="menu"
                tabindex="-1"
                aria-label="Queue actions and row size"
              >
                {@render CommandMenuItem(Play, 'Resume All', () => runCommandMenuAction(() => void handleResumeAll()), !canResumeAny)}
                {@render CommandMenuItem(Pause, 'Pause All', () => runCommandMenuAction(() => void handlePauseAll()), !canPauseAny)}
                {@render CommandMenuItem(RotateCw, 'Retry Failed', () => runCommandMenuAction(() => void handleRetryFailedJobs()), !canRetryFailed)}
                <div class="my-1 h-px bg-border"></div>
                <div class="px-3 py-1 text-[10px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">Row Size</div>
                {#each queueRowSizeOptions as option (option.value)}
                  {@render CommandMenuItem(
                    settings.queueRowSize === option.value ? Check : undefined,
                    option.label,
                    () => runCommandMenuAction(() => void handleQueueRowSizeChange(option.value)),
                    false,
                    settings.queueRowSize === option.value,
                  )}
                {/each}
              </div>
            {/if}
          </div>
        </div>

        <div class="flex w-[310px] max-w-[42vw] shrink-0 items-center justify-end gap-2">
          <label class="relative w-full min-w-0">
            <Search size={16} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground" />
            <input
              value={searchQuery}
              oninput={(event) => searchQuery = event.currentTarget.value}
              class="h-8 w-full rounded-md border border-input bg-background pl-9 pr-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
              placeholder="Search downloads..."
            />
          </label>
        </div>
      </div>
    {/if}
  </Titlebar>

  <div class="flex min-h-0 flex-1 overflow-hidden">
    {#if view !== 'settings'}
      <aside class="download-sidebar flex w-[220px] shrink-0 flex-col overflow-hidden border-r border-border bg-sidebar px-2 py-2">
        <nav class="min-h-0 flex-1 overflow-y-auto overscroll-contain pr-1 flex flex-col gap-0.5">
          <div class="flex items-center gap-1">
            {@render SectionCollapseButton(
              isDownloadSectionExpanded,
              'Collapse downloads section',
              'Expand downloads section',
              () => isDownloadSectionExpanded = !isDownloadSectionExpanded,
            )}
            <div class="min-w-0 flex-1">
              {@render NavItem(Download, 'All Downloads', view === 'all', () => requestViewChange('all'), counts.all)}
            </div>
          </div>
          {#if isDownloadSectionExpanded}
            <div class="mb-1 ml-3 mt-0.5 border-l border-border/80 pl-2">
              {#each DOWNLOAD_CATEGORIES as category (category.id)}
                {@render NavItem(
                  categoryIcon(category.iconName),
                  category.label,
                  view === categoryView(category.id),
                  () => requestViewChange(categoryView(category.id)),
                  counts.categories[category.id],
                  true,
                  15,
                )}
              {/each}
            </div>
          {/if}
          {@render NavItem(Gauge, 'Active', view === 'active', () => requestViewChange('active'), counts.active)}
          {@render NavItem(CheckCircle2, 'Completed', view === 'completed', () => requestViewChange('completed'), counts.completed)}
          <div class="mt-2 border-t border-border/70 pt-2">
            <div class="px-3 pb-1 text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">Torrents</div>
            <div class="flex items-center gap-1">
              {@render SectionCollapseButton(
                isTorrentSectionExpanded,
                'Collapse torrents section',
                'Expand torrents section',
                () => isTorrentSectionExpanded = !isTorrentSectionExpanded,
              )}
              <div class="min-w-0 flex-1">
                {@render NavItem(Magnet, 'All Torrents', view === 'torrents', () => requestViewChange('torrents'), counts.torrents.all)}
              </div>
            </div>
            {#if isTorrentSectionExpanded}
              <div class="mb-1 ml-3 mt-0.5 border-l border-border/80 pl-2">
                {@render NavItem(Gauge, 'Active', view === 'torrent-active', () => requestViewChange('torrent-active'), counts.torrents.active, true, 15)}
                {@render NavItem(Upload, 'Seeding', view === 'torrent-seeding', () => requestViewChange('torrent-seeding'), counts.torrents.seeding, true, 15)}
                {@render NavItem(CheckCircle2, 'Completed', view === 'torrent-completed', () => requestViewChange('torrent-completed'), counts.torrents.completed, true, 15)}
              </div>
            {/if}
          </div>
        </nav>

        <div class="shrink-0 space-y-2">
          <div class="h-px bg-border"></div>
          {@render NavItem(SettingsIcon, 'Settings', false, () => requestViewChange('settings'))}
        </div>
      </aside>
    {/if}

    <main class="flex min-w-0 flex-1 flex-col overflow-hidden bg-surface">
      {#if view === 'settings'}
        <SettingsPage
          {settings}
          activeSectionId={activeSettingsSectionId}
          isSaving={isSavingSettings}
          onActiveSectionChange={(id) => activeSettingsSectionId = id}
          onSave={(newSettings) => void handleSaveSettings(newSettings, 'all')}
          onDirtyChange={setSettingsDirtyState}
          onBrowseDirectory={handleBrowseDirectory}
          onClearTorrentSessionCache={() => void handleClearTorrentSessionCache()}
        />
      {:else}
        <QueueView
          jobs={displayedJobs}
          {sortMode}
          showDetailsOnClick={settings.showDetailsOnClick}
          queueRowSize={settings.queueRowSize}
          onSortChange={(nextSortMode) => sortMode = nextSortMode}
          {selectedJobId}
          onSelectJob={(id) => selectedJobId = id}
          onPause={(id) => void handlePause(id)}
          onResume={(id) => void handleResume(id)}
          onCancel={(id) => void handleCancel(id)}
          onRetry={(id) => void handleRetry(id)}
          onRestart={(id) => void handleRestart(id)}
          onDelete={(ids, deleteFromDisk) => void handleDeleteMany(ids, deleteFromDisk)}
          onRename={(id, filename) => void handleRename(id, filename)}
          onOpen={(id) => void handleOpenFile(id)}
          onReveal={(id) => void handleReveal(id)}
          onShowPopup={(id) => void handleShowPopup(id)}
          onSwapFailedToBrowser={(id) => void handleSwapFailedToBrowser(id)}
        />

        {@render StatusBar(
          isTorrentStatusView ? 'torrents' : 'downloads',
          counts.active,
          totalDownloadSpeed,
          torrentFooterStats,
          connectionState,
          settings.maxConcurrentDownloads,
        )}
      {/if}
    </main>
  </div>

  <ToastArea {toasts} onRemove={removeToast} />

  {#if isAddModalOpen}
    <AddDownloadModal
      onClose={() => isAddModalOpen = false}
      onAdded={handleAddDownloadResult}
    />
  {/if}

  {#if isUnsavedSettingsPromptOpen}
    {@render UnsavedSettingsPrompt(isSavingSettings, discardSettingsChanges, () => void saveSettingsAndLeave())}
  {/if}

  {#if isUpdatePromptOpen && updateState.availableUpdate}
    {@render UpdateAvailablePrompt(
      updateState.availableUpdate,
      updateState,
      () => isUpdatePromptOpen = false,
      () => void handleInstallUpdate(),
    )}
  {/if}
</div>

{#snippet ToolbarButton(icon: IconComponent, label: string, onClick: () => void, disabled = false, strong = false, ariaLabel?: string)}
  {@const Icon = icon}
  <button
    type="button"
    onclick={onClick}
    {disabled}
    aria-label={ariaLabel}
    class={`flex h-8 items-center gap-2 whitespace-nowrap rounded-md px-2.5 text-sm font-medium transition ${
      strong
        ? 'border border-primary/60 bg-primary text-primary-foreground shadow-sm hover:bg-primary/90 active:bg-primary/80 focus-visible:ring-2 focus-visible:ring-primary/30'
        : 'border border-transparent text-muted-foreground hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-muted-foreground'
    }`}
  >
    <Icon size={17} strokeWidth={strong ? 2.4 : 2} />
    <span>{label}</span>
  </button>
{/snippet}

{#snippet CommandMenuItem(icon: IconComponent | undefined, label: string, onClick: () => void, disabled = false, active = false)}
  <button
    type="button"
    role="menuitem"
    onclick={onClick}
    {disabled}
    class={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm transition ${
      active ? 'text-primary' : 'text-foreground'
    } ${disabled ? 'cursor-not-allowed opacity-40' : 'hover:bg-muted'}`}
  >
    <span class="flex h-4 w-4 shrink-0 items-center justify-center">
      {#if icon}
        {@const Icon = icon}
        <Icon size={16} />
      {/if}
    </span>
    <span class="truncate">{label}</span>
  </button>
{/snippet}

{#snippet SectionCollapseButton(expanded: boolean, collapseLabel: string, expandLabel: string, onToggle: () => void)}
  <button
    type="button"
    aria-label={expanded ? collapseLabel : expandLabel}
    aria-expanded={expanded}
    onclick={onToggle}
    class="flex h-9 w-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition hover:bg-muted hover:text-foreground"
  >
    {#if expanded}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
  </button>
{/snippet}

{#snippet NavItem(icon: IconComponent, label: string, active: boolean, onClick: () => void, count?: number, branch = false, iconSize = 18)}
  {@const Icon = icon}
  <button
    onclick={onClick}
    class={`group relative flex w-full items-center gap-2 rounded-md text-left text-xs font-medium transition ${
      active ? 'bg-primary-soft text-primary shadow-[inset_3px_0_0_var(--color-primary)]' : 'text-foreground hover:bg-muted'
    } ${branch ? 'h-7 px-2 text-[11px]' : 'h-9 px-2.5'}`}
  >
    <span class="shrink-0"><Icon size={iconSize} /></span>
    <span class="min-w-0 flex-1 truncate">{label}</span>
    {#if typeof count === 'number'}
      <span class={`rounded-full px-2 py-0.5 text-[11px] leading-4 ${active ? 'bg-primary/10 text-primary' : 'bg-muted text-muted-foreground'}`}>
        {count}
      </span>
    {/if}
  </button>
{/snippet}

{#snippet StatusBar(mode: 'downloads' | 'torrents', activeCount: number, downloadSpeed: number, torrentStats: TorrentFooterStats, connectionState: ConnectionState, connectionSlots: number)}
  {@const connectionPresentation = connectionStatusPresentation(connectionState)}
  {@const ConnectionIcon = connectionPresentation.icon}
  <footer class="status-bar flex h-10 shrink-0 items-center justify-between border-t border-border bg-command px-6 text-xs text-muted-foreground">
    <div class="flex items-center gap-4">
      {#if mode === 'torrents'}
        <span class="flex items-center gap-2">
          <Magnet size={16} class="text-primary" />
          {torrentStats.all} torrents
        </span>
        <span class="h-4 w-px bg-border"></span>
        <span class="flex items-center gap-2 text-foreground">
          <Upload size={16} class="text-fuchsia-400" />
          {formatBytes(torrentStats.uploadedBytes)}
        </span>
        <span class="h-4 w-px bg-border"></span>
        <span class="flex items-center gap-2 text-foreground">
          <Download size={16} class="text-primary" />
          {formatBytes(torrentStats.downloadedBytes)}
        </span>
        <span class="h-4 w-px bg-border"></span>
        <span class="flex items-center gap-2 text-muted-foreground">
          Total Ratio {formatTorrentStatusRatio(torrentStats.totalRatio)}
        </span>
      {:else}
        <span class="flex items-center gap-2">
          <Gauge size={16} class="text-primary" />
          {activeCount} active downloads
        </span>
        <span class="h-4 w-px bg-border"></span>
        <span class="flex items-center gap-2 text-foreground">
          <Download size={16} class="text-primary" />
          {formatBytes(downloadSpeed)}/s
        </span>
      {/if}
    </div>

    <div class="flex items-center gap-3">
      <span class={`flex items-center gap-2 ${connectionPresentation.className}`}>
        <ConnectionIcon size={16} />
        {connectionPresentation.label}
      </span>
      <span class="text-muted-foreground">Slots: {connectionSlots}</span>
    </div>
  </footer>
{/snippet}

{#snippet UnsavedSettingsPrompt(isSaving: boolean, onDiscard: () => void, onSave: () => void)}
  <div class="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 px-4">
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="unsaved-settings-title"
      class="w-full max-w-md rounded-md border border-border bg-card shadow-2xl"
    >
      <div class="border-b border-border bg-header px-5 py-4">
        <h2 id="unsaved-settings-title" class="text-base font-semibold text-foreground">Unsaved Settings</h2>
        <p class="mt-1 text-sm leading-5 text-muted-foreground">You changed application settings. Save them before leaving, or discard the draft.</p>
      </div>
      <div class="flex justify-end gap-2 px-5 py-4">
        <button type="button" onclick={onDiscard} disabled={isSaving} class="h-10 rounded-md border border-input bg-background px-4 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50">Discard Changes</button>
        <button type="button" onclick={onSave} disabled={isSaving} class="h-10 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50">
          {isSaving ? 'Saving...' : 'Save Changes'}
        </button>
      </div>
    </div>
  </div>
{/snippet}

{#snippet UpdateAvailablePrompt(update: AppUpdateMetadata, updateState: AppUpdateState, onDismiss: () => void, onInstall: () => void)}
  {@const isInstalling = updateState.status === 'downloading' || updateState.status === 'installing'}
  <div class="fixed inset-0 z-[80] flex items-center justify-center bg-black/60 px-4">
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="update-available-title"
      class="w-full max-w-md rounded-md border border-border bg-card shadow-2xl"
    >
      <div class="border-b border-border bg-header px-5 py-4">
        <h2 id="update-available-title" class="text-base font-semibold text-foreground">Update Available</h2>
        <p class="mt-1 text-sm leading-5 text-muted-foreground">Simple Download Manager {update.version} is ready to install.</p>
      </div>
      {#if update.body}
        <div class="border-b border-border px-5 py-4 text-sm leading-6 text-muted-foreground">{update.body}</div>
      {/if}
      <div class="flex justify-end gap-2 px-5 py-4">
        <button type="button" onclick={onDismiss} disabled={isInstalling} class="h-10 rounded-md border border-input bg-background px-4 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50">Later</button>
        <button type="button" onclick={onInstall} disabled={isInstalling} class="h-10 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50">
          {isInstalling ? 'Installing...' : 'Install Update'}
        </button>
      </div>
    </div>
  </div>
{/snippet}
