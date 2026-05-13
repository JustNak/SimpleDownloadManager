import { formatBytes, formatTime } from './popupShared.ts';
import type { DownloadJob } from './types.ts';

export function formatQueueSpeed(job: DownloadJob, averageSpeed: number) {
  if (job.state === 'downloading') return `${formatBytes(averageSpeed)}/s`;
  if (job.state === 'seeding' && job.torrent) return `Up ${formatBytes(job.torrent.uploadedBytes)}`;
  return '--';
}

export function formatTorrentSeedMetric(job: DownloadJob) {
  if (!job.torrent) return '--';
  if (job.torrent.uploadedBytes > 0) return formatBytes(job.torrent.uploadedBytes);
  return '--';
}

export function formatTorrentRatio(job: DownloadJob) {
  const ratio = job.torrent?.ratio;
  if (!Number.isFinite(ratio) || !ratio || ratio <= 0) return '--';
  return `${ratio.toFixed(2)}x`;
}

export function formatQueueTime(job: DownloadJob, timeRemaining: number) {
  if (job.state === 'downloading') return formatTime(timeRemaining);
  if (job.state === 'seeding' && job.torrent) return formatTorrentRatio(job);
  return '--';
}

export function formatJobDate(timestamp: number | undefined) {
  if (!isValidTimestamp(timestamp)) return '--';
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(timestamp));
}

export function formatFullJobDate(timestamp: number | undefined) {
  if (!isValidTimestamp(timestamp)) return 'No date recorded';
  return new Intl.DateTimeFormat(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(timestamp));
}

function isValidTimestamp(timestamp: number | undefined): timestamp is number {
  return typeof timestamp === 'number' && Number.isFinite(timestamp) && timestamp > 0;
}
