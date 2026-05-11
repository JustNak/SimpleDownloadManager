import type { AddJobResult, AddJobsResult, FailedBatchItem } from './backend';
import type { BulkArchiveStatus, DownloadJob } from './types';

export type { FailedBatchItem };

export type DownloadMode = 'single' | 'torrent' | 'bulk';
export type ProgressBatchKind = 'multi' | 'bulk';
export type BulkPhase = 'review' | 'downloading' | 'extracting' | 'combining' | 'creating_folder' | 'compressing' | 'ready' | 'failed' | 'canceled';
export type BulkUiState = 'review' | 'downloading' | 'finalizing' | 'ready' | 'failed' | 'canceled';
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

export interface BulkFailedRetrySelection {
  selectedJobs: DownloadJob[];
  excludedJobs: DownloadJob[];
  selectedJobIds: string[];
  excludedJobIds: string[];
  canRetry: boolean;
}

export interface BulkCancelConfirmPlan {
  cancelJobIds: string[];
  deleteJobIds: string[];
  deleteFromDisk: boolean;
  closeOnSuccess: boolean;
}

export interface ProgressBatchContext {
  batchId?: string;
  kind: ProgressBatchKind;
  jobIds: string[];
  title: string;
  archiveName?: string;
  failedItems?: FailedBatchItem[];
}

export type StoredProgressBatchContext = ProgressBatchContext & { batchId: string };

export function createStoredProgressBatchContext(context: ProgressBatchContext): StoredProgressBatchContext {
  return {
    ...context,
    batchId: context.batchId ?? createProgressBatchId(),
  };
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
  if (jobs.some((job) => bulkArchiveStatus(job) === 'failed')) {
    return 'failed';
  }
  if (isUntouchedBulkReviewGate(jobs)) {
    return 'review';
  }
  if (jobs.some((job) => bulkArchiveStatus(job) === 'completed')) {
    return 'ready';
  }
  if (jobs.some((job) => bulkArchiveStatus(job) === 'compressing')) {
    return 'compressing';
  }
  if (jobs.some((job) => bulkArchiveStatus(job) === 'combining')) {
    return 'combining';
  }
  if (jobs.some((job) => bulkArchiveStatus(job) === 'creating_folder')) {
    return 'combining';
  }
  if (jobs.some((job) => bulkArchiveStatus(job) === 'extracting')) {
    return 'extracting';
  }
  if (jobs.length > 0 && archiveJobs.length === 0 && jobs.every((job) => job.state === 'completed')) {
    return 'ready';
  }
  if (
    jobs.length > 0
    && jobs.every((job) => job.state === 'completed' || job.state === 'canceled')
    && jobs.some((job) => job.state === 'canceled')
  ) {
    return 'canceled';
  }
  if (jobs.length > 0 && jobs.every((job) => job.state === 'completed')) {
    const outputKind = archiveJobs.find((job) => job.bulkArchive)?.bulkArchive?.outputKind ?? 'folder';
    return outputKind === 'folder' ? 'combining' : 'compressing';
  }
  return 'downloading';
}

export function isUntouchedBulkReviewGate(jobs: DownloadJob[]): boolean {
  const archiveJobs = jobs.filter((job) => job.bulkArchive);
  return (
    archiveJobs.length > 0
    && archiveJobs.length === jobs.length
    && jobs.every((job) => (
      (job.state === 'paused' || job.state === 'queued')
      && job.downloadedBytes === 0
      && job.progress === 0
    ))
    && archiveJobs.every((job) => bulkArchiveStatus(job) === 'pending')
  );
}

export function deriveBulkUiState(jobs: DownloadJob[]): BulkUiState {
  const phase = deriveBulkPhase(jobs);
  if (phase === 'review' || phase === 'ready' || phase === 'failed' || phase === 'canceled') return phase;
  if (phase === 'extracting' || phase === 'combining' || phase === 'creating_folder' || phase === 'compressing') {
    return 'finalizing';
  }
  return 'downloading';
}

