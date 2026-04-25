import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { ConnectionState, JobState, type DiagnosticsSnapshot, type DownloadJob, type DownloadPrompt, type Settings } from './types';
import type { ProgressBatchContext } from './batchProgress';

export interface DesktopSnapshot {
  connectionState: ConnectionState;
  jobs: DownloadJob[];
  settings: Settings;
}

export type AddJobStatus = 'queued' | 'duplicate_existing_job';

export interface AddJobResult {
  jobId: string;
  filename: string;
  status: AddJobStatus;
}

export interface AddJobsResult {
  results: AddJobResult[];
  queuedCount: number;
  duplicateCount: number;
}

const STATE_CHANGED_EVENT = 'app://state-changed';
const DOWNLOAD_PROMPT_CHANGED_EVENT = 'app://download-prompt-changed';
const SELECT_JOB_EVENT = 'app://select-job';
const PROGRESS_BATCH_STORAGE_PREFIX = 'sdm.progressBatch.';
const mockDownloadDirectory = 'C:\\Users\\You\\Downloads';
const mockNow = Date.now();

const defaultSettings: Settings = {
  downloadDirectory: mockDownloadDirectory,
  maxConcurrentDownloads: 3,
  autoRetryAttempts: 3,
  speedLimitKibPerSecond: 0,
  downloadPerformanceMode: 'balanced',
  notificationsEnabled: true,
  theme: 'system',
  accentColor: '#3b82f6',
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
};

let mockState: DesktopSnapshot = {
  connectionState: ConnectionState.Connected,
  jobs: [
    {
      id: '1',
      url: 'https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso',
      filename: 'Ubuntu 24.04 LTS Desktop (iso)',
      state: JobState.Downloading,
      createdAt: mockNow - 1000 * 60 * 48,
      progress: 68,
      totalBytes: 4105302224,
      downloadedBytes: 2792853504,
      speed: 8808038,
      eta: 72,
      targetPath: `${mockDownloadDirectory}\\Ubuntu 24.04 LTS Desktop (iso).iso`,
    },
    {
      id: '2',
      url: 'https://cdn.example.com/The.Wild.Robot.2024.1080p.mkv',
      filename: 'The.Wild.Robot.2024.1080p.mkv',
      state: JobState.Downloading,
      createdAt: mockNow - 1000 * 60 * 36,
      progress: 35,
      totalBytes: 4187593113,
      downloadedBytes: 1503238554,
      speed: 5452595,
      eta: 388,
      targetPath: `${mockDownloadDirectory}\\The.Wild.Robot.2024.1080p.mkv`,
    },
    {
      id: '3',
      url: 'https://download.blender.org/release/Blender4.1/blender-4.1.1-windows-x64.msi',
      filename: 'Blender 4.1.1 Setup.exe',
      state: JobState.Downloading,
      createdAt: mockNow - 1000 * 60 * 18,
      progress: 12,
      totalBytes: 884998144,
      downloadedBytes: 106199777,
      speed: 3250585,
      eta: 581,
      targetPath: `${mockDownloadDirectory}\\Blender 4.1.1 Setup.exe`,
    },
    {
      id: '4',
      url: 'https://files.example.com/project-assets-2024.zip',
      filename: 'Project_Assets_2024.zip',
      state: JobState.Queued,
      createdAt: mockNow - 1000 * 60 * 12,
      progress: 0,
      totalBytes: 1288490188,
      downloadedBytes: 0,
      speed: 0,
      eta: 0,
      targetPath: `${mockDownloadDirectory}\\Project_Assets_2024.zip`,
    },
    {
      id: '5',
      url: 'https://docs.example.com/getting-started.pdf',
      filename: 'Getting_Started_Guide.pdf',
      state: JobState.Completed,
      createdAt: mockNow - 1000 * 60 * 90,
      progress: 100,
      totalBytes: 13002342,
      downloadedBytes: 13002342,
      speed: 0,
      eta: 0,
      targetPath: `${mockDownloadDirectory}\\Getting_Started_Guide.pdf`,
      artifactExists: true,
    },
    {
      id: '6',
      url: 'https://mirror.example.com/music-collection-flac.zip',
      filename: 'Music_Collection_FLAC.zip',
      state: JobState.Completed,
      createdAt: mockNow - 1000 * 60 * 120,
      progress: 100,
      totalBytes: 2254857830,
      downloadedBytes: 2254857830,
      speed: 0,
      eta: 0,
      targetPath: `${mockDownloadDirectory}\\Music_Collection_FLAC.zip`,
      artifactExists: false,
    },
    {
      id: '7',
      url: 'https://dl.fedoraproject.org/pub/fedora/linux/releases/40/Everything/x86_64/iso/Fedora-40-x86_64-DVD.iso',
      filename: 'Fedora-40-x86_64-DVD.iso',
      state: JobState.Paused,
      createdAt: mockNow - 1000 * 60 * 72,
      progress: 58,
      totalBytes: 3865470566,
      downloadedBytes: 2241972928,
      speed: 0,
      eta: 135,
      resumeSupport: 'unsupported',
      targetPath: `${mockDownloadDirectory}\\Fedora-40-x86_64-DVD.iso`,
    },
    {
      id: '8',
      url: 'https://example.com/broken-driver.exe',
      filename: 'driver-installer.exe',
      state: JobState.Failed,
      createdAt: mockNow - 1000 * 60 * 24,
      progress: 22,
      totalBytes: 219152384,
      downloadedBytes: 48213524,
      speed: 0,
      eta: 0,
      error: 'The server closed the connection before the transfer completed.',
      failureCategory: 'network',
      retryAttempts: 3,
      targetPath: `${mockDownloadDirectory}\\driver-installer.exe`,
    }
  ],
  settings: defaultSettings,
};

