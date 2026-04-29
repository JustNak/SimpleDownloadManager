import type { DownloadJob } from './types';

export const QUEUE_TABLE_COLUMNS = ['Name', 'Date', 'Speed', 'Time', 'Size', 'Actions'] as const;
export const TORRENT_QUEUE_TABLE_COLUMNS = ['Name', 'Date', 'Seed', 'Ratio', 'Size', 'Actions'] as const;

export type QueueStatusTone = 'primary' | 'success' | 'destructive' | 'warning' | 'muted';
export type FileBadgeActivityState = 'none' | 'buffering' | 'completed';

export interface QueueStatusPresentation {
  label: string;
  tone: QueueStatusTone;
}

export type TorrentDetailMetricKind = 'upload' | 'peers' | 'seeds';

export interface TorrentDetailMetric {
  kind: TorrentDetailMetricKind;
  label: string;
  value: number;
}

export function queueTableColumnsForView(view: string): readonly string[] {
  return isTorrentQueueView(view) ? TORRENT_QUEUE_TABLE_COLUMNS : QUEUE_TABLE_COLUMNS;
}

export function isTorrentQueueView(view: string): boolean {
  return view === 'torrents' || view.startsWith('torrent-');
}

type TorrentMetadataPendingJob = Pick<DownloadJob, 'state'> &
  Partial<Pick<DownloadJob, 'downloadedBytes' | 'filename' | 'progress' | 'torrent' | 'transferKind' | 'totalBytes'>>;

export function shouldShowNameProgress(job: TorrentMetadataPendingJob): boolean {
  if (isTorrentCheckingFiles(job) || isTorrentSeedingRestore(job)) return false;
  return job.state === 'downloading' && clampQueueProgress(job.progress ?? 0) > 0;
}

export function fileBadgeActivityState(
  job: Pick<DownloadJob, 'state'>,
  recentlyCompleted: boolean,
): FileBadgeActivityState {
  if (recentlyCompleted) return 'completed';
  if (job.state === 'starting' || job.state === 'downloading') return 'buffering';
  return 'none';
}

export function clampQueueProgress(progress: number): number {
  if (!Number.isFinite(progress)) return 0;
  return Math.max(0, Math.min(100, progress));
}

export function isTorrentMetadataPending(job: TorrentMetadataPendingJob): boolean {
  if (job.transferKind !== 'torrent') return false;
  if (job.state !== 'starting' && job.state !== 'downloading') return false;
  if ((job.totalBytes ?? 0) > 0) return false;
  return !job.torrent?.name;
}

export function isTorrentSeedingRestore(job: TorrentMetadataPendingJob): boolean {
  if (job.transferKind !== 'torrent') return false;
  if (!['queued', 'starting', 'downloading'].includes(job.state)) return false;
  if (isTorrentCheckingFiles(job)) return false;
  return typeof job.torrent?.seedingStartedAt === 'number';
}

export function isTorrentCheckingFiles(job: TorrentMetadataPendingJob): boolean {
  if (job.transferKind !== 'torrent') return false;
  if (job.state !== 'starting' && job.state !== 'downloading') return false;
  if ((job.totalBytes ?? 0) <= 0) return false;
  if ((job.downloadedBytes ?? 0) <= 0) return false;
  if (typeof job.torrent?.seedingStartedAt === 'number') return job.state === 'downloading';
  return (job.torrent?.fetchedBytes ?? 0) === 0;
}

export function torrentActivitySummary(job: TorrentMetadataPendingJob): string {
  if (isTorrentSeedingRestore(job)) return 'Restoring seeding';
  if (isTorrentCheckingFiles(job)) return 'Checking files';
  return isTorrentMetadataPending(job) ? 'Finding metadata' : 'No peer activity yet';
}

