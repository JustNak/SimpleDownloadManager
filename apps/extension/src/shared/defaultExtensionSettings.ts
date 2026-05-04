import {
  DEFAULT_EXTENSION_EXCLUDED_HOSTS,
  DEFAULT_EXTENSION_LISTEN_PORT,
  normalizeExcludedHostPattern,
  type ExtensionIntegrationSettings,
  type ProtectedDownloadAuthScope,
} from '@myapp/protocol';

export const DEFAULT_EXCLUDED_HOSTS = DEFAULT_EXTENSION_EXCLUDED_HOSTS;

export const defaultExtensionSettings: ExtensionIntegrationSettings = {
  enabled: true,
  downloadHandoffMode: 'ask',
  listenPort: DEFAULT_EXTENSION_LISTEN_PORT,
  contextMenuEnabled: true,
  showProgressAfterHandoff: true,
  showBadgeStatus: true,
  excludedHosts: [...DEFAULT_EXCLUDED_HOSTS],
  ignoredFileExtensions: [],
  authenticatedHandoffEnabled: false,
  protectedDownloadAuthScope: 'off',
  authenticatedHandoffHosts: [],
};

export function createDefaultExtensionSettings(): ExtensionIntegrationSettings {
  return {
    ...defaultExtensionSettings,
    excludedHosts: [...defaultExtensionSettings.excludedHosts],
    ignoredFileExtensions: [...defaultExtensionSettings.ignoredFileExtensions],
    authenticatedHandoffHosts: [...defaultExtensionSettings.authenticatedHandoffHosts],
  };
}

export function normalizeExtensionSettings(
  settings?: Partial<ExtensionIntegrationSettings>,
): ExtensionIntegrationSettings {
  const defaults = createDefaultExtensionSettings();
  const protectedDownloadAuthScope = normalizeProtectedDownloadAuthScope(settings);
  return {
    ...defaults,
    ...settings,
    authenticatedHandoffEnabled: protectedDownloadAuthScope !== 'off',
    protectedDownloadAuthScope,
    listenPort: normalizeListenPort(settings?.listenPort),
    excludedHosts: Array.from(
      new Set(
        (settings?.excludedHosts ?? defaults.excludedHosts)
          .map((host) => normalizeHost(host))
          .filter(Boolean),
      ),
    ),
    authenticatedHandoffHosts: Array.from(
      new Set(
        (settings?.authenticatedHandoffHosts ?? defaults.authenticatedHandoffHosts)
          .map((host) => normalizeHost(host))
          .filter(Boolean),
      ),
    ),
    ignoredFileExtensions: normalizeFileExtensions(
      settings?.ignoredFileExtensions ?? defaults.ignoredFileExtensions,
    ),
  };
}

function normalizeProtectedDownloadAuthScope(
  settings?: Partial<ExtensionIntegrationSettings>,
): ProtectedDownloadAuthScope {
  if (settings?.authenticatedHandoffEnabled === false) {
    return 'off';
  }

  if (settings?.protectedDownloadAuthScope === 'off') {
    return 'off';
  }

  if (settings?.protectedDownloadAuthScope === 'allowlist') {
    return 'allowlist';
  }

  if (settings?.protectedDownloadAuthScope === 'legacy_global') {
    return 'legacy_global';
  }

  return settings?.authenticatedHandoffEnabled ? 'legacy_global' : 'off';
}

function normalizeListenPort(value: unknown): number {
  const port = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(port)) return defaultExtensionSettings.listenPort;

  const normalizedPort = Math.floor(port);
  if (normalizedPort < 1 || normalizedPort > 65535) {
    return defaultExtensionSettings.listenPort;
  }

  return normalizedPort;
}

function normalizeHost(host: string): string {
  return normalizeExcludedHostPattern(host);
}

function normalizeFileExtensions(values: string[]): string[] {
  const extensions = new Set<string>();

  for (const value of values) {
    for (const candidate of value.split(/[,\s]+/)) {
      const extension = normalizeFileExtension(candidate);
      if (extension) extensions.add(extension);
    }
  }

  return [...extensions];
}

function normalizeFileExtension(value: string): string {
  const extension = value.trim().replace(/^\.+/, '').toLowerCase();
  if (!extension || extension.includes('/') || extension.includes('\\') || /^\.+$/.test(extension)) {
    return '';
  }

  return extension;
}
