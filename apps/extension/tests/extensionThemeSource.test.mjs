import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const popupHtml = readFileSync(new URL('../src/popup/index.html', import.meta.url), 'utf8');
const optionsHtml = readFileSync(new URL('../src/options/index.html', import.meta.url), 'utf8');
const buildSource = readFileSync(new URL('../scripts/build.mjs', import.meta.url), 'utf8');
const popupSource = readFileSync(new URL('../src/popup/index.ts', import.meta.url), 'utf8');
const optionsSource = readFileSync(new URL('../src/options/index.ts', import.meta.url), 'utf8');
const stateSource = readFileSync(new URL('../src/background/state.ts', import.meta.url), 'utf8');
const backgroundSource = readFileSync(new URL('../src/background/index.ts', import.meta.url), 'utf8');
const protocolSource = readFileSync(new URL('../../../packages/protocol/src/index.ts', import.meta.url), 'utf8');
const messagesSource = readFileSync(new URL('../src/shared/messages.ts', import.meta.url), 'utf8');

for (const [name, html] of [['popup', popupHtml], ['options', optionsHtml]]) {
  assert.match(html, /href="\.\/theme\.css"/, `${name} page should load the shared extension theme stylesheet`);
  assert.match(html, /src="\.\/appearance-preload\.js"[\s\S]*href="\.\/theme\.css"/, `${name} page should preload cached appearance before loading theme CSS`);
  assert.doesNotMatch(html, /--bg:|--panel-strong:|--primary-hover:/, `${name} page should not keep an isolated color variable system`);
}

assert.doesNotMatch(popupHtml, /active-count|attention-count|>Active<|>Attention</, 'popup quick actions should not render queue counters');
assert.match(popupHtml, /id="capture-mode-label"/, 'popup should name the capture mode quick control');
assert.match(popupHtml, /id="sync-button"/, 'popup should expose a manual theme/status sync action');
assert.match(buildSource, /theme\.css/, 'extension build should copy the shared theme stylesheet into each dist folder');
assert.match(buildSource, /appearance-preload\.js/, 'extension build should copy the appearance preload script into each dist folder');

assert.match(popupSource, /applyExtensionAppearance/, 'popup script should apply the synced desktop appearance');
assert.match(popupSource, /sync-button/, 'popup script should wire the manual sync action');
assert.match(popupSource, /state\.connection === 'connected' && state\.appearanceSettings/, 'popup should only cache/apply desktop appearance after a successful sync');
assert.match(optionsSource, /applyExtensionAppearance/, 'options script should apply the synced desktop appearance');
assert.match(optionsSource, /state\.connection === 'connected' && state\.appearanceSettings/, 'options should only cache/apply desktop appearance after a successful sync');
assert.match(stateSource, /appearanceSettings/, 'extension popup state should persist synced appearance settings');
assert.match(messagesSource, /appearanceSettings\?: AppearanceSettings/, 'popup state response should carry optional appearance settings');
assert.match(protocolSource, /export interface AppearanceSettings/, 'protocol should export the extension-safe appearance settings shape');
assert.match(protocolSource, /appearanceSettings\?: AppearanceSettings/, 'ready and pong payloads should include optional appearance settings');
assert.match(backgroundSource, /APPEARANCE_SYNC_ALARM_NAME\s*=\s*'appearance-sync'/, 'background script should name the appearance sync alarm');
assert.match(backgroundSource, /periodInMinutes:\s*15/, 'background script should schedule appearance sync every 15 minutes');
assert.match(backgroundSource, /browser\.alarms\.onAlarm\.addListener/, 'background script should listen for appearance sync alarms');
