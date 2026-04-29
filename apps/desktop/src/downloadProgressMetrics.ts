import type { DownloadJob } from './types';

const SAMPLE_WINDOW_MS = 60_000;
const MIN_SAMPLE_ELAPSED_MS = 1_000;

export interface ProgressSample {
  jobId: string;
  timestamp: number;
  downloadedBytes: number;
}

export interface DownloadProgressMetrics {
  averageSpeed: number;
  timeRemaining: number;
}

export function recordProgressSample(
  samples: ProgressSample[],
  job: DownloadJob,
  timestamp = Date.now(),
): ProgressSample[] {
  if (job.state !== 'downloading' || isTorrentMetadataPendingForProgress(job)) {
    return samples.filter((sample) => sample.jobId !== job.id);
  }

  const currentSample = {
    jobId: job.id,
    timestamp,
    downloadedBytes: Math.max(0, job.downloadedBytes || 0),
  };
  const cutoff = timestamp - SAMPLE_WINDOW_MS;
  const retainedSamples = samples.filter((sample) => {
    if (sample.jobId !== job.id) return true;
    return sample.timestamp >= cutoff && sample.timestamp !== timestamp;
  });

  return [...retainedSamples, currentSample];
}

export function calculateDownloadProgressMetrics(
  job: DownloadJob,
  samples: ProgressSample[],
  _timestamp = Date.now(),
): DownloadProgressMetrics {
  const backendSpeed = Math.max(0, job.speed || 0);
  const averageSpeed = job.transferKind === 'torrent'
    ? backendSpeed
    : observedAverageSpeed(job, samples) || backendSpeed;
  const remainingBytes = Math.max(0, (job.totalBytes || 0) - (job.downloadedBytes || 0));
  const timeRemaining = job.transferKind === 'torrent'
    ? torrentBackendTimeRemaining(job)
    : averageSpeed > 0 && remainingBytes > 0
      ? Math.ceil(remainingBytes / averageSpeed)
      : 0;

  return {
    averageSpeed,
    timeRemaining,
  };
}

export function calculateDownloadProgressMetricsByJobId(
  jobs: DownloadJob[],
  samples: ProgressSample[],
  timestamp = Date.now(),
): Record<string, DownloadProgressMetrics> {
  return Object.fromEntries(
    jobs.map((job) => [
      job.id,
      calculateDownloadProgressMetrics(job, samples, timestamp),
    ]),
  );
}

export function shouldShowCompletedFileAction(job: DownloadJob): boolean {
  return (job.state === 'completed' || job.state === 'seeding') && Boolean(job.targetPath);
}

function torrentBackendTimeRemaining(job: DownloadJob): number {
  if (job.state !== 'downloading' || isTorrentMetadataPendingForProgress(job)) return 0;
  return Math.max(0, Math.round(job.eta || 0));
}

function observedAverageSpeed(job: DownloadJob, samples: ProgressSample[]): number {
  const jobSamples = samples
    .filter((sample) => sample.jobId === job.id)
    .sort((left, right) => left.timestamp - right.timestamp);
  if (jobSamples.length < 2) return 0;

  const first = jobSamples[0];
  const last = jobSamples[jobSamples.length - 1];
  const elapsedMs = last.timestamp - first.timestamp;
  const byteDelta = last.downloadedBytes - first.downloadedBytes;
  if (elapsedMs < MIN_SAMPLE_ELAPSED_MS || byteDelta <= 0) return 0;

  return Math.round(byteDelta / (elapsedMs / 1000));
}

function isTorrentMetadataPendingForProgress(job: DownloadJob): boolean {
  if (job.transferKind !== 'torrent') return false;
  if (job.state !== 'starting' && job.state !== 'downloading') return false;
  if ((job.totalBytes ?? 0) > 0) return false;
  return !job.torrent?.name;
}
