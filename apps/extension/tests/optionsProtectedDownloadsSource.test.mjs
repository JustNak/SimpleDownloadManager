import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const optionsHtml = readFileSync(new URL('../src/options/index.html', import.meta.url), 'utf8');
const optionsSource = readFileSync(new URL('../src/options/index.ts', import.meta.url), 'utf8');

assert.match(
  optionsHtml,
  /Protected Download Sites/,
  'options page should expose a protected-download host allowlist section',
);
assert.match(
  optionsHtml,
  /id="protected-host-input"/,
  'protected-download allowlist should include a host input',
);
assert.match(
  optionsHtml,
  /id="add-protected-host-button"/,
  'protected-download allowlist should include an add button',
);
assert.match(
  optionsHtml,
  /id="protected-hosts"/,
  'protected-download allowlist should render configured hosts',
);

assert.match(
  optionsSource,
  /const protectedHostInput = document\.querySelector<HTMLInputElement>\('#protected-host-input'\);/,
  'options script should wire the protected-download host input',
);
assert.match(
  optionsSource,
  /const addProtectedHostButton = document\.querySelector<HTMLButtonElement>\('#add-protected-host-button'\);/,
  'options script should wire the protected-download add button',
);
assert.match(
  optionsSource,
  /const protectedHosts = document\.querySelector<HTMLDivElement>\('#protected-hosts'\);/,
  'options script should wire the protected-download chip container',
);
assert.match(
  optionsSource,
  /renderProtectedHosts\([\s\S]*settings\.authenticatedHandoffHosts \?\? \[\],[\s\S]*isSaving \|\| !settings\.enabled \|\| !protectedDownloadsEnabled,[\s\S]*\);/,
  'rendering settings should show the configured protected-download hosts',
);
assert.match(
  optionsSource,
  /function addProtectedHost\(\)/,
  'options script should provide an add handler for protected-download hosts',
);
assert.match(
  optionsSource,
  /authenticatedHandoffHosts: \[\.\.\.hosts, host\]/,
  'adding a protected-download host should persist authenticatedHandoffHosts',
);
assert.match(
  optionsSource,
  /protectedDownloadAuthScope: 'allowlist'/,
  'protected-download host edits should keep the extension in allowlist mode',
);
assert.match(
  optionsSource,
  /protectedHostInput[\s\S]*addProtectedHostButton[\s\S]*saving \|\| !extensionEnabled \|\| !protectedDownloadsEnabled/,
  'protected-download host controls should be disabled unless the extension and Protected Downloads are enabled',
);
