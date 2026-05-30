import { DEFAULT_ACCENT_COLOR } from './appearance.ts';
import { DEFAULT_TORRENT_TRACKERS } from './torrentSettings.ts';
import type { ExtensionIntegrationSettings, Settings } from './types.ts';

export const DEFAULT_DOWNLOAD_DIRECTORY = 'C:\\Users\\You\\Downloads';
export const DEFAULT_EXTENSION_LISTEN_PORT = 1420;
export const DEFAULT_EXTENSION_EXCLUDED_HOSTS = ['web.telegram.org'] as const;
export const DEFAULT_CAPTURED_FILE_EXTENSIONS = [
  '7z',
  'apk',
  'bz2',
  'cab',
  'csv',
  'deb',
  'dmg',
  'doc',
  'docx',
  'exe',
  'gz',
  'iso',
  'jar',
  'msi',
  'pdf',
  'ppt',
  'pptx',
  'rar',
  'rpm',
  'tar',
  'tgz',
  'torrent',
  'txz',
  'xls',
  'xlsx',
  'xz',
  'zip',
  'zst',
] as const;

export function defaultBulkDownloadDirectory(downloadDirectory: string): string {
  const trimmed = downloadDirectory.trim();
  if (!trimmed) return 'Bulk';
  const separator = trimmed.includes('/') && !trimmed.includes('\\') ? '/' : '\\';
  return `${trimmed.replace(/[\\/]+$/, '')}${separator}Bulk`;
}

export function createDefaultExtensionIntegrationSettings(): ExtensionIntegrationSettings {
  return {
    enabled: true,
    downloadHandoffMode: 'ask',
    listenPort: DEFAULT_EXTENSION_LISTEN_PORT,
    contextMenuEnabled: true,
    showProgressAfterHandoff: true,
    showBadgeStatus: true,
    excludedHosts: [...DEFAULT_EXTENSION_EXCLUDED_HOSTS],
    ignoredFileExtensions: [],
    capturedFileExtensions: [...DEFAULT_CAPTURED_FILE_EXTENSIONS],
    downloadCaptureDebugLogging: false,
  };
}

export function createDefaultSettings(downloadDirectory = DEFAULT_DOWNLOAD_DIRECTORY): Settings {
  return {
    downloadDirectory,
    maxConcurrentDownloads: 3,
    autoRetryAttempts: 3,
    speedLimitKibPerSecond: 0,
    torrent: {
      enabled: true,
      downloadDirectory: `${downloadDirectory}\\Torrent`,
      seedMode: 'forever',
      seedRatioLimit: 1,
      seedTimeLimitMinutes: 60,
      uploadLimitKibPerSecond: 0,
      portForwardingEnabled: false,
      portForwardingPort: 42000,
      peerConnectionWatchdogMode: 'assist',
      customTrackers: [...DEFAULT_TORRENT_TRACKERS],
    },
    bulk: {
      outputDirectory: defaultBulkDownloadDirectory(downloadDirectory),
      maxConcurrentDownloads: 4,
      speedLimitKibPerSecond: 0,
      hosterFairnessMode: 'adaptive',
      hosterAccelerationMode: 'safe',
      autoRetryOverrideEnabled: false,
      autoRetryAttempts: 3,
      startBehavior: 'review_then_start',
      expandActiveRowsByDefault: false,
    },
    notificationsEnabled: true,
    notificationSoundsEnabled: true,
    theme: 'system',
    accentColor: DEFAULT_ACCENT_COLOR,
    showDetailsOnClick: true,
    queueRowSize: 'medium',
    startOnStartup: false,
    startupLaunchMode: 'open',
    extensionIntegration: createDefaultExtensionIntegrationSettings(),
  };
}
