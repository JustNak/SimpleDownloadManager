import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/SettingsPage.tsx', import.meta.url), 'utf8');

assert.match(
  source,
  /settings-surface[\s\S]*grid[\s\S]*max-w-6xl[\s\S]*grid-cols-\[220px_minmax\(0,1fr\)\][\s\S]*gap-4[\s\S]*p-4/,
  'settings surface should use the two-column navigator layout tokens',
);

assert.match(
  source,
  /settings-nav/,
  'settings should include a dedicated left-side navigator',
);

for (const sectionId of [
  'settings-general',
  'settings-updates',
  'settings-torrenting',
  'settings-appearance',
  'settings-extension',
  'settings-native-host',
]) {
  assert.match(
    source,
    new RegExp(`id="${sectionId}"`),
    `settings navigator target ${sectionId} should exist`,
  );
  assert.match(
    source,
    new RegExp(`href="#${sectionId}"`),
    `settings navigator should link to ${sectionId}`,
  );
}

assert.match(
  source,
  /settings-nav sticky top-24/,
  'settings navigator should stay visible below the sticky settings header while scrolling long settings pages',
);

assert.match(
  source,
  /<header className="col-span-2 sticky top-0 z-30 flex items-center justify-between border-b border-border bg-surface\/95 pb-3 pt-4 backdrop-blur/,
  'settings header actions should stay sticky while scrolling long settings pages',
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
