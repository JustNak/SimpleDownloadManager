import assert from 'node:assert/strict';
import { normalizeTorrentSettings, shouldStopSeeding } from '../src/torrentSettings.ts';
import type { TorrentSettings } from '../src/types.ts';

const defaults = normalizeTorrentSettings({});

assert.deepEqual(
  defaults,
  {
    enabled: true,
    seedMode: 'forever',
    seedRatioLimit: 1,
    seedTimeLimitMinutes: 60,
    uploadLimitKibPerSecond: 0,
  },
  'torrent settings should default to enabled unlimited seeding with upload cap disabled',
);

assert.deepEqual(
  normalizeTorrentSettings({
    enabled: true,
    seedMode: 'ratio',
    seedRatioLimit: 0,
    seedTimeLimitMinutes: -20,
    uploadLimitKibPerSecond: -1,
  }),
  {
    enabled: true,
    seedMode: 'ratio',
    seedRatioLimit: 0.1,
    seedTimeLimitMinutes: 1,
    uploadLimitKibPerSecond: 0,
  },
  'torrent settings should clamp invalid numeric limits',
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