const mockListeners = new Set<(snapshot: DesktopSnapshot) => void>();
const mockProgressBatchContexts = new Map<string, ProgressBatchContext>();

function cloneSnapshot(snapshot: DesktopSnapshot): DesktopSnapshot {
  return {
    connectionState: snapshot.connectionState,
    settings: { ...snapshot.settings },
    jobs: snapshot.jobs.map((job) => ({ ...job })),
  };
}

function emitMockState() {
  const snapshot = cloneSnapshot(mockState);
  for (const listener of mockListeners) {
    listener(snapshot);
  }
}

function jobNeedsAttention(job: DownloadJob): boolean {
  if (job.state === JobState.Failed || job.failureCategory) return true;
  const isUnfinished = ![JobState.Completed, JobState.Canceled].includes(job.state);
  const hasPartialProgress = job.downloadedBytes > 0 || job.progress > 0;
  return isUnfinished && hasPartialProgress && job.resumeSupport === 'unsupported';
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, args);
}

function replacePathFilename(path: string | undefined, filename: string): string {
  if (!path) return filename;
  const lastSlash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  if (lastSlash < 0) return filename;
  return `${path.slice(0, lastSlash + 1)}${filename}`;
}

function filenameFromUrl(url: string): string {
  try {
    const parsed = new URL(url);
    const segment = parsed.pathname.split('/').filter(Boolean).pop();
    return segment ? decodeURIComponent(segment) : 'download';
  } catch {
    const segment = url.split('/').pop() || 'download';
    try {
      return decodeURIComponent(segment);
    } catch {
      return segment;
    }
  }
}

function createBatchId() {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `batch_${crypto.randomUUID()}`;
  }
  return `batch_${Date.now()}_${Math.random().toString(36).slice(2)}`;
}

function popupUrl(path: string) {
  if (typeof window === 'undefined') return path;
  return `${window.location.origin}${window.location.pathname}${path}`;
}

function storeMockProgressBatchContext(context: ProgressBatchContext) {
  if (!context.batchId) return;
  mockProgressBatchContexts.set(context.batchId, context);
  try {
    localStorage.setItem(`${PROGRESS_BATCH_STORAGE_PREFIX}${context.batchId}`, JSON.stringify(context));
  } catch {
    // Local storage is best-effort in browser preview mode.
  }
}

function readMockProgressBatchContext(batchId: string): ProgressBatchContext | null {
  const inMemory = mockProgressBatchContexts.get(batchId);
  if (inMemory) return inMemory;
  try {
    const stored = localStorage.getItem(`${PROGRESS_BATCH_STORAGE_PREFIX}${batchId}`);
    return stored ? JSON.parse(stored) as ProgressBatchContext : null;
  } catch {
    return null;
  }
}

