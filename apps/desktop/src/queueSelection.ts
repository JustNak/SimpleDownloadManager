import { isBulkAggregateJob, type QueueDisplayJob } from './bulkQueueRows.ts';

export function selectedIdsForJob(job: QueueDisplayJob, selectedJobIds: Set<string>): string[] {
  if (selectedJobIds.has(job.id) && selectedJobIds.size > 1) return [...selectedJobIds];
  return [job.id];
}

export function deleteJobIdsForPrompt(promptJobs: QueueDisplayJob[]): string[] {
  return promptJobs.flatMap((job) => isBulkAggregateJob(job) ? job.bulkMemberIds : [job.id]);
}
