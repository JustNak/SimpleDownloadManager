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
