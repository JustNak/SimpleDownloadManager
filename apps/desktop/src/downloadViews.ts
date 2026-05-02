import type { DownloadCategory } from './downloadCategories';
import type { DownloadJob } from './types';

export type CategoryViewState = `category:${DownloadCategory}`;
export type TorrentViewState =
  | 'torrents'
  | 'torrent-active'
  | 'torrent-seeding'
  | 'torrent-attention'
  | 'torrent-queued'
  | 'torrent-completed';
export type ViewState =
  | 'all'
  | 'attention'
  | 'active'
  | 'queued'
  | 'completed'
  | 'settings'
  | CategoryViewState
  | TorrentViewState;

export interface QueueCounts {
  all: number;
  active: number;
  attention: number;
  queued: number;
  completed: number;
  categories: Record<DownloadCategory, number>;
  torrents: {
    all: number;
    active: number;
    seeding: number;
    attention: number;
    queued: number;
    completed: number;
  };
}

export interface TorrentFooterStats {
  all: number;
  active: number;
  seeding: number;
  uploadedBytes: number;
  downloadedBytes: number;
  totalRatio: number;
}

const DOWNLOAD_CATEGORY_IDS: readonly DownloadCategory[] = ['document', 'program', 'picture', 'video', 'compressed', 'music', 'other'];
const CATEGORY_BY_EXTENSION = new Map<string, DownloadCategory>([
  ...['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'txt', 'rtf', 'csv', 'md', 'epub'].map((extension) => [extension, 'document'] as const),
  ...['exe', 'msi', 'apk', 'dmg', 'pkg', 'deb', 'rpm', 'appimage'].map((extension) => [extension, 'program'] as const),
  ...['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'svg', 'tif', 'tiff', 'heic'].map((extension) => [extension, 'picture'] as const),
  ...['mp4', 'mkv', 'avi', 'mov', 'webm', 'm4v', 'wmv', 'flv'].map((extension) => [extension, 'video'] as const),
  ...['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'tgz'].map((extension) => [extension, 'compressed'] as const),
  ...['mp3', 'wav', 'flac', 'ogg', 'm4a', 'aac', 'opus', 'wma'].map((extension) => [extension, 'music'] as const),
]);
const activeDownloadStates = ['starting', 'downloading', 'paused'] as const;
const finishedStates = ['completed', 'canceled'] as const;

export function getQueueCounts(jobs: readonly DownloadJob[]): QueueCounts {
  const regularJobs = jobs.filter(isRegularDownload);
  const torrentJobs = jobs.filter(isTorrentDownload);

  return {
    all: regularJobs.length,
    active: regularJobs.filter((job) => stateIn(job.state, activeDownloadStates)).length,
    attention: regularJobs.filter(jobNeedsAttention).length,
    queued: regularJobs.filter((job) => job.state === 'queued').length,
    completed: regularJobs.filter((job) => stateIn(job.state, finishedStates)).length,
    categories: countJobsByCategory(regularJobs),
    torrents: {
      all: torrentJobs.length,
      active: torrentJobs.filter((job) => stateIn(job.state, activeDownloadStates)).length,
      seeding: torrentJobs.filter((job) => job.state === 'seeding').length,
      attention: torrentJobs.filter(jobNeedsAttention).length,
      queued: torrentJobs.filter((job) => job.state === 'queued').length,
      completed: torrentJobs.filter((job) => stateIn(job.state, finishedStates)).length,
    },
  };
}

export function filterJobsForView(jobs: readonly DownloadJob[], view: ViewState, query = ''): DownloadJob[] {
  const normalizedQuery = query.trim().toLowerCase();
  const category = categoryFromView(view);

  return jobs.filter((job) => {
    if (view === 'settings') return false;
    if (category) {
      return isRegularDownload(job)
        && filterJobsByCategory([job], category).length > 0
        && matchesSearchQuery(job, normalizedQuery);
    }

    if (isTorrentView(view)) {
      if (!isTorrentDownload(job)) return false;
      if (view === 'torrent-active' && !stateIn(job.state, activeDownloadStates)) return false;
      if (view === 'torrent-seeding' && job.state !== 'seeding') return false;
      if (view === 'torrent-attention' && !jobNeedsAttention(job)) return false;
      if (view === 'torrent-queued' && job.state !== 'queued') return false;
      if (view === 'torrent-completed' && !stateIn(job.state, finishedStates)) return false;
      return matchesSearchQuery(job, normalizedQuery);
    }

    if (!isRegularDownload(job)) return false;
    if (view === 'attention' && !jobNeedsAttention(job)) return false;
    if (view === 'active' && !stateIn(job.state, activeDownloadStates)) return false;
    if (view === 'queued' && job.state !== 'queued') return false;
    if (view === 'completed' && !stateIn(job.state, finishedStates)) return false;
    return matchesSearchQuery(job, normalizedQuery);
  });
}

export function isTorrentView(view: ViewState): view is TorrentViewState {
  return view === 'torrents' || view.startsWith('torrent-');
}

export function getTorrentFooterStats(jobs: readonly DownloadJob[]): TorrentFooterStats {
  const torrentJobs = jobs.filter(isTorrentDownload);
  const uploadedBytes = torrentJobs.reduce((total, job) => total + (job.torrent?.uploadedBytes ?? 0), 0);
  const downloadedBytes = torrentJobs.reduce((total, job) => total + Math.max(0, job.downloadedBytes), 0);

  return {
    all: torrentJobs.length,
    active: torrentJobs.filter((job) => stateIn(job.state, activeDownloadStates)).length,
    seeding: torrentJobs.filter((job) => job.state === 'seeding').length,
    uploadedBytes,
    downloadedBytes,
    totalRatio: downloadedBytes > 0 ? uploadedBytes / downloadedBytes : 0,
  };
}

function countJobsByCategory(jobs: readonly DownloadJob[]): Record<DownloadCategory, number> {
  const counts = Object.fromEntries(
    DOWNLOAD_CATEGORY_IDS.map((category) => [category, 0]),
  ) as Record<DownloadCategory, number>;

  for (const job of jobs) {
    counts[categoryForFilename(job.filename)] += 1;
  }

  return counts;
}

function filterJobsByCategory(jobs: readonly DownloadJob[], category: DownloadCategory): DownloadJob[] {
  return jobs.filter((job) => categoryForFilename(job.filename) === category);
}

export function categoryView(category: DownloadCategory): CategoryViewState {
  return `category:${category}`;
}

export function categoryFromView(view: ViewState): DownloadCategory | null {
  if (!view.startsWith('category:')) return null;
  const category = view.slice('category:'.length);
  return DOWNLOAD_CATEGORY_IDS.some((item) => item === category)
    ? category as DownloadCategory
    : null;
}

export function jobNeedsAttention(job: DownloadJob): boolean {
  if (job.state === 'failed' || job.failureCategory) return true;
  const isUnfinished = !stateIn(job.state, finishedStates);
  const hasPartialProgress = job.downloadedBytes > 0 || job.progress > 0;
  return isUnfinished && hasPartialProgress && job.resumeSupport === 'unsupported';
}

function isRegularDownload(job: DownloadJob): boolean {
  return !isTorrentDownload(job);
}

function isTorrentDownload(job: DownloadJob): boolean {
  return job.transferKind === 'torrent';
}

function stateIn(state: DownloadJob['state'], states: readonly string[]): boolean {
  return states.includes(String(state));
}

function categoryForFilename(filename: string): DownloadCategory {
  const basename = filename.trim().split(/[\\/]/).pop() ?? '';
  const dotIndex = basename.lastIndexOf('.');
  if (dotIndex <= 0 || dotIndex === basename.length - 1) return 'other';
  const extension = basename.slice(dotIndex + 1).toLowerCase();
  return CATEGORY_BY_EXTENSION.get(extension) ?? 'other';
}

function matchesSearchQuery(job: DownloadJob, normalizedQuery: string): boolean {
  if (!normalizedQuery) return true;
  return `${job.filename} ${job.url} ${job.targetPath ?? ''}`.toLowerCase().includes(normalizedQuery);
}
