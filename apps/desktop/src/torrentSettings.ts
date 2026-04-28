import type { TorrentSettings } from './types';

const DEFAULT_TORRENT_SETTINGS: TorrentSettings = {
  enabled: true,
  seedMode: 'forever',
  seedRatioLimit: 1,
  seedTimeLimitMinutes: 60,
  uploadLimitKibPerSecond: 0,
  portForwardingEnabled: false,
  portForwardingPort: 42000,
};

export function normalizeTorrentSettings(value: Partial<TorrentSettings> | undefined): TorrentSettings {
  return {
    enabled: value?.enabled ?? DEFAULT_TORRENT_SETTINGS.enabled,
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
  };
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

function clampNumber(value: unknown, min: number, max: number, fallback: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback;
  return Math.max(min, Math.min(max, value));
}

function normalizeForwardingPort(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return DEFAULT_TORRENT_SETTINGS.portForwardingPort;
  const port = Math.round(value);
  return port >= 1024 && port <= 65534 ? port : DEFAULT_TORRENT_SETTINGS.portForwardingPort;
}
