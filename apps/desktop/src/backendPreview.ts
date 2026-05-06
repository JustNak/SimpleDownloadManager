import type { BulkOutputKind } from './bulkArchiveNaming';
import type { ProgressBatchContext } from './batchProgress';
import { createDefaultSettings, DEFAULT_DOWNLOAD_DIRECTORY } from './defaultSettings';
import { canSwapFailedDownloadToBrowser } from './queueCommands';
import { ConnectionState, JobState, type DiagnosticsSnapshot, type DownloadJob, type DownloadPrompt, type Settings, type TorrentSessionCacheClearResult, type TransferKind } from './types';
import type { AddJobResult, AddJobsResult, BatchProgressSnapshot, DesktopSnapshot, ExternalUseResult, ProgressJobSnapshot, SettingsSnapshot } from './backend';

type MockStateListener = (snapshot: DesktopSnapshot) => void;

interface AddJobCommandArgs {
  url: string;
  expectedSha256: string | null;
  transferKind?: TransferKind;
}

interface AddJobsCommandArgs {
  urls: string[];
  bulkArchiveName?: string;
  resolveHosterLinks?: boolean;
  startPaused?: boolean;
  bulkOutputKind?: BulkOutputKind;
}

const mockNow = Date.now();
const mockListeners = new Set<MockStateListener>();
const mockProgressBatchContexts = new Map<string, ProgressBatchContext>();

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
      targetPath: `${DEFAULT_DOWNLOAD_DIRECTORY}\\Ubuntu 24.04 LTS Desktop (iso).iso`,
    },
    {
      id: '2',
      url: 'https://cdn.example.com/The.Wild.Robot.2024.1080p.mkv',
      filename: 'The.Wild.Robot.2024.1080p.mkv',
      transferKind: 'http',
      state: JobState.Downloading,
      createdAt: mockNow - 1000 * 60 * 36,
      progress: 35,
      totalBytes: 4187593113,
      downloadedBytes: 1503238554,
      speed: 5452595,
      eta: 388,
      targetPath: `${DEFAULT_DOWNLOAD_DIRECTORY}\\The.Wild.Robot.2024.1080p.mkv`,
    },
    {
      id: '3',
      url: 'https://download.blender.org/release/Blender4.1/blender-4.1.1-windows-x64.msi',
      filename: 'Blender 4.1.1 Setup.exe',
      transferKind: 'http',
      state: JobState.Downloading,
      createdAt: mockNow - 1000 * 60 * 18,
      progress: 12,
      totalBytes: 884998144,
      downloadedBytes: 106199777,
      speed: 3250585,
      eta: 581,
      targetPath: `${DEFAULT_DOWNLOAD_DIRECTORY}\\Blender 4.1.1 Setup.exe`,
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
      targetPath: `${DEFAULT_DOWNLOAD_DIRECTORY}\\Debian 12.5 DVD Image`,
      torrent: {
        infoHash: '8f14e45fceea167a5a36dedd4bea2543deb12a91',
        name: 'Debian 12.5 DVD Image',
        totalFiles: 4,
        peers: 28,
        seeds: 112,
        uploadedBytes: 483183820,
        fetchedBytes: 3481846579,
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
      targetPath: `${DEFAULT_DOWNLOAD_DIRECTORY}\\Open Movie Archive`,
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
      id: '4',
      url: 'https://files.example.com/project-assets-2024.zip',
      filename: 'Project_Assets_2024.zip',
      transferKind: 'http',
      state: JobState.Queued,
      createdAt: mockNow - 1000 * 60 * 12,
      progress: 0,
      totalBytes: 1288490188,
      downloadedBytes: 0,
      speed: 0,
      eta: 0,
      targetPath: `${DEFAULT_DOWNLOAD_DIRECTORY}\\Project_Assets_2024.zip`,
    },
    {
      id: '5',
      url: 'https://docs.example.com/getting-started.pdf',
      filename: 'Getting_Started_Guide.pdf',
      transferKind: 'http',
      state: JobState.Completed,
      createdAt: mockNow - 1000 * 60 * 90,
      progress: 100,
      totalBytes: 13002342,
      downloadedBytes: 13002342,
      speed: 0,
      eta: 0,
      targetPath: `${DEFAULT_DOWNLOAD_DIRECTORY}\\Getting_Started_Guide.pdf`,
      artifactExists: true,
    },
    {
      id: '6',
      url: 'https://mirror.example.com/music-collection-flac.zip',
      filename: 'Music_Collection_FLAC.zip',
      transferKind: 'http',
      state: JobState.Completed,
      createdAt: mockNow - 1000 * 60 * 120,
      progress: 100,
      totalBytes: 2254857830,
      downloadedBytes: 2254857830,
      speed: 0,
      eta: 0,
      targetPath: `${DEFAULT_DOWNLOAD_DIRECTORY}\\Music_Collection_FLAC.zip`,
      artifactExists: false,
    },
    {
      id: '7',
      url: 'https://dl.fedoraproject.org/pub/fedora/linux/releases/40/Everything/x86_64/iso/Fedora-40-x86_64-DVD.iso',
      filename: 'Fedora-40-x86_64-DVD.iso',
      transferKind: 'http',
      state: JobState.Paused,
      createdAt: mockNow - 1000 * 60 * 72,
      progress: 58,
      totalBytes: 3865470566,
      downloadedBytes: 2241972928,
      speed: 0,
      eta: 135,
      resumeSupport: 'unsupported',
      targetPath: `${DEFAULT_DOWNLOAD_DIRECTORY}\\Fedora-40-x86_64-DVD.iso`,
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
      targetPath: `${DEFAULT_DOWNLOAD_DIRECTORY}\\driver-installer.exe`,
    }
  ],
  settings: createDefaultSettings(),
};

