import { DEFAULT_ACCENT_COLOR } from './appearance.ts';
import type { ExtensionIntegrationSettings, Settings } from './types.ts';

export const DEFAULT_DOWNLOAD_DIRECTORY = 'C:\\Users\\You\\Downloads';
export const DEFAULT_EXTENSION_LISTEN_PORT = 1420;
export const DEFAULT_EXTENSION_EXCLUDED_HOSTS = [] as const;
export const DEFAULT_PROTECTED_DOWNLOAD_AUTH_HOSTS = ['gofile.io'] as const;

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
    authenticatedHandoffEnabled: true,
    protectedDownloadAuthScope: 'allowlist',
    authenticatedHandoffHosts: [...DEFAULT_PROTECTED_DOWNLOAD_AUTH_HOSTS],
  };
}

export function createDefaultSettings(downloadDirectory = DEFAULT_DOWNLOAD_DIRECTORY): Settings {
  return {
    downloadDirectory,
    maxConcurrentDownloads: 3,
    autoRetryAttempts: 3,
    speedLimitKibPerSecond: 0,
    downloadPerformanceMode: 'fast',
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
      customTrackers: [],
    },
    bulk: {
      outputDirectory: defaultBulkDownloadDirectory(downloadDirectory),
      maxConcurrentDownloads: 4,
      speedLimitKibPerSecond: 0,
      downloadPerformanceMode: 'fast',
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
