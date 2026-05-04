import type { AddJobResult, AddJobsResult } from './backend';
import type { DownloadJob } from './types';

export type DownloadMode = 'single' | 'torrent' | 'multi' | 'bulk';
export type ProgressBatchKind = 'multi' | 'bulk';
export type BulkPhase = 'review' | 'downloading' | 'extracting' | 'combining' | 'creating_folder' | 'compressing' | 'ready' | 'failed';
export type BulkUiState = 'review' | 'downloading' | 'finalizing' | 'ready' | 'failed';
export type BulkFinalizingStepId = 'uncompressing' | 'combining' | 'compressing';

export interface BulkFinalizingStep {
  id: BulkFinalizingStepId;
  label: string;
}

export interface BulkReviewStartSelection {
  includedJobs: DownloadJob[];
  excludedJobs: DownloadJob[];
  resumableJobs: DownloadJob[];
}

export interface ProgressBatchContext {
  batchId?: string;
  kind: ProgressBatchKind;
  jobIds: string[];
  title: string;
  archiveName?: string;
}

export type ProgressPopupIntent =
  | { type: 'single'; jobId: string }
  | { type: 'batch'; context: ProgressBatchContext };

export interface BatchProgressSummary {
  progress: number;
  downloadedBytes: number;
  totalBytes: number;
  knownTotal: boolean;
  completedCount: number;
  failedCount: number;
  activeCount: number;
  totalCount: number;
}

const terminalStates = new Set(['completed', 'failed', 'canceled']);

export function calculateBatchProgress(jobs: DownloadJob[]): BatchProgressSummary {
  const totalCount = jobs.length;
  const downloadedBytes = jobs.reduce((total, job) => total + Math.max(0, job.downloadedBytes || 0), 0);
  const totalBytes = jobs.reduce((total, job) => total + Math.max(0, job.totalBytes || 0), 0);
  const knownTotal = totalCount > 0 && jobs.every((job) => job.totalBytes > 0);
  const completedCount = jobs.filter((job) => job.state === 'completed').length;
  const failedCount = jobs.filter((job) => job.state === 'failed').length;
  const activeCount = jobs.filter((job) => !terminalStates.has(job.state)).length;

  const progress = knownTotal && totalBytes > 0
    ? (downloadedBytes / totalBytes) * 100
    : totalCount > 0
      ? (jobs.filter((job) => terminalStates.has(job.state)).length / totalCount) * 100
      : 0;

  return {
    progress: clampProgress(progress),
    downloadedBytes,
    totalBytes,
    knownTotal,
    completedCount,
    failedCount,
    activeCount,
    totalCount,
  };
}

export function deriveBulkPhase(jobs: DownloadJob[]): BulkPhase {
  const archiveJobs = jobs.filter((job) => job.bulkArchive);
  if (jobs.some((job) => job.bulkArchive?.archiveStatus === 'failed' || job.state === 'failed')) {
    return 'failed';
  }
  if (
    archiveJobs.length > 0
    && archiveJobs.length === jobs.length
    && jobs.every((job) => job.state === 'paused' && job.downloadedBytes === 0 && job.progress === 0)
    && archiveJobs.every((job) => job.bulkArchive?.archiveStatus === 'pending')
  ) {
    return 'review';
  }
  if (jobs.some((job) => job.bulkArchive?.archiveStatus === 'completed')) {
    return 'ready';
  }
  if (jobs.some((job) => job.bulkArchive?.archiveStatus === 'compressing')) {
    return 'compressing';
  }
  if (jobs.some((job) => job.bulkArchive?.archiveStatus === 'combining')) {
    return 'combining';
  }
  if (jobs.some((job) => job.bulkArchive?.archiveStatus === 'creating_folder')) {
    return 'combining';
  }
  if (jobs.some((job) => job.bulkArchive?.archiveStatus === 'extracting')) {
    return 'extracting';
  }
  if (jobs.length > 0 && archiveJobs.length === 0 && jobs.every((job) => job.state === 'completed')) {
    return 'ready';
  }
  if (jobs.length > 0 && jobs.every((job) => job.state === 'completed')) {
    const outputKind = archiveJobs.find((job) => job.bulkArchive)?.bulkArchive?.outputKind ?? 'archive';
    return outputKind === 'folder' ? 'combining' : 'compressing';
  }
  return 'downloading';
}

export function deriveBulkUiState(jobs: DownloadJob[]): BulkUiState {
  const phase = deriveBulkPhase(jobs);
  if (phase === 'review' || phase === 'ready' || phase === 'failed') return phase;
  if (phase === 'extracting' || phase === 'combining' || phase === 'creating_folder' || phase === 'compressing') {
    return 'finalizing';
  }
  return 'downloading';
}

export function bulkReviewStartSelection(
  jobs: DownloadJob[],
  selectedJobIds: ReadonlySet<string>,
): BulkReviewStartSelection {
  const includedJobs = jobs.filter((job) => selectedJobIds.has(job.id));
  const excludedJobs = jobs.filter((job) => !selectedJobIds.has(job.id));
  return {
    includedJobs,
    excludedJobs,
    resumableJobs: includedJobs.filter(isResumableReviewJob),
  };
}

export function bulkFinalizingSteps(jobs: DownloadJob[]): BulkFinalizingStep[] {
  const archive = jobs.find((job) => job.bulkArchive)?.bulkArchive;
  const outputKind = archive?.outputKind ?? 'archive';
  const shouldShowUncompressing = archive?.requiresExtraction === true || archive?.archiveStatus === 'extracting';
  const steps: BulkFinalizingStep[] = [];

  if (shouldShowUncompressing) {
    steps.push({ id: 'uncompressing', label: 'Uncompressing' });
  }

  steps.push({ id: 'combining', label: 'Combining' });

  if (outputKind === 'archive') {
    steps.push({ id: 'compressing', label: 'Compressing' });
  }

  return steps;
}

export function activeBulkFinalizingStepId(phase: BulkPhase | null): BulkFinalizingStepId | null {
  if (phase === 'extracting') return 'uncompressing';
  if (phase === 'combining' || phase === 'creating_folder') return 'combining';
  if (phase === 'compressing') return 'compressing';
  return null;
}

export function progressPopupIntentForSubmission(
  mode: DownloadMode,
  result: AddJobResult | AddJobsResult,
  archiveName?: string,
): ProgressPopupIntent | null {
  if (mode === 'single' || mode === 'torrent') {
    const singleResult = result as AddJobResult;
    return singleResult.status === 'queued' ? { type: 'single', jobId: singleResult.jobId } : null;
  }

  const batchResult = result as AddJobsResult;
  const jobIds = batchResult.results
    .filter((item) => item.status === 'queued')
    .map((item) => item.jobId);

  if (jobIds.length === 0) return null;

  const isBulkArchive = mode === 'bulk' && Boolean(archiveName);
  return {
    type: 'batch',
    context: {
      kind: isBulkArchive ? 'bulk' : 'multi',
      jobIds,
      title: isBulkArchive ? 'Bulk download progress' : 'Multi-download progress',
      ...(isBulkArchive && archiveName ? { archiveName } : {}),
    },
  };
}

function clampProgress(value: number) {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, value));
}

function isResumableReviewJob(job: DownloadJob) {
  return ['paused', 'failed', 'canceled'].includes(job.state);
}