export async function getAppSnapshot(): Promise<DesktopSnapshot> {
  if (!isTauriRuntime()) return cloneSnapshot(mockState);
  return invokeCommand<DesktopSnapshot>('get_app_snapshot');
}

export async function getDiagnostics(): Promise<DiagnosticsSnapshot> {
  if (!isTauriRuntime()) {
    return {
      connectionState: mockState.connectionState,
      lastHostContactSecondsAgo: 2,
      queueSummary: {
        total: mockState.jobs.length,
        active: mockState.jobs.filter((job) => [JobState.Downloading, JobState.Starting, JobState.Queued, JobState.Paused].includes(job.state)).length,
        attention: mockState.jobs.filter(jobNeedsAttention).length,
        queued: mockState.jobs.filter((job) => job.state === JobState.Queued).length,
        downloading: mockState.jobs.filter((job) => [JobState.Downloading, JobState.Starting].includes(job.state)).length,
        completed: mockState.jobs.filter((job) => [JobState.Completed, JobState.Canceled].includes(job.state)).length,
        failed: mockState.jobs.filter((job) => job.state === JobState.Failed).length,
      },
      hostRegistration: {
        status: 'configured',
        entries: [
          {
            browser: 'Chrome',
            registryPath: 'HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.myapp.download_manager',
            manifestPath: 'C:\\Users\\You\\AppData\\Local\\Simple Download Manager\\native-messaging\\com.myapp.download_manager.chrome.json',
            manifestExists: true,
            hostBinaryPath: 'C:\\Users\\You\\AppData\\Local\\Simple Download Manager\\simple-download-manager-native-host.exe',
            hostBinaryExists: true,
          },
        ],
      },
    };
  }

  return invokeCommand<DiagnosticsSnapshot>('get_diagnostics');
}

export async function pauseJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.map((job) => (job.id === id ? { ...job, state: JobState.Paused } : job));
    emitMockState();
    return;
  }
  await invokeCommand('pause_job', { id });
}

export async function resumeJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.map((job) =>
      job.id === id
        ? {
            ...job,
            state: JobState.Queued,
            speed: 0,
            eta: 0,
            error: undefined,
            failureCategory: undefined,
            retryAttempts: 0,
          }
        : job,
    );
    emitMockState();
    return;
  }
  await invokeCommand('resume_job', { id });
}

export async function pauseAllJobs(): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.map((job) =>
      [JobState.Queued, JobState.Starting, JobState.Downloading].includes(job.state)
        ? { ...job, state: JobState.Paused, speed: 0, eta: 0 }
        : job,
    );
    emitMockState();
    return;
  }
  await invokeCommand('pause_all_jobs');
}

export async function resumeAllJobs(): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.map((job) =>
      [JobState.Paused, JobState.Failed, JobState.Canceled].includes(job.state)
        ? {
            ...job,
            state: JobState.Queued,
            speed: 0,
            eta: 0,
            error: undefined,
            failureCategory: undefined,
            retryAttempts: 0,
          }
        : job,
    );
    emitMockState();
    return;
  }
  await invokeCommand('resume_all_jobs');
}

export async function cancelJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.map((job) => (job.id === id ? { ...job, state: JobState.Canceled } : job));
    emitMockState();
    return;
  }
  await invokeCommand('cancel_job', { id });
}

export async function retryJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.map((job) =>
      job.id === id
        ? {
            ...job,
            state: JobState.Queued,
            progress: 0,
            downloadedBytes: 0,
            speed: 0,
            eta: 0,
            error: undefined,
            failureCategory: undefined,
            retryAttempts: 0,
          }
        : job
    );
    emitMockState();
    return;
  }
  await invokeCommand('retry_job', { id });
}

export async function restartJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.map((job) =>
      job.id === id
        ? {
            ...job,
            state: JobState.Queued,
            progress: 0,
            totalBytes: 0,
            downloadedBytes: 0,
            speed: 0,
            eta: 0,
            error: undefined,
            failureCategory: undefined,
            resumeSupport: 'unknown',
            retryAttempts: 0,
          }
        : job,
    );
    emitMockState();
    return;
  }
  await invokeCommand('restart_job', { id });
}

export async function retryFailedJobs(): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.map((job) =>
      job.state === JobState.Failed
        ? {
            ...job,
            state: JobState.Queued,
            progress: 0,
            downloadedBytes: 0,
            speed: 0,
            eta: 0,
            error: undefined,
            failureCategory: undefined,
            retryAttempts: 0,
          }
        : job,
    );
    emitMockState();
    return;
  }
  await invokeCommand('retry_failed_jobs');
}

