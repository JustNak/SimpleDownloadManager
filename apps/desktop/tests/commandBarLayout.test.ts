import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8');

assert.doesNotMatch(source, /Cycle filter|onCycleFilter|\bFilter,/, 'command bar should not keep unused cycle-filter plumbing');
assert.match(source, /\bFilePlus,[\s\S]*} from '@lucide\/svelte';/, 'command bar should import the richer FilePlus icon for the New Download CTA');
assert.match(source, /\bMoreHorizontal,[\s\S]*} from '@lucide\/svelte';/, 'command bar should import the ellipsis icon for the compact queue menu');
assert.match(source, /@render ToolbarButton\(FilePlus, 'New Download'[\s\S]*true\)/, 'New Download should use the richer FilePlus icon without changing its label or strong treatment');

for (const label of ['Resume All', 'Pause All', 'Retry Failed']) {
  assert.match(source, new RegExp(`CommandMenuItem\\([\\s\\S]{0,120}'${label}'`), `${label} should live in the compact queue menu`);
}

assert.match(source, /@render ToolbarButton\(MoreHorizontal, 'More'/, 'queue actions should be condensed behind the More ellipsis command');
assert.match(source, /class="flex w-\[310px\] max-w-\[42vw\] shrink-0 items-center justify-end gap-2"/, 'command-bar search should be capped at roughly half of its previous maximum width');
assert.match(source, /class="relative w-full min-w-0"/, 'command-bar search label should fill only the capped search container');
assert.match(source, /const queueRowSizeOptions[\s\S]*Compact[\s\S]*Small[\s\S]*Medium[\s\S]*Large[\s\S]*DAMN/, 'queue menu should expose all requested row-size choices including DAMN');
assert.match(source, /handleQueueRowSizeChange\(option\.value\)/, 'command bar should persist row-size choices through the desktop settings path');
assert.match(source, /let sortMode = \$state<SortMode>\('date:asc'\)/, 'fresh app sessions should default to Date ascending sort');
assert.match(source, /event\.key !== 'F11'[\s\S]*mainWindow\.toggleMaximize\(\)/, 'F11 should toggle the native main window maximize state');
assert.match(source, /event\.key !== 'Escape' \|\| view !== 'settings'[\s\S]*requestViewChange\('all'\)/, 'Escape in Settings should return to All Downloads through the guarded view-change path');
assert.match(source, /strong[\s\S]*border border-primary\/60 bg-primary text-primary-foreground shadow-sm/, 'the strong toolbar variant should use the primary accent treatment');
assert.match(source, /border border-transparent text-muted-foreground hover:bg-muted hover:text-foreground/, 'non-strong toolbar buttons should keep the muted toolbar treatment');
