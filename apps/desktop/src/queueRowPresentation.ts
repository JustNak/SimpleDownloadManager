import type { DownloadJob } from './types';

export const QUEUE_TABLE_COLUMNS = ['Name', 'Date', 'Speed', 'Time', 'Size', 'Actions'] as const;

export type QueueStatusTone = 'primary' | 'success' | 'destructive' | 'warning' | 'muted';

export interface QueueStatusPresentation {
  label: string;
  tone: QueueStatusTone;
}

export function shouldShowNameProgress(job: Pick<DownloadJob, 'state' | 'progress'>): boolean {
  return job.state === 'downloading' && clampQueueProgress(job.progress) > 0;
}

export function clampQueueProgress(progress: number): number {
  if (!Number.isFinite(progress)) return 0;
  return Math.max(0, Math.min(100, progress));
}

export function queueStatusPresentation(job: Pick<DownloadJob, 'state'>): QueueStatusPresentation {
  switch (job.state) {
    case 'completed':
      return { label: 'Done', tone: 'success' };
    case 'failed':
      return { label: 'Error', tone: 'destructive' };
    case 'queued':
      return { label: 'Queued', tone: 'warning' };
    case 'paused':
      return { label: 'Paused', tone: 'muted' };
    case 'canceled':
      return { label: 'Canceled', tone: 'muted' };
    case 'starting':
    case 'downloading':
      return { label: 'Downloading', tone: 'primary' };
    default:
      return { label: String(job.state), tone: 'muted' };
  }
}
