import assert from 'node:assert/strict';
import {
  filterJobsForView,
  getQueueCounts,
  getTorrentFooterStats,
  isTorrentView,
} from '../src/downloadViews.ts';
import type { DownloadJob } from '../src/types.ts';

const baseJob: DownloadJob = {
  id: 'job_1',
  url: 'https://example.com/file.zip',
  filename: 'file.zip',
  transferKind: 'http',
  state: 'queued',
  progress: 0,
  totalBytes: 0,
  downloadedBytes: 0,
  speed: 0,
  eta: 0,
};

const jobs: DownloadJob[] = [
  { ...baseJob, id: 'http_active', filename: 'app.exe', state: 'downloading', progress: 50, downloadedBytes: 50 },
  { ...baseJob, id: 'http_done', filename: 'guide.pdf', state: 'completed', progress: 100, downloadedBytes: 100, totalBytes: 100 },
  { ...baseJob, id: 'http_failed', filename: 'broken.zip', state: 'failed', failureCategory: 'network' },
  {
    ...baseJob,
    id: 'torrent_active',
    filename: 'linux.iso',
    url: 'magnet:?xt=urn:btih:active',
    transferKind: 'torrent',
    state: 'downloading',
  },
  {
    ...baseJob,
    id: 'torrent_seed',
    filename: 'movie.torrent',
    url: 'magnet:?xt=urn:btih:seed',
    transferKind: 'torrent',
    state: 'seeding',
    speed: 4096,
    torrent: { uploadedBytes: 1024, ratio: 1.4 },
  },
  {
    ...baseJob,
    id: 'torrent_failed',
    filename: 'failed.torrent',
    url: 'magnet:?xt=urn:btih:failed',
    transferKind: 'torrent',
    state: 'failed',
    failureCategory: 'torrent',
  },
];

assert.deepEqual(
  getQueueCounts(jobs),
  {
    all: 3,
    active: 1,
    attention: 1,
    queued: 0,
    completed: 1,
    categories: {
      document: 1,
      program: 1,
      picture: 0,
      video: 0,
      compressed: 1,
      music: 0,
      other: 0,
    },
    torrents: {
      all: 3,
      active: 1,
      seeding: 1,
      attention: 1,
      queued: 0,
      completed: 0,
    },
  },
  'regular download counts should exclude torrents and torrent counts should be isolated',
);

assert.deepEqual(
  getTorrentFooterStats(jobs),
  {
    all: 3,
    active: 1,
    seeding: 1,
    uploadedBytes: 1024,
    seedSpeed: 4096,
    averageRatio: 1.4,
  },
  'torrent footer stats should summarize live seeding upload speed without sharing regular download counts',
);

assert.deepEqual(
  filterJobsForView(jobs, 'all').map((job) => job.id),
  ['http_active', 'http_done', 'http_failed'],
  'all downloads view should exclude torrent jobs',
);

assert.deepEqual(
  filterJobsForView(jobs, 'torrents').map((job) => job.id),
  ['torrent_active', 'torrent_seed', 'torrent_failed'],
  'torrent view should include only torrent jobs',
);

assert.deepEqual(
  filterJobsForView(jobs, 'torrent-seeding').map((job) => job.id),
  ['torrent_seed'],
  'torrent seeding view should include only seeding torrents',
);

assert.deepEqual(
  filterJobsForView(jobs, 'attention').map((job) => job.id),
  ['http_failed'],
  'regular attention view should exclude torrent failures',
);

assert.deepEqual(
  filterJobsForView(jobs, 'torrent-attention').map((job) => job.id),
  ['torrent_failed'],
  'torrent attention view should include only torrent failures',
);

assert.equal(isTorrentView('torrents'), true);
assert.equal(isTorrentView('all'), false);
