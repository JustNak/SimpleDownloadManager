import { normalizeExcludedHostPattern, type ExtensionIntegrationSettings } from '@myapp/protocol';

export const DEFAULT_EXCLUDED_HOSTS = ['web.telegram.org'] as const;

export const defaultExtensionSettings: ExtensionIntegrationSettings = {
  enabled: true,
  downloadHandoffMode: 'ask',
  listenPort: 1420,
  contextMenuEnabled: true,
  showProgressAfterHandoff: true,
  showBadgeStatus: true,
  excludedHosts: [...DEFAULT_EXCLUDED_HOSTS],
  ignoredFileExtensions: [],
};

export function createDefaultExtensionSettings(): ExtensionIntegrationSettings {
  return {
    ...defaultExtensionSettings,
    excludedHosts: [...defaultExtensionSettings.excludedHosts],
    ignoredFileExtensions: [...defaultExtensionSettings.ignoredFileExtensions],
  };
}

export function normalizeExtensionSettings(
  settings?: Partial<ExtensionIntegrationSettings>,
): ExtensionIntegrationSettings {
  const defaults = createDefaultExtensionSettings();
  return {
    ...defaults,
    ...settings,
    listenPort: normalizeListenPort(settings?.listenPort),
    excludedHosts: Array.from(
      new Set(
        (settings?.excludedHosts ?? defaults.excludedHosts)
          .map((host) => normalizeHost(host))
          .filter(Boolean),
      ),
    ),
    ignoredFileExtensions: normalizeFileExtensions(
      settings?.ignoredFileExtensions ?? defaults.ignoredFileExtensions,
    ),
  };
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
