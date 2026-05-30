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
assert.match(popupHtml, /<h1>SDM<span id="connection-status" class="connection-dot checking" aria-hidden="true"><\/span><span id="connection-status-label" class="visually-hidden">Checking connection<\/span><\/h1>/, 'popup header should use compact SDM branding with an icon-only connection dot');
assert.match(popupHtml, /id="capture-mode-label"[^>]*>Silent Download</, 'popup quick toggle should always be labeled Silent Download');
assert.match(popupHtml, /On skips the prompt unless a duplicate needs review\./, 'popup should explain the simple silent-download on behavior');
assert.match(popupHtml, /id="extension-enabled-label"[^>]*>Web extension</, 'popup should expose extension enablement as an integrated settings row');
assert.doesNotMatch(popupHtml, /Ask Before Sending|Ask before sending downloads|Browser Only/, 'popup should not expose alternate handoff mode names');
assert.match(popupHtml, /id="sync-button"[\s\S]*aria-label="Sync extension status"/, 'popup should expose an icon-only manual theme/status sync action');
assert.match(popupHtml, /id="advanced-button"[\s\S]*aria-label="Open extension settings"/, 'popup should expose an icon-only settings action');
assert.doesNotMatch(popupHtml, />Connected<|>Checking<|>Sync<|>Advanced<|>Disable Extension<|>Enable Extension</, 'popup should not render verbose status/action labels');
assert.match(buildSource, /theme\.css/, 'extension build should copy the shared theme stylesheet into each dist folder');
assert.match(buildSource, /appearance-preload\.js/, 'extension build should copy the appearance preload script into each dist folder');

assert.match(popupSource, /applyExtensionAppearance/, 'popup script should apply the synced desktop appearance');
assert.match(popupSource, /sync-button/, 'popup script should wire the manual sync action');
assert.match(popupSource, /extension-enabled-toggle/, 'popup script should wire the integrated extension enablement toggle');
assert.match(popupSource, /connection-dot connected|connection-dot checking|connection-dot disconnected/, 'popup script should map connection states to dot color classes');
assert.match(popupSource, /state\.connection === 'connected' && state\.appearanceSettings/, 'popup should only cache/apply desktop appearance after a successful sync');
{
  const refreshStateStart = popupSource.indexOf('async function refreshState');
  assert.notEqual(refreshStateStart, -1, 'popup script should define refreshState');
  const refreshStateSource = popupSource.slice(refreshStateStart);
  const cachedStateReadIndex = refreshStateSource.indexOf("sendMessage<PopupStateResponse>({ type: 'popup_get_state' })");
  const pingIndex = refreshStateSource.indexOf("sendMessage({ type: 'popup_ping' })");
  assert.ok(cachedStateReadIndex !== -1 && pingIndex !== -1 && cachedStateReadIndex < pingIndex, 'popup should render cached background state before pinging the native host');
}
assert.doesNotMatch(popupSource, /captureModeLabelText|captureModeDescription|Ask Before Sending|Ask before sending downloads|Browser Only/, 'popup script should not transition quick-toggle copy between handoff modes');
assert.match(popupSource, /downloadHandoffMode:\s*silentDownloadToggle\.checked \? 'auto' : 'ask'/, 'silent download off should keep the default ask-before-download behavior');
assert.match(optionsSource, /applyExtensionAppearance/, 'options script should apply the synced desktop appearance');
assert.match(optionsSource, /state\.connection === 'connected' && state\.appearanceSettings/, 'options should only cache/apply desktop appearance after a successful sync');
assert.match(stateSource, /appearanceSettings/, 'extension popup state should persist synced appearance settings');
assert.match(messagesSource, /appearanceSettings\?: AppearanceSettings/, 'popup state response should carry optional appearance settings');
assert.match(protocolSource, /export interface AppearanceSettings/, 'protocol should export the extension-safe appearance settings shape');
assert.match(protocolSource, /appearanceSettings\?: AppearanceSettings/, 'ready and pong payloads should include optional appearance settings');
assert.match(backgroundSource, /APPEARANCE_SYNC_ALARM_NAME\s*=\s*'appearance-sync'/, 'background script should name the appearance sync alarm');
assert.match(backgroundSource, /periodInMinutes:\s*15/, 'background script should schedule appearance sync every 15 minutes');
assert.match(backgroundSource, /browser\.alarms\.onAlarm\.addListener/, 'background script should listen for appearance sync alarms');