export async function removeJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.filter((job) => job.id !== id);
    emitMockState();
    return;
  }
  await invokeCommand('remove_job', { id });
}

export async function deleteJob(id: string, deleteFromDisk: boolean): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.filter((job) => job.id !== id);
    emitMockState();
    return;
  }
  await invokeCommand('delete_job', { id, deleteFromDisk });
}

export async function deleteJobs(ids: string[], deleteFromDisk: boolean): Promise<void> {
  const uniqueIds = [...new Set(ids)].filter(Boolean);
  if (uniqueIds.length === 0) return;

  if (!isTauriRuntime()) {
    const selectedIds = new Set(uniqueIds);
    mockState.jobs = mockState.jobs.filter((job) => !selectedIds.has(job.id));
    emitMockState();
    return;
  }

  for (const id of uniqueIds) {
    await invokeCommand('delete_job', { id, deleteFromDisk });
  }
}

export async function renameJob(id: string, filename: string): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.map((job) => {
      if (job.id !== id) return job;
      const targetPath = replacePathFilename(job.targetPath, filename);
      return {
        ...job,
        filename,
        targetPath,
        tempPath: job.tempPath ? replacePathFilename(job.tempPath, `${filename}.part`) : undefined,
      };
    });
    emitMockState();
    return;
  }
  await invokeCommand('rename_job', { id, filename });
}

export async function clearCompletedJobs(): Promise<void> {
  if (!isTauriRuntime()) {
    mockState.jobs = mockState.jobs.filter((job) => ![JobState.Completed, JobState.Canceled].includes(job.state));
    emitMockState();
    return;
  }
  await invokeCommand('clear_completed_jobs');
}

export async function addJob(url: string): Promise<AddJobResult> {
  if (!isTauriRuntime()) {
    const duplicateJob = mockState.jobs.find((job) => job.url === url);
    if (duplicateJob) {
      return {
        jobId: duplicateJob.id,
        filename: duplicateJob.filename,
        status: 'duplicate_existing_job',
      };
    }

    const jobId = crypto.randomUUID();
    const filename = filenameFromUrl(url);
    mockState.jobs.push({
      id: jobId,
      url,
      filename,
      state: JobState.Queued,
      createdAt: Date.now(),
      progress: 0,
      totalBytes: 0,
      downloadedBytes: 0,
      speed: 0,
      eta: 0
    });
    emitMockState();
    return { jobId, filename, status: 'queued' };
  }
  return invokeCommand<AddJobResult>('add_job', { url });
}

export async function addJobs(urls: string[], bulkArchiveName?: string): Promise<AddJobsResult> {
  const normalizedUrls = urls.map((url) => url.trim()).filter(Boolean);
  if (normalizedUrls.length === 0) {
    throw new Error('Add at least one download URL.');
  }

  if (!isTauriRuntime()) {
    const results: AddJobResult[] = [];
    const bulkArchive = bulkArchiveName && normalizedUrls.length > 1
      ? { id: crypto.randomUUID(), name: bulkArchiveName, archiveStatus: 'pending' as const }
      : undefined;
    for (const url of normalizedUrls) {
      const duplicateJob = mockState.jobs.find((job) => job.url === url);
      if (duplicateJob) {
        results.push({
          jobId: duplicateJob.id,
          filename: duplicateJob.filename,
          status: 'duplicate_existing_job',
        });
        continue;
      }

      const jobId = crypto.randomUUID();
      const filename = filenameFromUrl(url);
      mockState.jobs.push({
        id: jobId,
        url,
        filename,
        state: JobState.Queued,
        createdAt: Date.now(),
        progress: 0,
        totalBytes: 0,
        downloadedBytes: 0,
        speed: 0,
        eta: 0,
        targetPath: replacePathFilename(`${mockState.settings.downloadDirectory}\\download`, filename),
        bulkArchive,
      });
      results.push({ jobId, filename, status: 'queued' });
    }
    emitMockState();
    const queuedCount = results.filter((result) => result.status === 'queued').length;
    return {
      results,
      queuedCount,
      duplicateCount: results.length - queuedCount,
    };
  }

  return invokeCommand<AddJobsResult>('add_jobs', {
    urls: normalizedUrls,
    bulkArchiveName: bulkArchiveName?.trim() || undefined,
  });
}

