import type { UnlistenFn } from '@tauri-apps/api/event';
import type { AppUpdateMetadata, UpdateInstallProgressEvent } from './appUpdates';
import type { ProgressBatchContext } from './batchProgress';
import { buildAddJobCommandArgs, type AddJobOptions } from './backendCommandArgs';
import type { AddJobResult, AddJobsResult, DesktopSnapshot, ExternalUseResult } from './backend';
import { canSwapFailedDownloadToBrowser } from './queueCommands';
import { ConnectionState, JobState, type DiagnosticsSnapshot, type DownloadJob, type DownloadPrompt, type Settings } from './types';

const PROGRESS_BATCH_STORAGE_PREFIX = 'sdm.progressBatch.';
const mockDownloadDirectory = 'C:\\Users\\You\\Downloads';
const mockNow = Date.now();

const defaultSettings: Settings = {
  downloadDirectory: mockDownloadDirectory,
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
    portForwardingEnabled: false,
    portForwardingPort: 42000,
  },
  notificationsEnabled: true,
  theme: 'system',
  accentColor: '#3b82f6',
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

let mockState: DesktopSnapshot = {
  connectionState: ConnectionState.Connected,
  jobs: [
    {
      id: '1',
      url: 'https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso',
      filename: 'Ubuntu 24.04 LTS Desktop (iso)',
      transferKind: 'http',
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
      id: '9',
      url: 'magnet:?xt=urn:btih:8f14e45fceea167a5a36dedd4bea2543deb12a91&dn=Debian%2012.5%20DVD%20Image',
      filename: 'Debian 12.5 DVD Image',
      transferKind: 'torrent',
      state: JobState.Downloading,
      createdAt: mockNow - 1000 * 60 * 28,
      progress: 74,
      totalBytes: 4705198080,
      downloadedBytes: 3481846579,
      speed: 6291456,
      eta: 194,
      targetPath: `${mockDownloadDirectory}\\Debian 12.5 DVD Image`,
      torrent: {
        infoHash: '8f14e45fceea167a5a36dedd4bea2543deb12a91',
        name: 'Debian 12.5 DVD Image',
        totalFiles: 4,
        peers: 28,
        seeds: 112,
        uploadedBytes: 483183820,
        ratio: 0.18,
      },
    },
    {
      id: '10',
      url: 'https://example.com/torrents/open-movie-archive.torrent',
      filename: 'Open Movie Archive',
      transferKind: 'torrent',
      state: JobState.Seeding,
      createdAt: mockNow - 1000 * 60 * 56,
      progress: 100,
      totalBytes: 2362232012,
      downloadedBytes: 2362232012,
      speed: 0,
      eta: 0,
      targetPath: `${mockDownloadDirectory}\\Open Movie Archive`,
      torrent: {
        infoHash: 'c9f0f895fb98ab9159f51fd0297e236d7f1234ab',
        name: 'Open Movie Archive',
        totalFiles: 12,
        peers: 9,
        seeds: 46,
        uploadedBytes: 3355443200,
        ratio: 1.42,
        seedingStartedAt: mockNow - 1000 * 60 * 18,
      },
    },
    {
      id: '8',
      url: 'https://example.com/broken-driver.exe',
      filename: 'driver-installer.exe',
      transferKind: 'http',
      source: {
        entryPoint: 'browser_download',
        browser: 'chrome',
        extensionVersion: '0.3.51',
      },
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
    },
  ],
  settings: defaultSettings,
};

const mockListeners = new Set<(snapshot: DesktopSnapshot) => void>();
const mockProgressBatchContexts = new Map<string, ProgressBatchContext>();

export async function getAppSnapshot(): Promise<DesktopSnapshot> {
  return cloneSnapshot(mockState);
}

