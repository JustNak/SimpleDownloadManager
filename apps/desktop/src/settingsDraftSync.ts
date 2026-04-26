import type { Settings } from './types';

export function shouldAdoptIncomingSettingsDraft(
  currentDraft: Settings,
  previousSettings: Settings,
  nextSettings: Settings,
): boolean {
  return settingsEqual(currentDraft, previousSettings) || settingsEqual(currentDraft, nextSettings);
}

export function settingsEqual(left: Settings, right: Settings): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}
