import type { DownloadJob } from './types';

const SAMPLE_WINDOW_MS = 60_000;
const MIN_SAMPLE_ELAPSED_MS = 1_000;
const STARTUP_BACKEND_SPEED_WINDOW_MS = 5_000;

export interface ProgressSample {
  jobId: string;
  timestamp: number;
  downloadedBytes: number;
}

export interface DownloadProgressMetrics {
  averageSpeed: number;
  timeRemaining: number;
}

interface ProgressSampleRange {
  first: ProgressSample;
  last: ProgressSample;
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

export function recordProgressSamples(
  samples: ProgressSample[],
  jobs: readonly DownloadJob[],
  timestamp = Date.now(),
): ProgressSample[] {
  const activeJobs = new Map<string, DownloadJob>();
  for (const job of jobs) {
    if (job.state === 'downloading' && !isTorrentMetadataPendingForProgress(job)) {
      activeJobs.set(job.id, job);
    }
  }

  if (activeJobs.size === 0) return [];

  const cutoff = timestamp - SAMPLE_WINDOW_MS;
  const retainedSamples = samples.filter((sample) => (
    activeJobs.has(sample.jobId)
    && sample.timestamp >= cutoff
    && sample.timestamp !== timestamp
  ));

  for (const job of jobs) {
    if (!activeJobs.has(job.id)) continue;
    retainedSamples.push({
      jobId: job.id,
      timestamp,
      downloadedBytes: Math.max(0, job.downloadedBytes || 0),
    });
  }

  return retainedSamples;
}

export function calculateDownloadProgressMetrics(
  job: DownloadJob,
  samples: ProgressSample[],
  _timestamp = Date.now(),
): DownloadProgressMetrics {
  return calculateDownloadProgressMetricsFromObservedSpeed(
    job,
    observedAverageSpeedForJob(job.id, samples),
  );
}

function calculateDownloadProgressMetricsFromObservedSpeed(
  job: DownloadJob,
  observedSpeed: { speed: number; elapsedMs: number },
): DownloadProgressMetrics {
  const backendSpeed = Math.max(0, job.speed || 0);
  const averageSpeed = job.transferKind === 'torrent'
    ? backendSpeed
    : httpAverageSpeed(backendSpeed, observedSpeed);
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

function httpAverageSpeed(
  backendSpeed: number,
  observedSpeed: { speed: number; elapsedMs: number },
): number {
  if (
    backendSpeed > 0
    && observedSpeed.elapsedMs > 0
    && observedSpeed.elapsedMs <= STARTUP_BACKEND_SPEED_WINDOW_MS
  ) {
    return backendSpeed;
  }
  return observedSpeed.speed || backendSpeed;
}

export function calculateDownloadProgressMetricsByJobId(
  jobs: DownloadJob[],
  samples: ProgressSample[],
  _timestamp = Date.now(),
): Record<string, DownloadProgressMetrics> {
  const samplesByJobId = summarizeProgressSamplesByJobId(samples);
  return Object.fromEntries(
    jobs.map((job) => [
      job.id,
      calculateDownloadProgressMetricsFromObservedSpeed(
        job,
        observedAverageSpeedFromRange(samplesByJobId.get(job.id)),
      ),
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

function observedAverageSpeedForJob(jobId: string, samples: ProgressSample[]): { speed: number; elapsedMs: number } {
  return observedAverageSpeedFromRange(selectProgressSampleRangeForJob(jobId, samples));
}

function observedAverageSpeedFromRange(range: ProgressSampleRange | undefined): { speed: number; elapsedMs: number } {
  if (!range) return { speed: 0, elapsedMs: 0 };

  const { first, last } = range;
  const elapsedMs = last.timestamp - first.timestamp;
  const byteDelta = last.downloadedBytes - first.downloadedBytes;
  if (elapsedMs < MIN_SAMPLE_ELAPSED_MS || byteDelta <= 0) return { speed: 0, elapsedMs };

  return { speed: Math.round(byteDelta / (elapsedMs / 1000)), elapsedMs };
}

function selectProgressSampleRangeForJob(jobId: string, samples: ProgressSample[]): ProgressSampleRange | undefined {
  let range: ProgressSampleRange | undefined;
  for (const sample of samples) {
    if (sample.jobId !== jobId) continue;
    range = mergeProgressSampleRange(range, sample);
  }
  return range;
}

function summarizeProgressSamplesByJobId(samples: ProgressSample[]): Map<string, ProgressSampleRange> {
  const ranges = new Map<string, ProgressSampleRange>();
  for (const sample of samples) {
    ranges.set(sample.jobId, mergeProgressSampleRange(ranges.get(sample.jobId), sample));
  }
  return ranges;
}

function mergeProgressSampleRange(
  range: ProgressSampleRange | undefined,
  sample: ProgressSample,
): ProgressSampleRange {
  if (!range) {
    return { first: sample, last: sample };
  }

  return {
    first: sample.timestamp < range.first.timestamp ? sample : range.first,
    last: sample.timestamp >= range.last.timestamp ? sample : range.last,
  };
}

function isTorrentMetadataPendingForProgress(job: DownloadJob): boolean {
  if (job.transferKind !== 'torrent') return false;
  if (job.state !== 'starting' && job.state !== 'downloading') return false;
  if ((job.totalBytes ?? 0) > 0) return false;
  return !job.torrent?.name;
}