export function getMockAppSnapshot(): DesktopSnapshot {
  return cloneSnapshot(mockState);
}

export function getMockProgressJobSnapshot(id: string): ProgressJobSnapshot {
  return {
    job: cloneSnapshot(mockState).jobs.find((job) => job.id === id) ?? null,
    settings: { ...mockState.settings },
  };
}

export function getMockBatchProgressSnapshot(batchId: string): BatchProgressSnapshot {
  const context = readMockProgressBatchContext(batchId);
  const ids = new Set(context?.jobIds ?? []);
  return {
    context,
    jobs: cloneSnapshot(mockState).jobs.filter((job) => ids.has(job.id)),
    settings: { ...mockState.settings },
  };
}

export function getMockSettingsSnapshot(): SettingsSnapshot {
  return { settings: { ...mockState.settings } };
}

export function getMockDiagnostics(): DiagnosticsSnapshot {
  return {
    connectionState: mockState.connectionState,
    lastHostContactSecondsAgo: 2,
    queueSummary: {
      total: mockState.jobs.length,
      active: mockState.jobs.filter((job) => [JobState.Downloading, JobState.Starting, JobState.Queued, JobState.Paused, JobState.Seeding].includes(job.state)).length,
      attention: mockState.jobs.filter(jobNeedsAttention).length,
      queued: mockState.jobs.filter((job) => job.state === JobState.Queued).length,
      downloading: mockState.jobs.filter((job) => [JobState.Downloading, JobState.Starting, JobState.Seeding].includes(job.state)).length,
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

export function pauseMockJob(id: string): void {
  mockState.jobs = mockState.jobs.map((job) => (job.id === id ? { ...job, state: JobState.Paused } : job));
  emitMockState();
}

export function pauseMockJobs(ids: string[]): void {
  const selectedIds = new Set(ids);
  mockState.jobs = mockState.jobs.map((job) =>
    selectedIds.has(job.id) && [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding].includes(job.state)
      ? { ...job, state: JobState.Paused, speed: 0, eta: 0 }
      : job,
  );
  emitMockState();
}

export function resumeMockJob(id: string): void {
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
}

export function resumeMockJobs(ids: string[]): void {
  const selectedIds = new Set(ids);
  mockState.jobs = mockState.jobs.map((job) =>
    selectedIds.has(job.id) && [JobState.Paused, JobState.Failed, JobState.Canceled].includes(job.state)
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

export function pauseAllMockJobs(): void {
  mockState.jobs = mockState.jobs.map((job) =>
    [JobState.Queued, JobState.Starting, JobState.Downloading, JobState.Seeding].includes(job.state)
      ? { ...job, state: JobState.Paused, speed: 0, eta: 0 }
      : job,
  );
  emitMockState();
}

export function resumeAllMockJobs(): void {
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
}

export function cancelMockJob(id: string): void {
  mockState.jobs = mockState.jobs.map((job) => (job.id === id ? { ...job, state: JobState.Canceled } : job));
  emitMockState();
}

export function cancelMockJobs(ids: string[]): void {
  const selectedIds = new Set(ids);
  mockState.jobs = mockState.jobs.map((job) =>
    selectedIds.has(job.id) ? { ...job, state: JobState.Canceled } : job,
  );
  emitMockState();
}

export function retryMockJob(id: string): void {
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
}

export function restartMockJob(id: string): void {
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
}

export function retryFailedMockJobs(): void {
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
}

export function swapFailedMockDownloadToBrowser(id: string): void {
  const job = mockState.jobs.find((candidate) => candidate.id === id);
  if (!job) throw new Error('Download was not found.');
  if (!canSwapFailedDownloadToBrowser(job)) {
    throw new Error('Only failed browser downloads can be swapped back to the browser.');
  }
  window.open(job.url, '_blank', 'noopener,noreferrer');
}

export function removeMockJob(id: string): void {
  mockState.jobs = mockState.jobs.filter((job) => job.id !== id);
  emitMockState();
}

export function deleteMockJob(id: string): void {
  mockState.jobs = mockState.jobs.filter((job) => job.id !== id);
  emitMockState();
}

export function deleteMockJobs(ids: string[]): void {
  const selectedIds = new Set(ids);
  mockState.jobs = mockState.jobs.filter((job) => !selectedIds.has(job.id));
  emitMockState();
}

export function renameMockJob(id: string, filename: string): void {
  mockState.jobs = mockState.jobs.map((job) => {
    if (job.id !== id) return job;
    const targetPath = replacePathFilename(job.targetPath, filename);
    return {
      ...job,
      filename,
      targetPath,
      tempPath: job.tempPath ? replacePathFilename(job.tempPath, `${filename}.part`) : job.tempPath,
    };
  });
  emitMockState();
}

export function clearCompletedMockJobs(): void {
  mockState.jobs = mockState.jobs.filter((job) => ![JobState.Completed, JobState.Canceled].includes(job.state));
  emitMockState();
}

export function addMockJob(args: AddJobCommandArgs): AddJobResult {
  const duplicateJob = mockState.jobs.find((job) => job.url === args.url);
  if (duplicateJob) {
    return {
      jobId: duplicateJob.id,
      filename: duplicateJob.filename,
      status: 'duplicate_existing_job',
    };
  }

  const jobId = crypto.randomUUID();
  const filename = filenameFromUrl(args.url);
  mockState.jobs.push({
    id: jobId,
    url: args.url,
    filename,
    transferKind: args.transferKind ?? 'http',
    integrityCheck: args.expectedSha256
      ? {
          algorithm: 'sha256',
          expected: args.expectedSha256,
          status: 'pending',
        }
      : undefined,
    torrent: args.transferKind === 'torrent'
      ? {
          uploadedBytes: 0,
          ratio: 0,
        }
      : undefined,
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

export function addMockJobs(args: AddJobsCommandArgs): AddJobsResult {
  const results: AddJobResult[] = [];
  const bulkArchive = args.bulkArchiveName && args.urls.length > 1
    ? {
        id: crypto.randomUUID(),
        name: args.bulkArchiveName,
        outputKind: args.bulkOutputKind ?? 'archive',
        archiveStatus: 'pending' as const,
      }
    : undefined;

  for (const url of args.urls) {
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
      transferKind: 'http',
      state: args.startPaused ? JobState.Paused : JobState.Queued,
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
    failedItems: [],
  };
}

export function saveMockSettings(settings: Settings): Settings {
  mockState.settings = { ...settings };
  emitMockState();
  return mockState.settings;
}

export function browseMockDirectory(): string {
  return mockState.settings.downloadDirectory;
}

export function clearMockTorrentSessionCache(): TorrentSessionCacheClearResult {
  return {
    cleared: true,
    pendingRestart: false,
    sessionPath: 'C:\\Users\\You\\AppData\\Local\\SimpleDownloadManager\\torrent-session',
  };
}

export function browseMockTorrentFile(): string {
  return 'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Imported%20Torrent';
}

export function getMockCurrentDownloadPrompt(): DownloadPrompt {
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
    duplicatePath: undefined,
    duplicateFilename: undefined,
    duplicateReason: undefined,
  };
}

export function openMockProgressWindow(id: string): void {
  const job = mockState.jobs.find((candidate) => candidate.id === id);
  if (job && job.transferKind === 'torrent') {
    window.open(popupUrl(`?window=torrent-progress&jobId=${encodeURIComponent(id)}`), `torrent-progress-${id}`, 'width=720,height=520');
    return;
  }

  window.open(popupUrl(`?window=download-progress&jobId=${encodeURIComponent(id)}`), `download-progress-${id}`, 'width=460,height=280');
}

export function openMockBatchProgressWindow(context: ProgressBatchContext): string {
  const batchId = context.batchId ?? createBatchId();
  const storedContext = { ...context, batchId };
  storeMockProgressBatchContext(storedContext);
  window.open(
    popupUrl(`?window=batch-progress&batchId=${encodeURIComponent(batchId)}`),
    `batch-progress-${batchId}`,
    'width=640,height=480',
  );
  return batchId;
}

export function createStoredProgressBatchContext(context: ProgressBatchContext): ProgressBatchContext & { batchId: string } {
  const batchId = context.batchId ?? createBatchId();
  const storedContext = { ...context, batchId };
  storeMockProgressBatchContext(storedContext);
  return storedContext;
}

export function getMockProgressBatchContext(batchId: string): ProgressBatchContext | null {
  return readMockProgressBatchContext(batchId);
}

export function prepareMockExternalUse(id: string): ExternalUseResult {
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

export function exportMockDiagnosticsReport(): string {
  return 'C:/Temp/simple-download-manager-diagnostics.json';
}

export function subscribeMockStateChanged(listener: MockStateListener): () => void {
  mockListeners.add(listener);
  listener(cloneSnapshot(mockState));
  return () => {
    mockListeners.delete(listener);
  };
}

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

function replacePathFilename(path: string | undefined, filename: string): string {
  if (!path) return filename;
  const lastSlash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  if (lastSlash < 0) return filename;
  return `${path.slice(0, lastSlash + 1)}${filename}`;
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
    localStorage.setItem(`sdm.progressBatch.${context.batchId}`, JSON.stringify(context));
  } catch {
    // Local storage is best-effort in browser preview mode.
  }
}

function readMockProgressBatchContext(batchId: string): ProgressBatchContext | null {
  const inMemory = mockProgressBatchContexts.get(batchId);
  if (inMemory) return inMemory;
  try {
    const stored = localStorage.getItem(`sdm.progressBatch.${batchId}`);
    return stored ? JSON.parse(stored) as ProgressBatchContext : null;
  } catch {
    return null;
  }
}
