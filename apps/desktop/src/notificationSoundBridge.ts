import { subscribeToNotificationSound } from './backend';
import { createNotificationSoundPlayer } from './notificationSounds';

const notificationSoundPlayer = createNotificationSoundPlayer();
let bridgeStarted = false;

export function getNotificationSoundPlayer() {
  return notificationSoundPlayer;
}

export function startNotificationSoundBridge(): void {
  if (bridgeStarted) return;
  bridgeStarted = true;

  void subscribeToNotificationSound((event) => {
    notificationSoundPlayer.play(event.kind, { notificationSoundsEnabled: true });
  });
}
