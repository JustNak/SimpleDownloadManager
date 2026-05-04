import { DEFAULT_ACCENT_COLOR } from './appearance';
import type { ExtensionIntegrationSettings, Settings } from './types';

export const DEFAULT_DOWNLOAD_DIRECTORY = 'C:\\Users\\You\\Downloads';
export const DEFAULT_EXTENSION_LISTEN_PORT = 1420;
export const DEFAULT_EXTENSION_EXCLUDED_HOSTS = ['web.telegram.org'] as const;

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
    authenticatedHandoffEnabled: false,
    protectedDownloadAuthScope: 'off',
    authenticatedHandoffHosts: [],
  };
}

export function createDefaultSettings(downloadDirectory = DEFAULT_DOWNLOAD_DIRECTORY): Settings {
  return {
    downloadDirectory,
    maxConcurrentDownloads: 3,
    autoRetryAttempts: 3,
    speedLimitKibPerSecond: 0,
    downloadPerformanceMode: 'balanced',
    torrent: {
      enabled: true,
      downloadDirectory: `${downloadDirectory}\\Torrent`,
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
    extensionIntegration: createDefaultExtensionIntegrationSettings(),
  };
}
