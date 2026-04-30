import type { DownloadJob } from './types';

export type TorrentPeerHealthTone = 'success' | 'warning' | 'muted';

export interface TorrentPeerHealthDot {
  tone: TorrentPeerHealthTone;
}

export function torrentSourceSummary(job: Pick<DownloadJob, 'url'>): string {
  const rawUrl = job.url.trim();
  if (rawUrl.startsWith('magnet:')) {
    const trackerCount = magnetTrackerCount(rawUrl);
    return trackerCount > 0
      ? `DHT, ${trackerCount} ${trackerCount === 1 ? 'tracker' : 'trackers'}`
      : 'Magnet link';
  }

  if (/^https?:\/\//i.test(rawUrl) && /\.torrent(?:$|[?#])/i.test(rawUrl)) {
    return '.torrent URL';
  }

  if (/\.torrent$/i.test(rawUrl)) {
    return 'Local .torrent file';
  }

  return 'Torrent source';
}

export function torrentRemainingText(
  job: Pick<DownloadJob, 'downloadedBytes' | 'totalBytes'>,
  formatBytes: (bytes: number) => string,
): string {
  if (job.totalBytes <= 0) return 'Unknown remaining';
  return `${formatBytes(Math.max(0, job.totalBytes - job.downloadedBytes))} remaining`;
}

export function buildTorrentPeerHealthDots(
  job: Pick<DownloadJob, 'torrent'>,
  totalDots = 12,
): TorrentPeerHealthDot[] {
  const peers = Math.max(0, job.torrent?.peers ?? 0);
  const seeds = Math.max(0, job.torrent?.seeds ?? 0);
  const successDots = peers > 0 ? Math.min(6, Math.max(1, Math.ceil(peers / 5))) : 0;
  const warningDots = seeds > 0 ? Math.min(2, Math.max(1, Math.ceil(Math.min(seeds, 100) / 50))) : 0;
  const activeDots = Math.min(totalDots, successDots + warningDots);

  return Array.from({ length: totalDots }, (_, index) => {
    if (index < successDots) return { tone: 'success' };
    if (index < activeDots) return { tone: 'warning' };
    return { tone: 'muted' };
  });
}

export function torrentInfoHash(job: Pick<DownloadJob, 'torrent' | 'url'>): string {
  const existing = job.torrent?.infoHash?.trim();
  if (existing) return existing;

  if (!job.url.startsWith('magnet:')) return '--';
  const params = magnetSearchParams(job.url);
  const exactTopic = params.get('xt') ?? '';
  const match = exactTopic.match(/urn:btih:([^&]+)/i);
  return match?.[1] ?? '--';
}

export function torrentFilesText(
  job: Pick<DownloadJob, 'torrent' | 'totalBytes'>,
  formatBytes: (bytes: number) => string,
): string {
  const fileCount = job.torrent?.totalFiles;
  const size = job.totalBytes > 0 ? ` (${formatBytes(job.totalBytes)})` : '';
  if (typeof fileCount !== 'number') return `Files pending${size}`;
  return `${fileCount.toLocaleString()} ${fileCount === 1 ? 'file' : 'files'}${size}`;
}

export function torrentConnectedText(job: Pick<DownloadJob, 'torrent'>): string {
  const peers = job.torrent?.peers;
  return typeof peers === 'number' ? `${peers.toLocaleString()} connected` : 'Waiting for peers';
}

function magnetTrackerCount(rawUrl: string): number {
  return magnetSearchParams(rawUrl).getAll('tr').filter(Boolean).length;
}

function magnetSearchParams(rawUrl: string): URLSearchParams {
  const queryStart = rawUrl.indexOf('?');
  if (queryStart === -1) return new URLSearchParams();
  return new URLSearchParams(rawUrl.slice(queryStart + 1));
}
