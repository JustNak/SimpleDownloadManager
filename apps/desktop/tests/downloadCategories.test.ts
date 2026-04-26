import assert from 'node:assert/strict';
import {
  categoryFolderForFilename,
  categoryForFilename,
  countJobsByCategory,
  DOWNLOAD_CATEGORIES,
  filterJobsByCategory,
} from '../src/downloadCategories.ts';
import type { DownloadJob } from '../src/types.ts';

const baseJob: DownloadJob = {
  id: 'job_1',
  url: 'https://example.com/file.bin',
  filename: 'file.bin',
  transferKind: 'http',
  state: 'queued',
  progress: 0,
  totalBytes: 0,
  downloadedBytes: 0,
  speed: 0,
  eta: 0,
};

function job(id: string, filename: string): DownloadJob {
  return { ...baseJob, id, filename };
}

assert.deepEqual(
  DOWNLOAD_CATEGORIES.map((category) => category.folderName),
  ['Document', 'Program', 'Picture', 'Video', 'Compressed', 'Music', 'Other'],
  'sidebar category folders should stay in the requested display order',
);

assert.deepEqual(
  DOWNLOAD_CATEGORIES.map((category) => [category.id, category.iconName]),
  [
    ['document', 'document'],
    ['program', 'program'],
    ['picture', 'picture'],
    ['video', 'video'],
    ['compressed', 'compressed'],
    ['music', 'music'],
    ['other', 'other'],
  ],
  'sidebar category branches should expose specific icon identities instead of sharing a generic folder icon',
);

assert.equal(categoryForFilename('report.PDF'), 'document');
assert.equal(categoryForFilename('installer.msi'), 'program');
assert.equal(categoryForFilename('photo.webp'), 'picture');
assert.equal(categoryForFilename('movie.mkv'), 'video');
assert.equal(categoryForFilename('archive.7z'), 'compressed');
assert.equal(categoryForFilename('album.flac'), 'music');
assert.equal(categoryForFilename('unknown.custom'), 'other');
assert.equal(categoryForFilename('no-extension'), 'other');
assert.equal(categoryFolderForFilename('album.flac'), 'Music');
assert.equal(categoryFolderForFilename('unknown.custom'), 'Other');

const jobs = [
  job('job_1', 'manual.pdf'),
  job('job_2', 'setup.exe'),
  job('job_3', 'cover.png'),
  job('job_4', 'clip.mp4'),
  job('job_5', 'assets.zip'),
  job('job_6', 'song.mp3'),
  job('job_7', 'payload.unknown'),
];

assert.deepEqual(
  countJobsByCategory(jobs),
  {
    document: 1,
    program: 1,
    picture: 1,
    video: 1,
    compressed: 1,
    music: 1,
    other: 1,
  },
  'category counts should classify every visible job',
);

assert.deepEqual(
  filterJobsByCategory(jobs, 'music').map((item) => item.id),
  ['job_6'],
  'category filters should return only matching jobs',
);
