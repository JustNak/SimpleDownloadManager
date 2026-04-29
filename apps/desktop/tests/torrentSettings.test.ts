import assert from 'node:assert/strict';
import { normalizeTorrentSettings, shouldStopSeeding } from '../src/torrentSettings.ts';
import type { TorrentSettings } from '../src/types.ts';

const defaults = normalizeTorrentSettings({});

assert.deepEqual(
  defaults,
  {
    enabled: true,
    downloadDirectory: '',
    seedMode: 'forever',
    seedRatioLimit: 1,
    seedTimeLimitMinutes: 60,
    uploadLimitKibPerSecond: 0,
    portForwardingEnabled: false,
    portForwardingPort: 42000,
  },
  'torrent settings should default to enabled unlimited seeding with upload cap and port forwarding disabled',
);

assert.equal(
  normalizeTorrentSettings({}, 'E:\\Downloads').downloadDirectory,
  'E:\\Downloads\\Torrent',
  'torrent settings should default the torrent directory from the main download directory',
);

assert.equal(
  normalizeTorrentSettings({ downloadDirectory: ' D:\\Torrents ' }, 'E:\\Downloads').downloadDirectory,
  'D:\\Torrents',
  'torrent settings should preserve a configured torrent directory',
);

assert.deepEqual(
  normalizeTorrentSettings({
    enabled: true,
    downloadDirectory: '',
    seedMode: 'ratio',
    seedRatioLimit: 0,
    seedTimeLimitMinutes: -20,
    uploadLimitKibPerSecond: -1,
    portForwardingEnabled: true,
    portForwardingPort: 80,
  }, 'E:\\Downloads'),
  {
    enabled: true,
    downloadDirectory: 'E:\\Downloads\\Torrent',
    seedMode: 'ratio',
    seedRatioLimit: 0.1,
    seedTimeLimitMinutes: 1,
    uploadLimitKibPerSecond: 0,
    portForwardingEnabled: true,
    portForwardingPort: 42000,
  },
  'torrent settings should clamp invalid numeric limits',
);

assert.deepEqual(
  normalizeTorrentSettings({
    enabled: true,
    downloadDirectory: 'D:\\Torrents',
    uploadLimitKibPerSecond: 10_000_000,
    portForwardingEnabled: true,
    portForwardingPort: 43000,
  }),
  {
    enabled: true,
    downloadDirectory: 'D:\\Torrents',
    seedMode: 'forever',
    seedRatioLimit: 1,
    seedTimeLimitMinutes: 60,
    uploadLimitKibPerSecond: 1_048_576,
    portForwardingEnabled: true,
    portForwardingPort: 43000,
  },
  'torrent settings should keep valid forwarding ports and cap upload limit to a safe maximum',
);

const ratioPolicy: TorrentSettings = {
  ...defaults,
  seedMode: 'ratio',
  seedRatioLimit: 1.5,
};

assert.equal(shouldStopSeeding(ratioPolicy, 1.49, 3600), false);
assert.equal(shouldStopSeeding(ratioPolicy, 1.5, 3600), true);

const timePolicy: TorrentSettings = {
  ...defaults,
  seedMode: 'time',
  seedTimeLimitMinutes: 30,
};

assert.equal(shouldStopSeeding(timePolicy, 0.25, 29 * 60), false);
assert.equal(shouldStopSeeding(timePolicy, 0.25, 30 * 60), true);
