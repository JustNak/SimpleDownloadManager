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

export type RemovalState = 'removing' | 'cleanup_failed';

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
export type TransferKind = 'http' | 'torrent' | 'browser_adopted' | 'browser_blob';
export type IntegrityAlgorithm = 'sha256';
export type IntegrityStatus = 'pending' | 'verified' | 'failed';
export type DownloadHandoffMode = 'off' | 'ask' | 'auto';
export type StartupLaunchMode = 'open' | 'tray';
export type BulkOutputKind = 'archive' | 'folder';
export type BulkFinalizeMode = 'move' | 'extract' | 'zip';
export type BulkArchiveStatus = 'pending' | 'extracting' | 'combining' | 'creating_folder' | 'compressing' | 'completed' | 'failed';
export type BulkStartBehavior = 'review_then_start' | 'start_immediately';
export type BulkHosterFairnessMode = 'adaptive' | 'safe' | 'off';
export type BulkHosterAccelerationMode = 'safe' | 'off';
export type HosterPreflightStatus = 'unchecked' | 'checking' | 'ready' | 'failed';
export type DownloadPerformanceMode = 'stable' | 'balanced' | 'fast';
export type TorrentSeedMode = 'forever' | 'ratio' | 'time' | 'ratio_or_time';
export type TorrentPeerConnectionWatchdogMode = 'assist' | 'diagnose' | 'recover';
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
    diagnostics?: TorrentRuntimeDiagnostics;
  };
  state: JobState;
  removalState?: RemovalState;
  createdAt?: number;
  progress: number; // 0-100
  totalBytes: number;
  downloadedBytes: number;
  speed: number; // bytes per second
  eta: number; // seconds remaining
  activeSegments?: number;
  plannedSegments?: number;
  error?: string;
  failureCategory?: FailureCategory;
  resumeSupport?: ResumeSupport;
  retryAttempts?: number;
  autoRestartAttempts?: number;
  resolvedFromUrl?: string;
  hosterPreflight?: {
    status: HosterPreflightStatus;
    message?: string;
  };
  targetPath?: string;
  tempPath?: string;
  artifactExists?: boolean;
  bulkArchive?: {
    id: string;
    name: string;
    outputKind?: BulkOutputKind;
    archiveStatus?: BulkArchiveStatus;
    requiresExtraction?: boolean;
    outputPath?: string;
    error?: string;
    warning?: string;
    finalizeTotalBytes?: number;
    finalizeProcessedBytes?: number;
    finalizeMode?: BulkFinalizeMode;
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
  duplicatePath?: string;
  duplicateFilename?: string;
  duplicateReason?: 'url' | 'path' | 'file' | 'partial_file';
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
  capturedFileExtensions: string[];
  downloadCaptureDebugLogging: boolean;
}

export interface TorrentSettings {
  enabled: boolean;
  downloadDirectory: string;
  seedMode: TorrentSeedMode;
  seedRatioLimit: number;
  seedTimeLimitMinutes: number;
  uploadLimitKibPerSecond: number;
  portForwardingEnabled: boolean;
  portForwardingPort: number;
  peerConnectionWatchdogMode: TorrentPeerConnectionWatchdogMode;
  customTrackers: string[];
}

export interface BulkDownloadSettings {
  outputDirectory: string;
  maxConcurrentDownloads: number;
  speedLimitKibPerSecond: number;
  downloadPerformanceMode: DownloadPerformanceMode;
  hosterFairnessMode: BulkHosterFairnessMode;
  hosterAccelerationMode: BulkHosterAccelerationMode;
  autoRetryOverrideEnabled: boolean;
  autoRetryAttempts: number;
  startBehavior: BulkStartBehavior;
  expandActiveRowsByDefault: boolean;
}

export interface TorrentSessionCacheClearResult {
  cleared: boolean;
  pendingRestart: boolean;
  sessionPath: string;
}

export interface Settings {
  downloadDirectory: string;
  maxConcurrentDownloads: number;
  autoRetryAttempts: number;
  speedLimitKibPerSecond: number;
  downloadPerformanceMode: DownloadPerformanceMode;
  torrent: TorrentSettings;
  bulk: BulkDownloadSettings;
  notificationsEnabled: boolean;
  notificationSoundsEnabled: boolean;
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

export interface TorrentPeerDiagnostics {
  state: string;
  fetchedBytes: number;
  errors: number;
  downloadedPieces: number;
  connectionAttempts: number;
}

export interface TorrentRuntimeDiagnostics {
  queuedPeers: number;
  connectingPeers: number;
  livePeers: number;
  seenPeers: number;
  deadPeers: number;
  notNeededPeers: number;
  contributingPeers: number;
  peerErrors: number;
  peersWithErrors: number;
  peerConnectionAttempts: number;
  sessionDownloadSpeed: number;
  sessionUploadSpeed: number;
  dhtNodes?: number;
  dhtWarmupAgeMillis?: number;
  peerCacheHits?: number;
  millisecondsSinceMetadataResolved?: number;
  firstLivePeerMillis?: number;
  firstContributingPeerMillis?: number;
  firstPayloadMillis?: number;
  dhtNodesAtMetadataResolved?: number;
  lastPeerDiscoveryAssistAction?: string;
  averagePieceDownloadMillis?: number;
  listenPort?: number;
  listenerFallback: boolean;
  peerSamples?: TorrentPeerDiagnostics[];
}

export interface TorrentJobDiagnostics {
  jobId: string;
  filename: string;
  infoHash?: string;
  diagnostics: TorrentRuntimeDiagnostics;
}

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
  torrentDiagnostics?: TorrentJobDiagnostics[];
  recentEvents: DiagnosticEvent[];
}

export interface ToastMessage {
  id: string;
  type: 'info' | 'success' | 'warning' | 'error';
  title: string;
  message: string;
  autoClose?: boolean;
}
