import type { DownloadJob } from './types';

const activeRemovalStates = new Set<string>([
  'starting',
  'downloading',
  'seeding',
]);

export function canRetryFailedDownloads(jobs: DownloadJob[]): boolean {
  return jobs.some((job) => job.state === 'failed');
}

export function canClearCompletedDownloads(jobs: DownloadJob[]): boolean {
  return jobs.some((job) => job.state === 'completed' || job.state === 'canceled');
}

export function canRemoveDownloadImmediately(job: DownloadJob): boolean {
  return !activeRemovalStates.has(job.state);
}
