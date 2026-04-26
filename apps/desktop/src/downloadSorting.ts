import type { DownloadJob } from './types';

export type SortColumn = 'name' | 'date' | 'size';
export type SortDirection = 'asc' | 'desc';
export type SortMode = `${SortColumn}:${SortDirection}`;

export function compareDownloadsForSort(a: DownloadJob, b: DownloadJob, sortMode: SortMode): number {
  const key = sortModeKey(sortMode);
  const direction = sortModeDirection(sortMode);

  if (key === 'name') {
    const filenameComparison = a.filename.localeCompare(b.filename);
    return direction === 'asc' ? filenameComparison : -filenameComparison;
  }

  if (key === 'size') {
    const sizeComparison = compareNumber(a.totalBytes, b.totalBytes, direction);
    return sizeComparison || a.filename.localeCompare(b.filename);
  }

  return compareCreatedAt(a, b, direction) || a.filename.localeCompare(b.filename);
}

export function sortModeKey(sortMode: SortMode): SortColumn {
  return sortMode.split(':')[0] as SortColumn;
}

export function sortModeDirection(sortMode: SortMode): SortDirection {
  return sortMode.split(':')[1] as SortDirection;
}

export function nextSortModeForColumn(currentSortMode: SortMode, column: SortColumn): SortMode {
  if (sortModeKey(currentSortMode) === column) {
    return `${column}:${sortModeDirection(currentSortMode) === 'asc' ? 'desc' : 'asc'}`;
  }

  return `${column}:${defaultSortDirection(column)}`;
}

function defaultSortDirection(column: SortColumn): SortDirection {
  return column === 'name' ? 'asc' : 'desc';
}

function compareCreatedAt(a: DownloadJob, b: DownloadJob, direction: SortDirection): number {
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

function compareNumber(a: number, b: number, direction: SortDirection): number {
  if (a === b) return 0;
  return direction === 'asc' ? a - b : b - a;
}
