import type { TorrentSettings } from './types';

type TorrentSettingsInput = Partial<
  Omit<TorrentSettings, 'peerConnectionWatchdogMode'> & {
    peerConnectionWatchdogMode?: unknown;
  }
>;

const DEFAULT_TORRENT_SETTINGS: TorrentSettings = {
  enabled: true,
  downloadDirectory: '',
  seedMode: 'forever',
  seedRatioLimit: 1,
  seedTimeLimitMinutes: 60,
  uploadLimitKibPerSecond: 0,
  portForwardingEnabled: false,
  portForwardingPort: 42000,
  peerConnectionWatchdogMode: 'assist',
  customTrackers: [],
};

export function normalizeTorrentSettings(
  value: TorrentSettingsInput | undefined,
  downloadDirectory = '',
): TorrentSettings {
  return {
    enabled: value?.enabled ?? DEFAULT_TORRENT_SETTINGS.enabled,
    downloadDirectory: normalizeTorrentDownloadDirectory(value?.downloadDirectory, downloadDirectory),
    seedMode: isSeedMode(value?.seedMode) ? value.seedMode : DEFAULT_TORRENT_SETTINGS.seedMode,
    seedRatioLimit: clampNumber(value?.seedRatioLimit, 0.1, 100, DEFAULT_TORRENT_SETTINGS.seedRatioLimit),
    seedTimeLimitMinutes: Math.round(clampNumber(
      value?.seedTimeLimitMinutes,
      1,
      525600,
      DEFAULT_TORRENT_SETTINGS.seedTimeLimitMinutes,
    )),
    uploadLimitKibPerSecond: Math.round(clampNumber(value?.uploadLimitKibPerSecond, 0, 1_048_576, 0)),
    portForwardingEnabled: value?.portForwardingEnabled ?? DEFAULT_TORRENT_SETTINGS.portForwardingEnabled,
    portForwardingPort: normalizeForwardingPort(value?.portForwardingPort),
    peerConnectionWatchdogMode: normalizePeerConnectionWatchdogMode(value?.peerConnectionWatchdogMode),
    customTrackers: normalizeCustomTrackers(value?.customTrackers),
  };
}

export function defaultTorrentDownloadDirectory(downloadDirectory: string): string {
  const trimmed = downloadDirectory.trim().replace(/[\\/]+$/, '');
  if (!trimmed) return '';
  const separator = trimmed.includes('\\') ? '\\' : '/';
  return `${trimmed}${separator}Torrent`;
}

export function shouldStopSeeding(settings: TorrentSettings, ratio: number, elapsedSeconds: number): boolean {
  switch (settings.seedMode) {
    case 'forever':
      return false;
    case 'ratio':
      return ratio >= settings.seedRatioLimit;
    case 'time':
      return elapsedSeconds >= settings.seedTimeLimitMinutes * 60;
    case 'ratio_or_time':
      return ratio >= settings.seedRatioLimit || elapsedSeconds >= settings.seedTimeLimitMinutes * 60;
    default:
      return false;
  }
}

function isSeedMode(value: unknown): value is TorrentSettings['seedMode'] {
  return value === 'forever' || value === 'ratio' || value === 'time' || value === 'ratio_or_time';
}

function normalizePeerConnectionWatchdogMode(value: unknown): TorrentSettings['peerConnectionWatchdogMode'] {
  if (value === 'assist') return 'assist';
  if (value === 'diagnose') return 'diagnose';
  if (value === 'recover' || value === 'experimental') return 'recover';
  return DEFAULT_TORRENT_SETTINGS.peerConnectionWatchdogMode;
}

function clampNumber(value: unknown, min: number, max: number, fallback: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback;
  return Math.max(min, Math.min(max, value));
}

function normalizeForwardingPort(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return DEFAULT_TORRENT_SETTINGS.portForwardingPort;
  const port = Math.round(value);
  return port >= 1024 && port <= 65534 ? port : DEFAULT_TORRENT_SETTINGS.portForwardingPort;
}

function normalizeCustomTrackers(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  const trackers: string[] = [];

  for (const item of value) {
    if (typeof item !== 'string') continue;
    const normalized = normalizeTrackerUrl(item);
    if (!normalized) continue;
    const key = normalized.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    trackers.push(normalized);
    if (trackers.length >= 64) break;
  }

  return trackers;
}

function normalizeTrackerUrl(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  try {
    const url = new URL(trimmed);
    if (url.protocol !== 'udp:' && url.protocol !== 'http:' && url.protocol !== 'https:') return null;
    if (!url.hostname) return null;
    url.hash = '';
    return url.toString();
  } catch {
    return null;
  }
}

function normalizeTorrentDownloadDirectory(value: unknown, downloadDirectory: string): string {
  if (typeof value === 'string' && value.trim()) return value.trim();
  return defaultTorrentDownloadDirectory(downloadDirectory);
}
