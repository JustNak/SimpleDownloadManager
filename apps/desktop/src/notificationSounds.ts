import type { Settings } from './types';

export type NotificationSoundKind = 'success' | 'failed' | 'update';
export type NotificationSoundSettings = Pick<Settings, 'notificationSoundsEnabled'>;

export interface NotificationAudio {
  currentTime: number;
  preload: string;
  play: () => Promise<void> | void;
}

export type NotificationAudioFactory = (src: string) => NotificationAudio;

export const NOTIFICATION_SOUND_ASSETS: Record<NotificationSoundKind, string> = {
  success: '/sounds/notification-success.mp3',
  failed: '/sounds/notification-failed.mp3',
  update: '/sounds/notification-update.mp3',
};

export function canPlayNotificationSound(settings: NotificationSoundSettings): boolean {
  return settings.notificationSoundsEnabled;
}

export function createNotificationSoundPlayer(
  createAudio: NotificationAudioFactory = (src) => {
    const audio = new Audio(src);
    audio.preload = 'auto';
    return audio;
  },
) {
  const audioByKind = new Map<NotificationSoundKind, NotificationAudio>();

  function audioFor(kind: NotificationSoundKind): NotificationAudio {
    const cached = audioByKind.get(kind);
    if (cached) return cached;

    const audio = createAudio(NOTIFICATION_SOUND_ASSETS[kind]);
    audio.preload = 'auto';
    audioByKind.set(kind, audio);
    return audio;
  }

  return {
    preload(): void {
      for (const kind of Object.keys(NOTIFICATION_SOUND_ASSETS) as NotificationSoundKind[]) {
        audioFor(kind);
      }
    },

    play(kind: NotificationSoundKind, settings: NotificationSoundSettings): boolean {
      if (!canPlayNotificationSound(settings)) return false;

      try {
        const audio = audioFor(kind);
        audio.currentTime = 0;
        const result = audio.play();
        if (result && typeof result.catch === 'function') {
          result.catch(() => undefined);
        }
        return true;
      } catch {
        return false;
      }
    },
  };
}

export type NotificationSoundPlayer = ReturnType<typeof createNotificationSoundPlayer>;

export function createUpdateNotificationSoundGate() {
  let lastVersion: string | null = null;

  return {
    play(version: string | undefined | null, settings: NotificationSoundSettings, player: NotificationSoundPlayer): boolean {
      const normalizedVersion = version?.trim();
      if (!normalizedVersion || normalizedVersion === lastVersion) return false;

      lastVersion = normalizedVersion;
      return player.play('update', settings);
    },
  };
}
