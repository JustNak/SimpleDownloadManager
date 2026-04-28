export enum ConnectionState {
  Checking = 'checking',
  Connected = 'connected',
  HostMissing = 'host_missing',
  AppMissing = 'app_missing',
  AppUnreachable = 'app_unreachable',
  Error = 'error',
}

export enum JobState {
  Queued = 'queued',
  Starting = 'starting',
  Downloading = 'downloading',
  Seeding = 'seeding',
  Paused = 'paused',
  Completed = 'completed',
  Failed = 'failed',
  Canceled = 'canceled',
}

export type FailureCategory =
  | 'network'
  | 'http'
  | 'server'
  | 'disk'
  | 'permission'
  | 'resume'
  | 'integrity'
  | 'torrent'
  | 'internal';

export type ResumeSupport = 'unknown' | 'supported' | 'unsupported';
export type TransferKind = 'http' | 'torrent';
export type IntegrityAlgorithm = 'sha256';
export type IntegrityStatus = 'pending' | 'verified' | 'failed';
export type DownloadHandoffMode = 'off' | 'ask' | 'auto';
export type StartupLaunchMode = 'open' | 'tray';
export type BulkArchiveStatus = 'pending' | 'compressing' | 'completed' | 'failed';
export type DownloadPerformanceMode = 'stable' | 'balanced' | 'fast';
export type TorrentSeedMode = 'forever' | 'ratio' | 'time' | 'ratio_or_time';
export type QueueRowSize = 'compact' | 'small' | 'medium' | 'large' | 'damn';

export interface DownloadSource {
  entryPoint: string;
  browser: string;
  extensionVersion: string;
  pageUrl?: string;
  pageTitle?: string;
  referrer?: string;
  incognito?: boolean;
}

export interface DownloadJob {
  id: string;
  url: string;
  filename: string;
  source?: DownloadSource;
  transferKind: TransferKind;
  integrityCheck?: {
    algorithm: IntegrityAlgorithm;
    expected: string;
    actual?: string;
    status: IntegrityStatus;
  };
  torrent?: {
    infoHash?: string;
    engineId?: number;
    name?: string;
    totalFiles?: number;
    peers?: number;
    seeds?: number;
    uploadedBytes: number;
    lastRuntimeUploadedBytes?: number;
    fetchedBytes?: number;
    lastRuntimeFetchedBytes?: number;
    ratio: number;
    seedingStartedAt?: number;
  };
  state: JobState;
  createdAt?: number;
  progress: number; // 0-100
  totalBytes: number;
  downloadedBytes: number;
  speed: number; // bytes per second
  eta: number; // seconds remaining
  error?: string;
  failureCategory?: FailureCategory;
  resumeSupport?: ResumeSupport;
  retryAttempts?: number;
  targetPath?: string;
  tempPath?: string;
  artifactExists?: boolean;
  bulkArchive?: {
    id: string;
    name: string;
    archiveStatus?: BulkArchiveStatus;
    outputPath?: string;
    error?: string;
  };
}

export interface DownloadPrompt {
  id: string;
  url: string;
  filename: string;
  source?: DownloadSource;
  totalBytes?: number;
  defaultDirectory: string;
  targetPath: string;
  duplicateJob?: DownloadJob;
}

export interface ExtensionIntegrationSettings {
  enabled: boolean;
  downloadHandoffMode: DownloadHandoffMode;
  listenPort: number;
  contextMenuEnabled: boolean;
  showProgressAfterHandoff: boolean;
  showBadgeStatus: boolean;
  excludedHosts: string[];
  ignoredFileExtensions: string[];
  authenticatedHandoffEnabled: boolean;
  authenticatedHandoffHosts: string[];
}

export interface TorrentSettings {
  enabled: boolean;
  seedMode: TorrentSeedMode;
  seedRatioLimit: number;
  seedTimeLimitMinutes: number;
  uploadLimitKibPerSecond: number;
  portForwardingEnabled: boolean;
  portForwardingPort: number;
}

export interface Settings {
  downloadDirectory: string;
  maxConcurrentDownloads: number;
  autoRetryAttempts: number;
  speedLimitKibPerSecond: number;
  downloadPerformanceMode: DownloadPerformanceMode;
  torrent: TorrentSettings;
  notificationsEnabled: boolean;
  theme: 'light' | 'dark' | 'oled_dark' | 'system';
  accentColor: string;
  showDetailsOnClick: boolean;
  queueRowSize: QueueRowSize;
  startOnStartup: boolean;
  startupLaunchMode: StartupLaunchMode;
  extensionIntegration: ExtensionIntegrationSettings;
}

export interface QueueSummary {
  total: number;
  active: number;
  attention: number;
  queued: number;
  downloading: number;
  completed: number;
  failed: number;
}

export type HostRegistrationStatus = 'configured' | 'missing' | 'broken';

export interface HostRegistrationEntry {
  browser: string;
  registryPath: string;
  manifestPath?: string;
  manifestExists: boolean;
  hostBinaryPath?: string;
  hostBinaryExists: boolean;
}

export interface HostRegistrationDiagnostics {
  status: HostRegistrationStatus;
  entries: HostRegistrationEntry[];
}

export type DiagnosticLevel = 'info' | 'warning' | 'error';

export interface DiagnosticEvent {
  timestamp: number;
  level: DiagnosticLevel;
  category: string;
  message: string;
  jobId?: string;
}

export interface DiagnosticsSnapshot {
  connectionState: ConnectionState;
  queueSummary: QueueSummary;
  lastHostContactSecondsAgo?: number;
  hostRegistration: HostRegistrationDiagnostics;
  recentEvents: DiagnosticEvent[];
}

export interface ToastMessage {
  id: string;
  type: 'info' | 'success' | 'warning' | 'error';
  title: string;
  message: string;
  autoClose?: boolean;
}
