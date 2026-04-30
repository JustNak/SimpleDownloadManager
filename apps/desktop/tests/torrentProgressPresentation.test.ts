import assert from 'node:assert/strict';
import {
  buildTorrentPeerHealthDots,
  torrentRemainingText,
  torrentSourceSummary,
} from '../src/torrentProgressPresentation.ts';
import type { DownloadJob } from '../src/types.ts';

const baseTorrentJob: DownloadJob = {
  id: 'torrent_1',
  url: 'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example&tr=udp%3A%2F%2Ftracker.example%2Fannounce&tr=https%3A%2F%2Ftracker.example%2Fannounce',
  filename: 'Example',
  transferKind: 'torrent',
  state: 'downloading',
  createdAt: 1_000,
  progress: 74,
  totalBytes: 4_400,
  downloadedBytes: 3_200,
  speed: 64,
  eta: 10,
  targetPath: 'D:\\Torrents\\Example',
  torrent: {
    infoHash: '0123456789abcdef0123456789abcdef01234567',
    name: 'Example',
    totalFiles: 3,
    peers: 28,
    seeds: 112,
    uploadedBytes: 512,
    fetchedBytes: 3_200,
    ratio: 0.18,
  },
};

assert.equal(
  torrentSourceSummary(baseTorrentJob),
  'DHT, 2 trackers',
  'magnet source summary should include DHT and tracker count from tr parameters',
);

assert.equal(
  torrentSourceSummary({ ...baseTorrentJob, url: 'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567' }),
  'Magnet link',
  'magnet source summary should fall back when no trackers are declared',
);

assert.equal(
  torrentSourceSummary({ ...baseTorrentJob, url: 'https://example.com/linux.iso.torrent' }),
  '.torrent URL',
  'http torrent source summary should identify torrent URLs',
);

assert.equal(
  torrentSourceSummary({ ...baseTorrentJob, url: 'C:\\Users\\You\\Downloads\\linux.iso.torrent' }),
  'Local .torrent file',
  'local torrent source summary should identify torrent files',
);

assert.equal(
  torrentRemainingText(baseTorrentJob, (bytes) => `${bytes} B`),
  '1200 B remaining',
  'remaining text should derive bytes from total minus downloaded bytes',
);

assert.equal(
  torrentRemainingText({ ...baseTorrentJob, totalBytes: 0 }, (bytes) => `${bytes} B`),
  'Unknown remaining',
  'remaining text should not invent a value before metadata resolves total size',
);

assert.deepEqual(
  buildTorrentPeerHealthDots(baseTorrentJob).map((dot) => dot.tone),
  ['success', 'success', 'success', 'success', 'success', 'success', 'warning', 'warning', 'muted', 'muted', 'muted', 'muted'],
  'peer health dots should summarize connected peers, seeds, and inactive capacity with theme tones',
);
