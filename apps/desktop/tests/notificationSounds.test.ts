import assert from 'node:assert/strict';
import {
  NOTIFICATION_SOUND_ASSETS,
  canPlayNotificationSound,
  createNotificationSoundPlayer,
  createUpdateNotificationSoundGate,
} from '../src/notificationSounds.ts';
import type { NotificationSoundKind } from '../src/notificationSounds.ts';

const soundKinds: NotificationSoundKind[] = ['success', 'failed', 'update'];

assert.deepEqual(
  Object.keys(NOTIFICATION_SOUND_ASSETS).sort(),
  [...soundKinds].sort(),
  'notification sound assets should cover success, failed, and update sounds',
);

for (const kind of soundKinds) {
  const assetPath = NOTIFICATION_SOUND_ASSETS[kind];
  assert.match(assetPath, /\.mp3$/, `${kind} notification sound should use an mp3 asset`);
  assert.doesNotMatch(assetPath, /\.flac$/i, `${kind} notification sound should not point at a flac asset`);
}

const settings = {
  notificationsEnabled: true,
  notificationSoundsEnabled: true,
};
assert.equal(settings.notificationSoundsEnabled, true, 'custom notification sounds should default on');
assert.equal(canPlayNotificationSound(settings), true, 'sound playback should be enabled when notifications and sounds are both enabled');
assert.equal(
  canPlayNotificationSound({ ...settings, notificationsEnabled: false }),
  false,
  'sound playback should stop when desktop notifications are disabled',
);
assert.equal(
  canPlayNotificationSound({ ...settings, notificationSoundsEnabled: false }),
  false,
  'sound playback should stop when notification sounds are disabled',
);

const playedSources: string[] = [];
const player = createNotificationSoundPlayer((src) => ({
  currentTime: 0,
  preload: '',
  play: () => {
    playedSources.push(src);
    return Promise.resolve();
  },
}));

assert.equal(player.play('failed', settings), true, 'enabled player should play failure sounds');
assert.deepEqual(playedSources, [NOTIFICATION_SOUND_ASSETS.failed]);
assert.equal(
  player.play('success', { ...settings, notificationSoundsEnabled: false }),
  false,
  'disabled player should report that no sound was played',
);
assert.deepEqual(playedSources, [NOTIFICATION_SOUND_ASSETS.failed], 'disabled playback should not call Audio.play');

const updateGate = createUpdateNotificationSoundGate();
assert.equal(updateGate.play('0.8.0-beta', settings, player), true, 'first update version should play once');
assert.equal(updateGate.play('0.8.0-beta', settings, player), false, 'same update version should not replay');
assert.equal(updateGate.play('0.8.1-beta', settings, player), true, 'a newer update version should play again');
assert.deepEqual(
  playedSources,
  [
    NOTIFICATION_SOUND_ASSETS.failed,
    NOTIFICATION_SOUND_ASSETS.update,
    NOTIFICATION_SOUND_ASSETS.update,
  ],
  'update sound gate should only add one sound per version',
);
