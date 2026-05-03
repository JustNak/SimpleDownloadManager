import type { DownloadJob } from './types';

export interface DownloadUpdateBatch {
  jobs: DownloadJob[];
  removedJobIds: string[];
}

export function applyDownloadUpdateBatch(
  currentJobs: DownloadJob[],
  batch: DownloadUpdateBatch,
): DownloadJob[] {
  if (batch.jobs.length === 0 && batch.removedJobIds.length === 0) {
    return currentJobs;
  }

  const removedIds = new Set(batch.removedJobIds);
  const updatesById = new Map(batch.jobs.map((job) => [job.id, job]));
  const seenIds = new Set<string>();
  const nextJobs: DownloadJob[] = [];

  for (const job of currentJobs) {
    if (removedIds.has(job.id)) continue;

    const updatedJob = updatesById.get(job.id);
    if (updatedJob) {
      nextJobs.push(updatedJob);
      seenIds.add(job.id);
      continue;
    }

    nextJobs.push(job);
  }

  for (const job of batch.jobs) {
    if (!seenIds.has(job.id) && !removedIds.has(job.id)) {
      nextJobs.push(job);
    }
  }

  return nextJobs;
}
