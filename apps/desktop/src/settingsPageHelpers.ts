import type { AppUpdateState, AppUpdateVersionTone } from './appUpdates.ts';
import { DEFAULT_EXTENSION_LISTEN_PORT, defaultBulkDownloadDirectory } from './defaultSettings.ts';
import type { DiagnosticsSnapshot, Settings } from './types.ts';

const bulkHosterFairnessModes = new Set(['adaptive', 'safe', 'off']);
const bulkHosterAccelerationModes = new Set(['safe', 'off']);

export function newestFirstDiagnosticEvents(
  diagnostics: DiagnosticsSnapshot | null,
): DiagnosticsSnapshot['recentEvents'] {
  return diagnostics?.recentEvents ? [...diagnostics.recentEvents].reverse() : [];
}

export function normalizeListenPort(value: string): number {
  const port = Number.parseInt(value, 10);
  return Number.isFinite(port) && port >= 1 && port <= 65535 ? port : DEFAULT_EXTENSION_LISTEN_PORT;
}

export function normalizeBulkSettings(
  settings: Settings['bulk'],
  downloadDirectory: string,
): Settings['bulk'] {
  return {
    ...settings,
    outputDirectory: settings.outputDirectory.trim() || defaultBulkDownloadDirectory(downloadDirectory),
    maxConcurrentDownloads: Math.max(1, Math.min(24, Math.trunc(settings.maxConcurrentDownloads) || 4)),
    speedLimitKibPerSecond: Math.max(0, Math.min(1048576, Math.trunc(settings.speedLimitKibPerSecond) || 0)),
    hosterFairnessMode: bulkHosterFairnessModes.has(settings.hosterFairnessMode) ? settings.hosterFairnessMode : 'adaptive',
    hosterAccelerationMode: bulkHosterAccelerationModes.has(settings.hosterAccelerationMode) ? settings.hosterAccelerationMode : 'safe',
    autoRetryAttempts: Math.max(0, Math.min(10, Math.trunc(settings.autoRetryAttempts) || 0)),
  };
}

export function normalizeTorrentPort(value: string): number {
  const port = Number.parseInt(value, 10);
  return Number.isFinite(port) && port >= 1024 && port <= 65534 ? port : 42000;
}

export function normalizeInteger(value: string, fallback: number): number {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? parsed : fallback;
}

export function normalizeNumber(value: string, fallback: number): number {
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

export function usesTorrentRatioLimit(mode: Settings['torrent']['seedMode']) {
  return mode === 'ratio' || mode === 'ratio_or_time';
}

export function usesTorrentTimeLimit(mode: Settings['torrent']['seedMode']) {
  return mode === 'time' || mode === 'ratio_or_time';
}

export function renderUpdateStatus(
  state: AppUpdateState,
  updateInstallBlocked: boolean,
): string {
  if (state.status === 'checking') return 'Checking GitHub Releases for a newer beta build.';
  if (state.status === 'available' && state.availableUpdate && updateInstallBlocked) return `Version ${state.availableUpdate.version} is ready after active bulk downloads pause or finish.`;
  if (state.status === 'available' && state.availableUpdate) return `Version ${state.availableUpdate.version} is available.`;
  if (state.status === 'not_available') return 'You are running the latest beta build.';
  if (state.status === 'downloading') return 'Downloading the signed update package.';
  if (state.status === 'installing') return 'Installing the update. The app may close automatically.';
  if (state.status === 'error') return 'The last update action failed.';
  return 'Checks the signed beta feed hosted on GitHub Releases.';
}

export function versionIndicatorToneClass(tone: AppUpdateVersionTone): string {
  switch (tone) {
    case 'available':
      return 'text-primary';
    case 'error':
      return 'text-destructive';
    case 'pending':
      return 'text-muted-foreground';
    default:
      return 'text-foreground';
  }
}

export function updateProgressPercent(state: AppUpdateState): number {
  if (!state.totalBytes || state.totalBytes <= 0) return 0;
  return Math.max(0, Math.min(100, (state.downloadedBytes / state.totalBytes) * 100));
}

export function formatUpdateProgress(state: AppUpdateState): string {
  if (!state.totalBytes) return `${formatCompactBytes(state.downloadedBytes)} downloaded`;
  return `${formatCompactBytes(state.downloadedBytes)} / ${formatCompactBytes(state.totalBytes)}`;
}

export function formatCompactBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  const units = ['B', 'KiB', 'MiB', 'GiB'];
  let unitIndex = 0;
  let nextValue = value;
  while (nextValue >= 1024 && unitIndex < units.length - 1) {
    nextValue /= 1024;
    unitIndex += 1;
  }
  return `${nextValue >= 10 || unitIndex === 0 ? nextValue.toFixed(0) : nextValue.toFixed(1)} ${units[unitIndex]}`;
}

export function renderRegistrationMessage(status?: DiagnosticsSnapshot['hostRegistration']['status']) {
  switch (status) {
    case 'configured':
      return 'At least one browser has a valid native host registration and host binary path.';
    case 'broken':
      return 'A browser registration exists, but the manifest or native host binary path is broken.';
    case 'missing':
      return 'No browser registration was detected for the native messaging host.';
    default:
      return 'Diagnostics are still loading.';
  }
}

export function registrationStatusLabel(status?: DiagnosticsSnapshot['hostRegistration']['status']) {
  switch (status) {
    case 'configured':
      return 'Ready';
    case 'broken':
      return 'Repair';
    case 'missing':
      return 'Missing';
    default:
      return 'Checking';
  }
}

export function registrationBadgeClass(status?: DiagnosticsSnapshot['hostRegistration']['status']) {
  switch (status) {
    case 'configured':
      return 'bg-success/10 text-success';
    case 'broken':
      return 'bg-warning/10 text-warning';
    case 'missing':
      return 'bg-destructive/10 text-destructive';
    default:
      return 'bg-muted text-muted-foreground';
  }
}

export function diagnosticLevelConsoleClass(level: DiagnosticsSnapshot['recentEvents'][number]['level']) {
  switch (level) {
    case 'error':
      return 'text-red-300';
    case 'warning':
      return 'text-amber-300';
    default:
      return 'text-emerald-300';
  }
}

export function formatDiagnosticEventTime(timestamp: number) {
  if (!Number.isFinite(timestamp) || timestamp <= 0) return 'Unknown time';
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(timestamp));
}