export async function saveSettings(settings: Settings): Promise<Settings> {
  if (!isTauriRuntime()) {
    mockState.settings = { ...settings };
    emitMockState();
    return mockState.settings;
  }
  return invokeCommand<Settings>('save_settings', { settings });
}

export async function browseDirectory(): Promise<string | null> {
  if (!isTauriRuntime()) return mockState.settings.downloadDirectory;
  return invokeCommand<string | null>('browse_directory');
}

export async function getCurrentDownloadPrompt(): Promise<DownloadPrompt | null> {
  if (!isTauriRuntime()) {
    return {
      id: 'mock_prompt',
      url: 'https://download.blender.org/release/Blender4.1/blender-4.1.1-windows-x64.msi',
      filename: 'Blender 4.1.1 Setup.exe',
      totalBytes: 884998144,
      defaultDirectory: mockState.settings.downloadDirectory,
      targetPath: `${mockState.settings.downloadDirectory}\\Blender 4.1.1 Setup.exe`,
      source: {
        entryPoint: 'browser_download',
        browser: 'chrome',
        extensionVersion: '0.1.0',
      },
      duplicateJob: undefined,
    };
  }
  return invokeCommand<DownloadPrompt | null>('get_current_download_prompt');
}

export async function confirmDownloadPrompt(
  id: string,
  directoryOverride: string | null,
  allowDuplicate: boolean,
): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('confirm_download_prompt', { id, directoryOverride, allowDuplicate });
}

export async function showExistingDownloadPrompt(id: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('show_existing_download_prompt', { id });
}

export async function cancelDownloadPrompt(id: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('cancel_download_prompt', { id });
}

export async function openProgressWindow(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    window.open(popupUrl(`?window=download-progress&jobId=${encodeURIComponent(id)}`), `download-progress-${id}`, 'width=500,height=360');
    return;
  }
  await invokeCommand('open_progress_window', { id });
}

export async function openBatchProgressWindow(context: ProgressBatchContext): Promise<string> {
  const batchId = context.batchId ?? createBatchId();
  const storedContext = { ...context, batchId };
  storeMockProgressBatchContext(storedContext);

  if (!isTauriRuntime()) {
    window.open(
      popupUrl(`?window=batch-progress&batchId=${encodeURIComponent(batchId)}`),
      `batch-progress-${batchId}`,
      'width=560,height=430',
    );
    return batchId;
  }

  await invokeCommand('open_batch_progress_window', { context: storedContext });
  return batchId;
}

export async function getProgressBatchContext(batchId: string): Promise<ProgressBatchContext | null> {
  if (!isTauriRuntime()) return readMockProgressBatchContext(batchId);
  return invokeCommand<ProgressBatchContext | null>('get_progress_batch_context', { batchId });
}

export async function openJobFile(id: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('open_job_file', { id });
}

export async function revealJobInFolder(id: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('reveal_job_in_folder', { id });
}

export async function openInstallDocs(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('open_install_docs');
}

export async function runHostRegistrationFix(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('run_host_registration_fix');
}

export async function testExtensionHandoff(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('test_extension_handoff');
}

export async function exportDiagnosticsReport(): Promise<string | null> {
  if (!isTauriRuntime()) return 'C:/Temp/simple-download-manager-diagnostics.json';
  return invokeCommand<string | null>('export_diagnostics_report');
}

export async function subscribeToStateChanged(
  listener: (snapshot: DesktopSnapshot) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    mockListeners.add(listener);
    listener(cloneSnapshot(mockState));
    return async () => mockListeners.delete(listener);
  }
  return listen<DesktopSnapshot>(STATE_CHANGED_EVENT, (event) => listener(event.payload));
}

export async function subscribeToDownloadPromptChanged(
  listener: (prompt: DownloadPrompt) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return async () => undefined;
  return listen<DownloadPrompt>(DOWNLOAD_PROMPT_CHANGED_EVENT, (event) => listener(event.payload));
}

export async function subscribeToSelectedJobRequested(
  listener: (jobId: string) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return async () => undefined;
  return listen<string>(SELECT_JOB_EVENT, (event) => listener(event.payload));
}
