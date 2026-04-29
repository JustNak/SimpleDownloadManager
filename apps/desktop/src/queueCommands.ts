import type { DownloadJob } from './types';

const activeRemovalStates = new Set<string>([
  'starting',
  'downloading',
  'seeding',
]);

const progressPopupStates = new Set<string>([
  'queued',
  'starting',
  'downloading',
  'seeding',
]);

export function canRetryFailedDownloads(jobs: DownloadJob[]): boolean {
  return jobs.some((job) => job.state === 'failed');
}

export function canSwapFailedDownloadToBrowser(job: DownloadJob): boolean {
  return job.state === 'failed'
    && job.transferKind === 'http'
    && job.source?.entryPoint === 'browser_download'
    && isHttpUrl(job.url);
}

export function canClearCompletedDownloads(jobs: DownloadJob[]): boolean {
  return jobs.some((job) => job.state === 'completed' || job.state === 'canceled');
}

export function canRemoveDownloadImmediately(job: DownloadJob): boolean {
  return !activeRemovalStates.has(job.state);
}

export function isPausedSeedingTorrentDeleteCandidate(job: DownloadJob): boolean {
  return job.transferKind === 'torrent'
    && job.state === 'paused'
    && typeof job.torrent?.seedingStartedAt === 'number';
}

export function deleteActionLabelForJob(job: DownloadJob): string {
  return isPausedSeedingTorrentDeleteCandidate(job) ? 'Delete from disk...' : 'Delete';
}

export function defaultDeleteFromDiskForJobs(jobs: DownloadJob[]): boolean {
  return jobs.length === 1 && isPausedSeedingTorrentDeleteCandidate(jobs[0]);
}

export function canShowProgressPopup(job: DownloadJob): boolean {
  return progressPopupStates.has(job.state);
}

function isHttpUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    return parsed.protocol === 'http:' || parsed.protocol === 'https:';
  } catch {
    return false;
  }
}
