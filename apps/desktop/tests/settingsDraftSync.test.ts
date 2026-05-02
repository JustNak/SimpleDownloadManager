import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { shouldAdoptIncomingSettingsDraft } from '../src/settingsDraftSync.ts';
import type { Settings } from '../src/types.ts';

function makeSettings(theme: Settings['theme']): Settings {
  return {
    downloadDirectory: 'C:\\Users\\You\\Downloads',
    maxConcurrentDownloads: 3,
    autoRetryAttempts: 3,
    speedLimitKibPerSecond: 0,
    downloadPerformanceMode: 'balanced',
    torrent: {
      enabled: true,
      downloadDirectory: 'C:\\Users\\You\\Downloads\\Torrent',
      seedMode: 'forever',
      seedRatioLimit: 1,
      seedTimeLimitMinutes: 60,
      uploadLimitKibPerSecond: 0,
      portForwardingEnabled: false,
      portForwardingPort: 42000,
      peerConnectionWatchdogMode: 'diagnose',
    },
    notificationsEnabled: true,
    theme,
    accentColor: '#3b82f6',
    showDetailsOnClick: true,
    queueRowSize: 'medium',
    startOnStartup: false,
    startupLaunchMode: 'open',
    extensionIntegration: {
      enabled: true,
      downloadHandoffMode: 'ask',
      listenPort: 1420,
      contextMenuEnabled: true,
      showProgressAfterHandoff: true,
      showBadgeStatus: true,
      excludedHosts: [],
      ignoredFileExtensions: [],
      authenticatedHandoffEnabled: false,
      authenticatedHandoffHosts: [],
    },
  };
}

const persistedOled = makeSettings('oled_dark');
const draftLight = makeSettings('light');

assert.equal(shouldAdoptIncomingSettingsDraft(persistedOled, persistedOled, makeSettings('oled_dark')), true, 'clean settings forms should adopt incoming backend settings');
assert.equal(shouldAdoptIncomingSettingsDraft(draftLight, persistedOled, makeSettings('oled_dark')), false, 'dirty Light theme drafts should not be overwritten by a fresh snapshot of the old persisted theme');
assert.equal(shouldAdoptIncomingSettingsDraft(draftLight, persistedOled, makeSettings('light')), true, 'settings forms can adopt incoming settings once the draft matches the saved value');

const settingsPageSource = readFileSync(new URL('../src/SettingsPage.svelte', import.meta.url), 'utf8');
const appSource = readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8');
assert.match(settingsPageSource, /onDirtyChange\(isDirty, isDirty \? cloneSettings\(formData\) : null\)/, 'SettingsPage should report dirty Svelte drafts to the app shell');
assert.match(appSource, /view === 'settings' && settingsDirty[\s\S]*isUnsavedSettingsPromptOpen = true/, 'App should guard navigation away from dirty settings drafts');
