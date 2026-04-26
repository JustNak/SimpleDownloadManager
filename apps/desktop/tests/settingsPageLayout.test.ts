import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/SettingsPage.tsx', import.meta.url), 'utf8');

assert.match(
  source,
  /settings-surface[\s\S]*max-w-3xl[\s\S]*gap-3[\s\S]*p-4/,
  'settings surface should use the compact width, gap, and padding tokens',
);

assert.match(
  source,
  /function ExcludedSitesDialog/,
  'excluded-site editing should live in a dedicated dialog component',
);

assert.match(
  source,
  /Configure Sites/,
  'excluded-site row should open the dedicated configuration dialog',
);

assert.doesNotMatch(
  source,
  /No browser-only hosts configured/,
  'excluded-site row should not render a summary card around the configure button',
);

assert.doesNotMatch(
  source,
  /FieldRow label="Excluded Sites"[\s\S]*value=\{excludedHostInput\}/,
  'excluded-site row should not keep the full inline add/list editor',
);
