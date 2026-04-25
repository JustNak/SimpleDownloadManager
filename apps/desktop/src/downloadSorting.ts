import type { DownloadJob } from './types';

export type SortMode = 'status' | 'name' | 'progress' | 'size' | 'newest' | 'oldest';

export function compareDownloadsForSort(a: DownloadJob, b: DownloadJob, sortMode: SortMode): number {
  if (sortMode === 'name') return a.filename.localeCompare(b.filename);
  if (sortMode === 'progress') return b.progress - a.progress;
  if (sortMode === 'size') return b.totalBytes - a.totalBytes;
  if (sortMode === 'newest') return compareCreatedAt(a, b, 'desc') || a.filename.localeCompare(b.filename);
  if (sortMode === 'oldest') return compareCreatedAt(a, b, 'asc') || a.filename.localeCompare(b.filename);

  return statusRank(a.state) - statusRank(b.state) || a.filename.localeCompare(b.filename);
}

function compareCreatedAt(a: DownloadJob, b: DownloadJob, direction: 'asc' | 'desc'): number {
  const left = createdAtRank(a);
  const right = createdAtRank(b);
  if (left === right) return 0;
  if (left === 0) return 1;
  if (right === 0) return -1;
  return direction === 'asc' ? left - right : right - left;
}

function createdAtRank(job: DownloadJob): number {
  return Number.isFinite(job.createdAt) && typeof job.createdAt === 'number' && job.createdAt > 0
    ? job.createdAt
    : 0;
}

function statusRank(state: DownloadJob['state']) {
  switch (state) {
    case 'downloading':
      return 0;
    case 'starting':
      return 1;
    case 'queued':
      return 2;
    case 'paused':
      return 3;
    case 'failed':
      return 4;
    case 'completed':
      return 5;
    case 'canceled':
      return 6;
    default:
      return 7;
  }
}
