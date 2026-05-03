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
assert.match(settingsPageSource, /const draft = dirty \? completeSettingsDraft\(\) : null[\s\S]*onDirtyChange\(dirty, draft\)/, 'SettingsPage should report complete dirty Svelte drafts to the app shell');
assert.match(settingsPageSource, /function completeSettingsDraft\(\): Settings[\s\S]*accentColor: normalizeAccentColor\(accentColorInput\)/, 'SettingsPage should include the normalized accent input in the live settings draft');
assert.match(settingsPageSource, /const draft = dirty \? completeSettingsDraft\(\) : null[\s\S]*const draftKey = draft \? JSON\.stringify\(draft\) : ''/, 'SettingsPage should key dirty draft updates from the complete draft including accent changes');
assert.match(settingsPageSource, /const draft = completeSettingsDraft\(\);[\s\S]*onSave\(\{[\s\S]*\.\.\.draft/, 'saving settings should use the same complete draft that powers live accent preview');
assert.match(appSource, /const liveSettings = \$derived\(settingsDraft \?\? settings\)/, 'App should keep a live settings preview while the settings form is dirty');
assert.match(appSource, /const nextAppearance = liveSettings[\s\S]*applyAppearance\(nextAppearance\)/, 'theme and accent changes should apply live from the settings draft');
assert.match(appSource, /showDetailsOnClick=\{liveSettings\.showDetailsOnClick\}/, 'details pane click behavior should preview from the live settings draft');
assert.match(appSource, /queueRowSize=\{liveSettings\.queueRowSize\}/, 'queue row density should preview from the live settings draft');
assert.match(appSource, /view === 'settings' && settingsDirty[\s\S]*isUnsavedSettingsPromptOpen = true/, 'App should guard navigation away from dirty settings drafts');