export function bulkReviewStartSelection(
  jobs: DownloadJob[],
  selectedJobIds: ReadonlySet<string>,
): BulkReviewStartSelection {
  const includedJobs = jobs.filter((job) => selectedJobIds.has(job.id) && isBulkReviewReadyJob(job));
  const excludedJobs = jobs.filter((job) => !selectedJobIds.has(job.id) || !isBulkReviewReadyJob(job));
  return {
    includedJobs,
    excludedJobs,
    resumableJobs: includedJobs.filter(isResumableReviewJob),
  };
}

export function bulkReviewCanStart(
  jobs: DownloadJob[],
  selectedJobIds: ReadonlySet<string>,
): boolean {
  if (jobs.some(hasPendingHosterPreflight)) return false;
  return bulkReviewStartSelection(jobs, selectedJobIds).resumableJobs.length > 0;
}

export function isBulkReviewReadyJob(job: DownloadJob): boolean {
  return !job.hosterPreflight || job.hosterPreflight.status === 'ready';
}

export function isBulkReviewPendingJob(job: DownloadJob): boolean {
  return hasPendingHosterPreflight(job);
}

export function isBulkReviewUnavailableJob(job: DownloadJob): boolean {
  return job.hosterPreflight?.status === 'failed';
}

export function bulkFailedRetrySelection(
  jobs: DownloadJob[],
  selectedJobIds: ReadonlySet<string>,
): BulkFailedRetrySelection {
  const selectedJobs = jobs.filter((job) => selectedJobIds.has(job.id));
  const excludedJobs = jobs.filter((job) => !selectedJobIds.has(job.id));
  return {
    selectedJobs,
    excludedJobs,
    selectedJobIds: selectedJobs.map((job) => job.id),
    excludedJobIds: excludedJobs.map((job) => job.id),
    canRetry: selectedJobs.length >= 2,
  };
}

export function bulkCancelConfirmPlan(
  jobs: DownloadJob[],
  state: BulkUiState | null,
): BulkCancelConfirmPlan {
  const cancelJobIds = state === 'review' || state === 'downloading'
    ? jobs.filter(isCancelableBulkCancelJob).map((job) => job.id).filter(Boolean)
    : [];
  const deleteJobIds = state === 'review' || state === 'downloading'
    ? jobs.map((job) => job.id).filter(Boolean)
    : [];

  return {
    cancelJobIds,
    deleteJobIds,
    deleteFromDisk: deleteJobIds.length > 0,
    closeOnSuccess: state === 'review' || state === 'downloading',
  };
}

export function bulkFinalizingSteps(jobs: DownloadJob[]): BulkFinalizingStep[] {
  const archive = jobs.find((job) => job.bulkArchive)?.bulkArchive;
  const shouldShowUncompressing = archive?.requiresExtraction === true || archive?.archiveStatus === 'extracting';
  const steps: BulkFinalizingStep[] = [];

  if (shouldShowUncompressing) {
    steps.push({ id: 'uncompressing', label: 'Uncompressing' });
  }

  steps.push({ id: 'combining', label: 'Combining' });

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
  const failedItems = batchResult.failedItems ?? [];

  if (jobIds.length === 0 && failedItems.length === 0) return null;

  const isBulkArchive = Boolean(archiveName) && jobIds.length > 1;
  return {
    type: 'batch',
    context: {
      kind: isBulkArchive ? 'bulk' : 'multi',
      jobIds,
      title: 'Bulk download progress',
      ...(isBulkArchive && archiveName ? { archiveName } : {}),
      ...(failedItems.length > 0 ? { failedItems } : {}),
    },
  };
}

function clampProgress(value: number) {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, value));
}

function createProgressBatchId() {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `batch_${crypto.randomUUID()}`;
  }
  return `batch_${Date.now()}_${Math.random().toString(36).slice(2)}`;
}

function bulkArchiveStatus(job: DownloadJob): BulkArchiveStatus | null {
  if (!job.bulkArchive) return null;
  return job.bulkArchive.archiveStatus ?? 'pending';
}

function hasPendingHosterPreflight(job: DownloadJob): boolean {
  return job.hosterPreflight?.status === 'checking' || job.hosterPreflight?.status === 'unchecked';
}

function isResumableReviewJob(job: DownloadJob) {
  return ['paused', 'failed', 'canceled'].includes(job.state);
}

function isCancelableBulkCancelJob(job: DownloadJob) {
  return ['queued', 'starting', 'downloading', 'seeding', 'paused', 'failed'].includes(job.state);
}
