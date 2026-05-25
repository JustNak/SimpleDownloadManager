import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const optionsHtml = readFileSync(new URL('../src/options/index.html', import.meta.url), 'utf8');
const optionsSource = readFileSync(new URL('../src/options/index.ts', import.meta.url), 'utf8');

assert.doesNotMatch(
  optionsHtml,
  /Protected Download Sites/,
  'options page should not expose a protected-download host allowlist section',
);
assert.doesNotMatch(
  optionsHtml,
  /id="protected-host-input"/,
  'protected-download settings should not include a host input',
);
assert.doesNotMatch(
  optionsHtml,
  /id="add-protected-host-button"/,
  'protected-download settings should not include an add button',
);
assert.doesNotMatch(
  optionsHtml,
  /id="protected-hosts"/,
  'protected-download settings should not render configured host chips',
);

assert.doesNotMatch(
  optionsSource,
  /const protectedHostInput = document\.querySelector<HTMLInputElement>\('#protected-host-input'\);/,
  'options script should not wire a protected-download host input',
);
assert.doesNotMatch(
  optionsSource,
  /const addProtectedHostButton = document\.querySelector<HTMLButtonElement>\('#add-protected-host-button'\);/,
  'options script should not wire a protected-download add button',
);
assert.doesNotMatch(
  optionsSource,
  /const protectedHosts = document\.querySelector<HTMLDivElement>\('#protected-hosts'\);/,
  'options script should not wire a protected-download chip container',
);
assert.doesNotMatch(
  optionsSource,
  /renderProtectedHosts\([\s\S]*settings\.authenticatedHandoffHosts \?\? \[\],[\s\S]*isSaving \|\| !settings\.enabled \|\| !protectedDownloadsEnabled,[\s\S]*\);/,
  'rendering settings should not show configured protected-download hosts',
);
assert.doesNotMatch(
  optionsSource,
  /function addProtectedHost\(\)/,
  'options script should not provide an add handler for protected-download hosts',
);
assert.doesNotMatch(
  optionsSource,
  /authenticatedHandoffHosts: \[\.\.\.hosts, host\]/,
  'options script should not persist protected-download host edits',
);
assert.match(
  optionsSource,
  /protectedDownloadAuthScope: authHandoffToggle\.checked \? 'legacy_global' : 'off'/,
  'Protected Downloads toggle should save global browser-session forwarding when enabled',
);
assert.doesNotMatch(
  optionsSource,
  /protectedHostInput[\s\S]*addProtectedHostButton[\s\S]*saving \|\| !extensionEnabled \|\| !protectedDownloadsEnabled/,
  'options script should not manage removed protected-download host controls',
);