export function torrentDisplayName(job: TorrentMetadataPendingJob): string {
  const torrentName = job.torrent?.name?.trim();
  if (torrentName) return torrentName;

  const filename = job.filename?.trim();
  if (filename) return filename;

  const infoHash = job.torrent?.infoHash?.trim();
  if (infoHash) return `Torrent ${infoHash.slice(0, 12)}`;

  return 'Metadata pending';
}

export function queueStatusPresentation(job: TorrentMetadataPendingJob): QueueStatusPresentation {
  if (isTorrentSeedingRestore(job)) {
    return { label: 'Restoring seeding', tone: 'warning' };
  }

  if (isTorrentMetadataPending(job)) {
    return { label: 'Finding', tone: 'warning' };
  }

  if (isTorrentCheckingFiles(job)) {
    return { label: 'Checking', tone: 'warning' };
  }

  switch (job.state) {
    case 'seeding':
      return {
        label: 'Seeding',
        tone: 'primary',
      };
    case 'completed':
      return { label: 'Done', tone: 'success' };
    case 'failed':
      return { label: 'Error', tone: 'destructive' };
    case 'queued':
      return { label: 'Queued', tone: 'warning' };
    case 'paused':
      return { label: 'Paused', tone: 'muted' };
    case 'canceled':
      return { label: 'Canceled', tone: 'muted' };
    case 'starting':
    case 'downloading':
      return { label: 'Downloading', tone: 'primary' };
    default:
      return { label: String(job.state), tone: 'muted' };
  }
}

export function formatQueueSize(
  job: Pick<DownloadJob, 'state' | 'downloadedBytes' | 'totalBytes'> & Partial<Pick<DownloadJob, 'transferKind'>>,
  formatBytes: (bytes: number) => string,
): string {
  if (job.totalBytes <= 0) return formatBytes(job.downloadedBytes);
  if (job.transferKind === 'torrent') return formatBytes(job.totalBytes);
  if (job.state === 'completed') return formatBytes(job.totalBytes);
  return `${formatBytes(job.downloadedBytes)} / ${formatBytes(job.totalBytes)}`;
}

export function formatTorrentVerifiedSize(
  job: Pick<DownloadJob, 'downloadedBytes' | 'totalBytes'>,
  formatBytes: (bytes: number) => string,
): string {
  return `Verified ${formatBytes(job.downloadedBytes)} / ${job.totalBytes > 0 ? formatBytes(job.totalBytes) : 'Unknown'}`;
}

export function formatTorrentFetchedSize(
  job: Pick<DownloadJob, 'torrent' | 'totalBytes'>,
  formatBytes: (bytes: number) => string,
): string {
  const fetched = formatBytes(job.torrent?.fetchedBytes ?? 0);
  if (job.totalBytes <= 0) return `${fetched} from peers`;
  return `${fetched} / ${formatBytes(job.totalBytes)} from peers`;
}

export function formatQueueSizeTitle(
  job: Pick<DownloadJob, 'state' | 'downloadedBytes' | 'totalBytes' | 'torrent'> & Partial<Pick<DownloadJob, 'transferKind'>>,
  formatBytes: (bytes: number) => string,
): string {
  if (job.transferKind !== 'torrent') return formatQueueSize(job, formatBytes);
  return `${formatTorrentVerifiedSize(job, formatBytes)}; Downloaded ${formatTorrentFetchedSize(job, formatBytes)}`;
}

export function torrentDetailMetrics(job: Pick<DownloadJob, 'torrent' | 'state' | 'speed'>): TorrentDetailMetric[] {
  const metrics: TorrentDetailMetric[] = [];

  if (job.state === 'seeding' && job.torrent) {
    metrics.push({ kind: 'upload', label: 'Upload speed', value: Math.max(0, job.speed) });
  }

  if (typeof job.torrent?.peers === 'number') {
    metrics.push({ kind: 'peers', label: 'Peers', value: job.torrent.peers });
  }

  if (typeof job.torrent?.seeds === 'number') {
    metrics.push({ kind: 'seeds', label: 'Seeds', value: job.torrent.seeds });
  }

  return metrics;
}
