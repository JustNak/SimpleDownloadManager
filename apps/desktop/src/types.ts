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
  | 'internal';

export type ResumeSupport = 'unknown' | 'supported' | 'unsupported';

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
  state: JobState;
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

export interface Settings {
  downloadDirectory: string;
  maxConcurrentDownloads: number;
  autoRetryAttempts: number;
  speedLimitKibPerSecond: number;
  notificationsEnabled: boolean;
  theme: 'light' | 'dark' | 'system';
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

export interface DiagnosticsSnapshot {
  connectionState: ConnectionState;
  queueSummary: QueueSummary;
  lastHostContactSecondsAgo?: number;
  hostRegistration: HostRegistrationDiagnostics;
}

export interface ToastMessage {
  id: string;
  type: 'info' | 'success' | 'warning' | 'error';
  title: string;
  message: string;
  autoClose?: boolean;
}
