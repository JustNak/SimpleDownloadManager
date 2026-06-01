import { subscribeToNotificationSound } from './backend';
import { createNotificationSoundPlayer } from './notificationSounds';

export function startNotificationSoundBridge(): void {
  const player = createNotificationSoundPlayer();
  let preloaded = false;

  function preload(): void {
    if (preloaded) return;
    preloaded = true;
    player.preload();
  }

  window.addEventListener('pointerdown', preload, { once: true, passive: true });
  window.addEventListener('keydown', preload, { once: true });
  preload();

  void subscribeToNotificationSound((event) => {
    player.play(event.kind, { notificationSoundsEnabled: true });
  });
}
