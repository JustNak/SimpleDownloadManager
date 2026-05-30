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
assert.doesNotMatch(
  optionsSource,
  /authHandoffToggle|protectedDownloadAuthScope: authHandoffToggle/,
  'options script should not wire a browser-session toggle for automatic capture',
);
assert.doesNotMatch(
  optionsHtml,
  /Browser Session Downloads|auth-handoff-toggle/,
  'options page should not expose a browser-session toggle for automatic capture',
);
assert.doesNotMatch(
  optionsHtml,
  /Forward memory-only browser session headers/,
  'options page should not present header forwarding as protected-download support',
);
assert.doesNotMatch(
  optionsSource,
  /protectedHostInput[\s\S]*addProtectedHostButton[\s\S]*saving \|\| !extensionEnabled \|\| !protectedDownloadsEnabled/,
  'options script should not manage removed protected-download host controls',
);

assert.doesNotMatch(
  optionsHtml,
  /Ignored File Extensions|ignored-extension-input|ignored-extensions/,
  'options page should not expose ignored file extensions now that captured extensions are the single gate',
);
assert.doesNotMatch(
  optionsSource,
  /ignoredExtensionInput|renderIgnoredExtensions|addIgnoredExtensions|ignoredFileExtensions:/,
  'options script should not wire ignored file extension controls',
);
assert.match(
  optionsHtml,
  /id="restore-captured-extensions-button"[\s\S]*<svg viewBox="0 0 24 24"/,
  'captured extension settings should provide an SVG restore-default button',
);
assert.match(
  optionsSource,
  /DEFAULT_CAPTURED_FILE_EXTENSIONS[\s\S]*capturedFileExtensions: \[\.\.\.DEFAULT_CAPTURED_FILE_EXTENSIONS\]/,
  'restore-default button should reset captured extensions to the supported default list',
);
