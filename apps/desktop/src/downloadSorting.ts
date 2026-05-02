import type { DownloadJob } from './types';

export type SortColumn = 'name' | 'date' | 'size';
export type SortDirection = 'asc' | 'desc';
export type SortMode = `${SortColumn}:${SortDirection}`;

export const DEFAULT_SORT_MODE: SortMode = 'date:asc';
export const SORT_MODE_STORAGE_KEY = 'simple-download-manager.sortMode';

export interface SortModeStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

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

export function isSortMode(value: string | null | undefined): value is SortMode {
  return /^(name|date|size):(asc|desc)$/.test(value ?? '');
}

export function readStoredSortMode(storage: SortModeStorage | null = getBrowserSortStorage()): SortMode {
  if (!storage) return DEFAULT_SORT_MODE;

  try {
    const storedSortMode = storage.getItem(SORT_MODE_STORAGE_KEY);
    return isSortMode(storedSortMode) ? storedSortMode : DEFAULT_SORT_MODE;
  } catch {
    return DEFAULT_SORT_MODE;
  }
}

export function writeStoredSortMode(sortMode: SortMode, storage: SortModeStorage | null = getBrowserSortStorage()): void {
  if (!storage) return;

  try {
    storage.setItem(SORT_MODE_STORAGE_KEY, sortMode);
  } catch {
    // Non-critical preference persistence can fail in restricted browser storage modes.
  }
}

function defaultSortDirection(column: SortColumn): SortDirection {
  return column === 'name' ? 'asc' : 'desc';
}

function getBrowserSortStorage(): SortModeStorage | null {
  if (typeof globalThis.localStorage === 'undefined') return null;
  return globalThis.localStorage;
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
