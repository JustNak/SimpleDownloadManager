import {
  DEFAULT_EXTENSION_EXCLUDED_HOSTS,
  DEFAULT_EXTENSION_LISTEN_PORT,
  DEFAULT_CAPTURED_FILE_EXTENSIONS,
  normalizeExcludedHostPattern,
  type ExtensionIntegrationSettings,
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
  capturedFileExtensions: [...DEFAULT_CAPTURED_FILE_EXTENSIONS],
  downloadCaptureDebugLogging: false,
};

export function createDefaultExtensionSettings(): ExtensionIntegrationSettings {
  return {
    ...defaultExtensionSettings,
    excludedHosts: [...defaultExtensionSettings.excludedHosts],
    ignoredFileExtensions: [...defaultExtensionSettings.ignoredFileExtensions],
    capturedFileExtensions: [...defaultExtensionSettings.capturedFileExtensions],
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
    excludedHosts: normalizeExcludedHosts(settings?.excludedHosts ?? defaults.excludedHosts),
    ignoredFileExtensions: normalizeFileExtensions(
      settings?.ignoredFileExtensions ?? defaults.ignoredFileExtensions,
    ),
    capturedFileExtensions: normalizeFileExtensions(
      settings?.capturedFileExtensions ?? defaults.capturedFileExtensions,
    ),
  };
}

function normalizeExcludedHosts(hosts: string[]): string[] {
  return Array.from(
    new Set(
      hosts
        .map((host) => normalizeHost(host))
        .filter(Boolean),
    ),
  );
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
  const extension = normalizeFileExtensionAlias(value.trim().replace(/^\.+/, '').toLowerCase());
  if (!extension || extension.includes('/') || extension.includes('\\') || /^\.+$/.test(extension)) {
    return '';
  }

  return extension;
}

function normalizeFileExtensionAlias(value: string): string {
  return value === '7zip' ? '7z' : value;
}
