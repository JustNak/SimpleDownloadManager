import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const popupHtml = readFileSync(new URL('../src/popup/index.html', import.meta.url), 'utf8');
const optionsHtml = readFileSync(new URL('../src/options/index.html', import.meta.url), 'utf8');
const buildSource = readFileSync(new URL('../scripts/build.mjs', import.meta.url), 'utf8');
const popupSource = readFileSync(new URL('../src/popup/index.ts', import.meta.url), 'utf8');
const optionsSource = readFileSync(new URL('../src/options/index.ts', import.meta.url), 'utf8');
const stateSource = readFileSync(new URL('../src/background/state.ts', import.meta.url), 'utf8');
const protocolSource = readFileSync(new URL('../../../packages/protocol/src/index.ts', import.meta.url), 'utf8');
const messagesSource = readFileSync(new URL('../src/shared/messages.ts', import.meta.url), 'utf8');

for (const [name, html] of [['popup', popupHtml], ['options', optionsHtml]]) {
  assert.match(html, /href="\.\/theme\.css"/, `${name} page should load the shared extension theme stylesheet`);
  assert.doesNotMatch(html, /--bg:|--panel-strong:|--primary-hover:/, `${name} page should not keep an isolated color variable system`);
}

assert.match(
  popupHtml,
  /id="active-count"[\s\S]*id="attention-count"/,
  'popup should expose compact queue counters for quick status checks',
);
assert.match(popupHtml, /id="capture-mode-label"/, 'popup should name the capture mode quick control');
assert.match(buildSource, /theme\.css/, 'extension build should copy the shared theme stylesheet into each dist folder');

assert.match(popupSource, /applyExtensionAppearance/, 'popup script should apply the synced desktop appearance');
assert.match(optionsSource, /applyExtensionAppearance/, 'options script should apply the synced desktop appearance');
assert.match(stateSource, /appearanceSettings/, 'extension popup state should persist synced appearance settings');
assert.match(messagesSource, /appearanceSettings\?: AppearanceSettings/, 'popup state response should carry optional appearance settings');
assert.match(protocolSource, /export interface AppearanceSettings/, 'protocol should export the extension-safe appearance settings shape');
assert.match(protocolSource, /appearanceSettings\?: AppearanceSettings/, 'ready and pong payloads should include optional appearance settings');
