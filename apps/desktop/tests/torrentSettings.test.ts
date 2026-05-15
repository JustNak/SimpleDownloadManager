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
    peerConnectionWatchdogMode: 'assist',
    customTrackers: [],
  },
  'torrent settings should default to enabled unlimited seeding with safe peer assist',
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
    peerConnectionWatchdogMode: 'invalid',
    customTrackers: [
      ' https://tracker.example/announce ',
      'HTTPS://tracker.example/announce',
      'udp://tracker.example:1337/announce',
      'ftp://tracker.example/announce',
    ],
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
    peerConnectionWatchdogMode: 'assist',
    customTrackers: [
      'https://tracker.example/announce',
      'udp://tracker.example:1337/announce',
    ],
  },
  'torrent settings should clamp invalid numeric limits and normalize custom trackers',
);

assert.equal(
  normalizeTorrentSettings({ peerConnectionWatchdogMode: 'diagnose' }).peerConnectionWatchdogMode,
  'diagnose',
  'torrent settings should keep diagnose-only mode as an explicit option',
);

assert.deepEqual(
  normalizeTorrentSettings({
    enabled: true,
    downloadDirectory: 'D:\\Torrents',
    uploadLimitKibPerSecond: 10_000_000,
    portForwardingEnabled: true,
    portForwardingPort: 43000,
    peerConnectionWatchdogMode: 'experimental',
    customTrackers: Array.from({ length: 70 }, (_, index) => `https://tracker-${index}.example/announce`),
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
    peerConnectionWatchdogMode: 'recover',
    customTrackers: Array.from({ length: 64 }, (_, index) => `https://tracker-${index}.example/announce`),
  },
  'torrent settings should keep valid forwarding ports, cap upload limit, and migrate experimental watchdog mode to recover',
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
