import { detectMultipartArchivePart } from './bulkArchiveNaming.ts';
import type { BulkArchiveStatus, DownloadJob } from './types';

export interface BulkAggregateDownloadJob extends DownloadJob {
  bulkAggregate: true;
  bulkMemberIds: string[];
  bulkArchiveId: string;
  bulkArchiveOutputPath?: string;
  bulkRetryableMemberCount: number;
  bulkArchiveFixable: boolean;
}

export type QueueDisplayJob = DownloadJob | BulkAggregateDownloadJob;
export type BulkMembersByArchiveId = Record<string, DownloadJob[]>;

const QUEUED = 'queued' as DownloadJob['state'];
const STARTING = 'starting' as DownloadJob['state'];
const DOWNLOADING = 'downloading' as DownloadJob['state'];
const PAUSED = 'paused' as DownloadJob['state'];
const COMPLETED = 'completed' as DownloadJob['state'];
const FAILED = 'failed' as DownloadJob['state'];
const CANCELED = 'canceled' as DownloadJob['state'];

const activeStates = new Set<string>([
  QUEUED,
  STARTING,
  DOWNLOADING,
  'seeding',
]);

export function groupBulkQueueRows(jobs: readonly DownloadJob[]): QueueDisplayJob[] {
  const rows: Array<QueueDisplayJob | null> = [];
  const groups = new Map<string, DownloadJob[]>();
  const groupRowIndexes = new Map<string, number>();

  for (const job of jobs) {
    const archive = job.transferKind === 'http' ? job.bulkArchive : undefined;
    if (!archive?.id) {
      rows.push(job);
      continue;
    }

    const existing = groups.get(archive.id);
    if (existing) {
      existing.push(job);
      continue;
    }

    const group = [job];
    groups.set(archive.id, group);
    groupRowIndexes.set(archive.id, rows.length);
    rows.push(buildBulkAggregateRow(group));
  }

  for (const [archiveId, rowIndex] of groupRowIndexes) {
    const members = groups.get(archiveId);
    if (members) rows[rowIndex] = buildBulkAggregateRow(members);
  }

  return rows.filter((row): row is QueueDisplayJob => row !== null);
}

export function groupBulkMembersByArchiveId(jobs: readonly DownloadJob[]): BulkMembersByArchiveId {
  const groups: BulkMembersByArchiveId = {};

  for (const job of jobs) {
    const archive = job.transferKind === 'http' ? job.bulkArchive : undefined;
    if (!archive?.id) continue;
    (groups[archive.id] ??= []).push(job);
  }

  for (const archiveId of Object.keys(groups)) {
    groups[archiveId] = orderedBulkMembers(groups[archiveId]);
  }

  return groups;
}

export function isBulkAggregateJob(job: DownloadJob | QueueDisplayJob): job is BulkAggregateDownloadJob {
  return (job as Partial<BulkAggregateDownloadJob>).bulkAggregate === true;
}

function buildBulkAggregateRow(members: readonly DownloadJob[]): BulkAggregateDownloadJob {
  const orderedMembers = orderedBulkMembers(members);
  const first = orderedMembers[0];
  const archive = orderedMembers.find((job) => job.bulkArchive)?.bulkArchive;
  const totalBytes = sum(orderedMembers, (job) => Math.max(0, job.totalBytes));
  const downloadedBytes = sum(orderedMembers, (job) => Math.max(0, job.downloadedBytes));
  const speed = sum(
    orderedMembers.filter((job) => job.state === DOWNLOADING),
    (job) => Math.max(0, job.speed),
  );
  const progress = deriveAggregateProgress(
    orderedMembers,
    downloadedBytes,
    totalBytes,
    archive?.archiveStatus,
    archive?.finalizeProcessedBytes,
    archive?.finalizeTotalBytes,
  );
  const state = deriveAggregateState(orderedMembers, archive?.archiveStatus);
  const removalState = deriveAggregateRemovalState(orderedMembers);
  const remainingBytes = Math.max(0, totalBytes - downloadedBytes);

  return {
    ...first,
    id: `bulk:${archive?.id ?? first.id}`,
    url: first.url,
    filename: archive?.name?.trim() || 'bulk-download.zip',
    source: undefined,
    transferKind: 'http',
    state,
    removalState,
    createdAt: firstCreatedAt(members),
    progress,
    totalBytes,
    downloadedBytes: state === COMPLETED && totalBytes > 0 ? totalBytes : downloadedBytes,
    speed,
    eta: speed > 0 && remainingBytes > 0 ? Math.ceil(remainingBytes / speed) : 0,
    error: archive?.error ?? members.find((job) => job.error)?.error,
    failureCategory: members.find((job) => job.failureCategory)?.failureCategory,
    targetPath: archive?.archiveStatus === 'completed' ? archive.outputPath : undefined,
    tempPath: '',
    artifactExists: undefined,
    bulkArchive: archive,
    bulkAggregate: true,
    bulkMemberIds: orderedMembers.map((job) => job.id),
    bulkArchiveId: archive?.id ?? first.id,
    bulkArchiveOutputPath: archive?.outputPath,
    bulkRetryableMemberCount: orderedMembers.filter(isRetryableBulkMember).length,
    bulkArchiveFixable: isFixableBulkArchive(orderedMembers, archive?.archiveStatus),
  };
}