export async function getDiagnostics(): Promise<DiagnosticsSnapshot> {
  return {
    connectionState: mockState.connectionState,
    lastHostContactSecondsAgo: 2,
    queueSummary: {
      total: mockState.jobs.length,
      active: mockState.jobs.filter((job) => [
        JobState.Downloading,
        JobState.Starting,
        JobState.Queued,
        JobState.Paused,
        JobState.Seeding,
      ].includes(job.state)).length,
      attention: mockState.jobs.filter(jobNeedsAttention).length,
      queued: mockState.jobs.filter((job) => job.state === JobState.Queued).length,
      downloading: mockState.jobs.filter((job) => [
        JobState.Downloading,
        JobState.Starting,
        JobState.Seeding,
      ].includes(job.state)).length,
      completed: mockState.jobs.filter((job) => [
        JobState.Completed,
        JobState.Canceled,
      ].includes(job.state)).length,
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
    recentEvents: [
      {
        timestamp: mockNow - 30_000,
        level: 'info',
        category: 'download',
        message: 'Mock diagnostics are available.',
      },
    ],
  };
}

export async function pauseJob(id: string): Promise<void> {
  updateJob(id, (job) => ({ ...job, state: JobState.Paused }));
}

export async function resumeJob(id: string): Promise<void> {
  updateJob(id, (job) => ({
    ...job,
    state: JobState.Queued,
    speed: 0,
    eta: 0,
    error: undefined,
    failureCategory: undefined,
    retryAttempts: 0,
  }));
}

export async function pauseAllJobs(): Promise<void> {
  const pausableStates = [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding];
  mockState.jobs = mockState.jobs.map((job) =>
    pausableStates.includes(job.state) ? { ...job, state: JobState.Paused, speed: 0, eta: 0 } : job,
  );
  emitMockState();
}

export async function resumeAllJobs(): Promise<void> {
  const resumableStates = [JobState.Paused, JobState.Failed, JobState.Canceled];
  mockState.jobs = mockState.jobs.map((job) =>
    resumableStates.includes(job.state)
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
}

export async function cancelJob(id: string): Promise<void> {
  updateJob(id, (job) => ({ ...job, state: JobState.Canceled }));
}

export async function retryJob(id: string): Promise<void> {
  updateJob(id, resetForRetry);
}

export async function restartJob(id: string): Promise<void> {
  updateJob(id, (job) => ({
    ...resetForRetry(job),
    totalBytes: 0,
    resumeSupport: 'unknown',
  }));
}

export async function retryFailedJobs(): Promise<void> {
  mockState.jobs = mockState.jobs.map((job) => job.state === JobState.Failed ? resetForRetry(job) : job);
  emitMockState();
}

export async function swapFailedDownloadToBrowser(id: string): Promise<void> {
  const job = mockState.jobs.find((candidate) => candidate.id === id);
  if (!job) throw new Error('Download was not found.');
  if (!canSwapFailedDownloadToBrowser(job)) {
    throw new Error('Only failed browser downloads can be swapped back to the browser.');
  }
  window.open(job.url, '_blank', 'noopener,noreferrer');
}

export async function removeJob(id: string): Promise<void> {
  mockState.jobs = mockState.jobs.filter((job) => job.id !== id);
  emitMockState();
}

export async function deleteJob(id: string, _deleteFromDisk: boolean): Promise<void> {
  await removeJob(id);
}

export async function deleteJobs(ids: string[], _deleteFromDisk: boolean): Promise<void> {
  const selectedIds = new Set(ids);
  mockState.jobs = mockState.jobs.filter((job) => !selectedIds.has(job.id));
  emitMockState();
}

export async function renameJob(id: string, filename: string): Promise<void> {
  updateJob(id, (job) => ({
    ...job,
    filename,
    targetPath: replacePathFilename(job.targetPath, filename),
    tempPath: job.tempPath ? replacePathFilename(job.tempPath, `${filename}.part`) : undefined,
  }));
}

export async function clearCompletedJobs(): Promise<void> {
  mockState.jobs = mockState.jobs.filter((job) => ![JobState.Completed, JobState.Canceled].includes(job.state));
  emitMockState();
}

export async function addJob(url: string, options?: AddJobOptions): Promise<AddJobResult> {
  const args = buildAddJobCommandArgs(url, options);
  const duplicateJob = mockState.jobs.find((job) => job.url === url);
  if (duplicateJob) {
    return {
      jobId: duplicateJob.id,
      filename: duplicateJob.filename,
      status: 'duplicate_existing_job',
    };
  }

  const jobId = createId('job');
  const filename = filenameFromUrl(url);
  mockState.jobs.push({
    id: jobId,
    url,
    filename,
    transferKind: args.transferKind ?? 'http',
    integrityCheck: args.expectedSha256
      ? { algorithm: 'sha256', expected: args.expectedSha256, status: 'pending' }
      : undefined,
    torrent: args.transferKind === 'torrent' ? { uploadedBytes: 0, ratio: 0 } : undefined,
    state: JobState.Queued,
    createdAt: Date.now(),
    progress: 0,
    totalBytes: 0,
    downloadedBytes: 0,
    speed: 0,
    eta: 0,
    targetPath: replacePathFilename(`${mockState.settings.downloadDirectory}\\download`, filename),
  });
  emitMockState();
  return { jobId, filename, status: 'queued' };
}

export async function addJobs(urls: string[], bulkArchiveName?: string): Promise<AddJobsResult> {
  const normalizedUrls = urls.map((url) => url.trim()).filter(Boolean);
  const bulkArchive = bulkArchiveName && normalizedUrls.length > 1
    ? { id: createId('bulk'), name: bulkArchiveName, archiveStatus: 'pending' as const }
    : undefined;
  const results: AddJobResult[] = [];

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

    const jobId = createId('job');
    const filename = filenameFromUrl(url);
    mockState.jobs.push({
      id: jobId,
      url,
      filename,
      transferKind: 'http',
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

export async function saveSettings(settings: Settings): Promise<Settings> {
  mockState.settings = structuredClone(settings);
  emitMockState();
  return structuredClone(mockState.settings);
}

export async function browseDirectory(): Promise<string | null> {
  return mockState.settings.downloadDirectory;
}

export async function browseTorrentFile(): Promise<string | null> {
  return 'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Imported%20Torrent';
}

export async function getCurrentDownloadPrompt(): Promise<DownloadPrompt | null> {
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

export async function confirmDownloadPrompt(
  _id: string,
  _directoryOverride: string | null,
  _options: unknown = {},
): Promise<void> {}

export async function showExistingDownloadPrompt(_id: string): Promise<void> {}
export async function swapDownloadPrompt(_id: string): Promise<void> {}
export async function cancelDownloadPrompt(_id: string): Promise<void> {}

export async function openProgressWindow(id: string): Promise<void> {
  window.open(popupUrl(`?window=download-progress&jobId=${encodeURIComponent(id)}`), `download-progress-${id}`, 'width=460,height=280');
}

export async function openBatchProgressWindow(context: ProgressBatchContext): Promise<string> {
  const batchId = context.batchId ?? createId('batch');
  const storedContext = { ...context, batchId };
  storeMockProgressBatchContext(storedContext);
  window.open(
    popupUrl(`?window=batch-progress&batchId=${encodeURIComponent(batchId)}`),
    `batch-progress-${batchId}`,
    'width=560,height=430',
  );
  return batchId;
}

export async function getProgressBatchContext(batchId: string): Promise<ProgressBatchContext | null> {
  return readMockProgressBatchContext(batchId);
}

export async function openJobFile(id: string): Promise<ExternalUseResult> {
  return prepareMockExternalUse(id);
}

export async function revealJobInFolder(id: string): Promise<ExternalUseResult> {
  return prepareMockExternalUse(id);
}

export async function openInstallDocs(): Promise<void> {}
export async function runHostRegistrationFix(): Promise<void> {}
export async function testExtensionHandoff(): Promise<void> {}

export async function exportDiagnosticsReport(): Promise<string | null> {
  return 'C:/Temp/simple-download-manager-diagnostics.json';
}

export async function checkForUpdate(): Promise<AppUpdateMetadata | null> {
  return null;
}

export async function installUpdate(): Promise<void> {}

export async function subscribeToStateChanged(
  listener: (snapshot: DesktopSnapshot) => void,
): Promise<UnlistenFn> {
  mockListeners.add(listener);
  listener(cloneSnapshot(mockState));
  return async () => {
    mockListeners.delete(listener);
  };
}

export async function subscribeToDownloadPromptChanged(
  _listener: (prompt: DownloadPrompt) => void,
): Promise<UnlistenFn> {
  return async () => undefined;
}

export async function subscribeToSelectedJobRequested(
  _listener: (jobId: string) => void,
): Promise<UnlistenFn> {
  return async () => undefined;
}

export async function subscribeToUpdateInstallProgress(
  _listener: (event: UpdateInstallProgressEvent) => void,
): Promise<UnlistenFn> {
  return async () => undefined;
}

function cloneSnapshot(snapshot: DesktopSnapshot): DesktopSnapshot {
  return structuredClone(snapshot);
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
  const hasPartialProgress = (job.downloadedBytes ?? 0) > 0 || job.progress > 0;
  return isUnfinished && hasPartialProgress && job.resumeSupport === 'unsupported';
}

function updateJob(id: string, update: (job: DownloadJob) => DownloadJob) {
  mockState.jobs = mockState.jobs.map((job) => (job.id === id ? update(job) : job));
  emitMockState();
}

function resetForRetry(job: DownloadJob): DownloadJob {
  return {
    ...job,
    state: JobState.Queued,
    progress: 0,
    downloadedBytes: 0,
    speed: 0,
    eta: 0,
    error: undefined,
    failureCategory: undefined,
    retryAttempts: 0,
  };
}

function replacePathFilename(path: string | undefined, filename: string): string {
  if (!path) return filename;
  const lastSlash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  if (lastSlash < 0) return filename;
  return `${path.slice(0, lastSlash + 1)}${filename}`;
}

function prepareMockExternalUse(id: string): ExternalUseResult {
  const job = mockState.jobs.find((candidate) => candidate.id === id);
  if (job?.transferKind === 'torrent' && job.state === JobState.Seeding) {
    job.state = JobState.Paused;
    job.speed = 0;
    job.eta = 0;
    emitMockState();
    return { pausedTorrent: true, autoReseedRetrySeconds: 60 };
  }

  return { pausedTorrent: false };
}

function filenameFromUrl(url: string): string {
  try {
    const parsed = new URL(url);
    if (parsed.protocol === 'magnet:') {
      return parsed.searchParams.get('dn')?.trim() || 'Torrent Download';
    }
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

function createId(prefix: string): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `${prefix}_${crypto.randomUUID()}`;
  }
  return `${prefix}_${Date.now()}_${Math.random().toString(36).slice(2)}`;
}
