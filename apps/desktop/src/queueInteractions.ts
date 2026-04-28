import type { DownloadJob } from './types';

export function shouldOpenJobFileOnDoubleClick(job: DownloadJob, button: number): boolean {
  return button === 0 && Boolean(job.targetPath?.trim());
}

export function isJobArtifactMissing(job: DownloadJob): boolean {
  return job.state === 'completed' && job.artifactExists === false;
}

export function shouldBlurJobIdentity(job: DownloadJob): boolean {
  return job.state === 'starting' || job.state === 'downloading';
}

export function selectJobRange(jobIds: string[], anchorId: string, currentId: string): string[] {
  const anchorIndex = jobIds.indexOf(anchorId);
  const currentIndex = jobIds.indexOf(currentId);
  if (anchorIndex === -1 || currentIndex === -1) return [];

  const start = Math.min(anchorIndex, currentIndex);
  const end = Math.max(anchorIndex, currentIndex);
  return jobIds.slice(start, end + 1);
}