function deriveAggregateRemovalState(members: readonly DownloadJob[]): DownloadJob['removalState'] {
  if (members.some((job) => job.removalState === 'removing') && !members.some(isActivelyDownloadableMember)) {
    return 'removing';
  }
  if (members.some((job) => job.removalState === 'cleanup_failed')) {
    return 'cleanup_failed';
  }
  return undefined;
}

function isActivelyDownloadableMember(job: DownloadJob): boolean {
  return job.removalState !== 'removing' && activeStates.has(job.state);
}

function isRetryableBulkMember(job: DownloadJob): boolean {
  return job.transferKind === 'http'
    && job.state === FAILED
    && job.bulkArchive?.archiveStatus === 'pending';
}

function isFixableBulkArchive(
  members: readonly DownloadJob[],
  archiveStatus: BulkArchiveStatus | undefined,
): boolean {
  return archiveStatus === FAILED
    && members.length >= 2
    && members.every((job) => job.transferKind === 'http' && job.state === COMPLETED);
}

function deriveAggregateState(
  members: readonly DownloadJob[],
  archiveStatus: BulkArchiveStatus | undefined,
): DownloadJob['state'] {
  if (archiveStatus === 'failed' || members.some((job) => job.state === FAILED)) return FAILED;
  if (archiveStatus === 'completed') return COMPLETED;
  if (archiveStatus === 'extracting' || archiveStatus === 'combining' || archiveStatus === 'creating_folder' || archiveStatus === 'compressing') return DOWNLOADING;
  if (members.some((job) => job.state === DOWNLOADING)) return DOWNLOADING;
  if (members.some((job) => job.state === STARTING)) return STARTING;
  if (members.some((job) => job.state === QUEUED)) return QUEUED;
  if (members.some((job) => job.state === PAUSED)) return PAUSED;
  if (members.every((job) => job.state === COMPLETED)) return DOWNLOADING;
  if (members.every((job) => job.state === CANCELED)) return CANCELED;
  if (members.some((job) => activeStates.has(job.state))) return DOWNLOADING;
  return members[0]?.state ?? QUEUED;
}

function deriveAggregateProgress(
  members: readonly DownloadJob[],
  downloadedBytes: number,
  totalBytes: number,
  archiveStatus: BulkArchiveStatus | undefined,
  finalizeProcessedBytes: number | undefined,
  finalizeTotalBytes: number | undefined,
): number {
  if (isFinalizingBulkArchive(archiveStatus) && (finalizeTotalBytes ?? 0) > 0) {
    return clampProgress(((finalizeProcessedBytes ?? 0) / (finalizeTotalBytes ?? 0)) * 100);
  }
  if (totalBytes > 0) return clampProgress((downloadedBytes / totalBytes) * 100);
  if (members.length === 0) return 0;
  const terminalCount = members.filter((job) => (
    job.state === COMPLETED
    || job.state === FAILED
    || job.state === CANCELED
  )).length;
  return clampProgress((terminalCount / members.length) * 100);
}

function isFinalizingBulkArchive(status: BulkArchiveStatus | undefined): boolean {
  return status === 'extracting'
    || status === 'combining'
    || status === 'creating_folder'
    || status === 'compressing';
}

function firstCreatedAt(members: readonly DownloadJob[]): number | undefined {
  const timestamps = members
    .map((job) => job.createdAt)
    .filter((value): value is number => typeof value === 'number' && Number.isFinite(value) && value > 0);
  return timestamps.length > 0 ? Math.min(...timestamps) : undefined;
}

function clampProgress(progress: number): number {
  if (!Number.isFinite(progress)) return 0;
  return Math.max(0, Math.min(100, progress));
}

function sum<T>(items: readonly T[], getValue: (item: T) => number): number {
  return items.reduce((total, item) => total + getValue(item), 0);
}

function orderedBulkMembers(members: readonly DownloadJob[]): DownloadJob[] {
  return [...members].sort(compareBulkMembers);
}

function compareBulkMembers(left: DownloadJob, right: DownloadJob): number {
  const leftPart = detectMultipartArchivePart(left.filename);
  const rightPart = detectMultipartArchivePart(right.filename);

  if (leftPart && rightPart) {
    const keyOrder = leftPart.key.localeCompare(rightPart.key);
    if (keyOrder !== 0) return keyOrder;
    if (leftPart.partNumber !== rightPart.partNumber) {
      return leftPart.partNumber - rightPart.partNumber;
    }
  } else if (leftPart || rightPart) {
    return leftPart ? -1 : 1;
  }

  const createdOrder = (left.createdAt ?? 0) - (right.createdAt ?? 0);
  if (createdOrder !== 0) return createdOrder;
  const filenameOrder = left.filename.localeCompare(right.filename);
  if (filenameOrder !== 0) return filenameOrder;
  return left.id.localeCompare(right.id);
}
