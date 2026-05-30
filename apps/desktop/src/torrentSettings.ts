import type { TorrentSettings } from './types';

type TorrentSettingsInput = Partial<
  Omit<TorrentSettings, 'peerConnectionWatchdogMode'> & {
    peerConnectionWatchdogMode?: unknown;
  }
>;

export const DEFAULT_TORRENT_TRACKERS = [
  'udp://zer0day.ch:1337/announce',
  'udp://tracker.publictracker.xyz:6969/announce',
  'udp://tracker.opentrackr.org:1337/announce',
  'udp://open.demonii.com:1337/announce',
  'udp://open.stealth.si:80/announce',
  'udp://tracker.torrent.eu.org:451/announce',
  'udp://tracker.theoks.net:6969/announce',
  'udp://wepzone.net:6969/announce',
  'udp://vito-tracker.space:6969/announce',
  'udp://vito-tracker.duckdns.org:6969/announce',
  'udp://udp.tracker.projectk.org:23333/announce',
  'udp://tracker.tryhackx.org:6969/announce',
  'udp://tracker.t-1.org:6969/announce',
  'udp://tracker.srv00.com:6969/announce',
  'udp://tracker.qu.ax:6969/announce',
  'udp://tracker.auctor.tv:6969/announce',
  'udp://tracker.plx.im:6969/announce',
  'udp://tracker.opentorrent.top:6969/announce',
  'udp://tracker.gmi.gd:6969/announce',
  'udp://tracker.ducks.party:1984/announce',
  'udp://tracker.bluefrog.pw:2710/announce',
  'udp://tracker.bittor.pw:1337/announce',
  'udp://tracker.1h.is:1337/announce',
  'udp://tracker.004430.xyz:1337/announce',
  'udp://tracker-udp.gbitt.info:80/announce',
  'udp://tr4ck3r.duckdns.org:6969/announce',
  'udp://torrents.tmtime.dev:6969/announce',
  'udp://retracker01-msk-virt.corbina.net:80/announce',
  'https://tracker.zhuqiy.com:443/announce',
  'https://tracker.yemekyedim.com:443/announce',
  'https://tracker.pmman.tech:443/announce',
  'https://tracker.nekomi.cn:443/announce',
  'https://tracker.moeking.me:443/announce',
  'https://tracker.bt4g.com:443/announce',
  'https://torrents.tmtime.dev:443/announce',
  'https://open.ftorrent.com:443/announce',
  'http://tracker.zhuqiy.com:80/announce',
] as const;

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
  customTrackers: [...DEFAULT_TORRENT_TRACKERS],
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
    customTrackers: value && 'customTrackers' in value
      ? normalizeCustomTrackers(value.customTrackers)
      : [...DEFAULT_TORRENT_SETTINGS.customTrackers],
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
