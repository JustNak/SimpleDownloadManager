import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8');

assert.doesNotMatch(
  source,
  /Cycle filter/,
  'command bar should not expose the cycle-filter icon button',
);

assert.doesNotMatch(
  source,
  /onCycleFilter/,
  'command bar should not keep unused cycle-filter plumbing',
);

assert.doesNotMatch(
  source,
  /\bFilter,/,
  'command bar should not import the unused Filter icon',
);

assert.match(
  source,
  /\bFilePlus,\s*\n[\s\S]*} from 'lucide-react';/,
  'command bar should import the richer FilePlus icon for the New Download CTA',
);

assert.match(
  source,
  /\bMoreHorizontal,\s*\n[\s\S]*} from 'lucide-react';/,
  'command bar should import the ellipsis icon for the compact queue menu',
);

assert.doesNotMatch(
  source,
  /<ToolbarButton icon=\{<Plus size=\{17\} \/>\} label="New Download"/,
  'New Download should not use the generic Plus icon',
);

assert.match(
  source,
  /<ToolbarButton icon=\{<FilePlus size=\{17\} strokeWidth=\{2\.4\} \/>\} label="New Download" onClick=\{onAdd\} strong \/>/,
  'New Download should use the richer FilePlus icon without changing its label or click behavior',
);

for (const label of ['Resume All', 'Pause All', 'Retry Failed']) {
  assert.doesNotMatch(
    source,
    new RegExp(`<ToolbarButton[^>]+label="${label}"`),
    `${label} should not remain as a visible top-level toolbar button`,
  );
  assert.match(
    source,
    new RegExp(`<CommandMenuItem[\\s\\S]{0,120}label="${label}"`),
    `${label} should move into the compact queue menu`,
  );
}

assert.match(
  source,
  /<ToolbarButton[\s\S]{0,120}icon=\{<MoreHorizontal size=\{18\} \/>\}[\s\S]{0,80}label="More"/,
  'queue actions should be condensed behind the More ellipsis command',
);

assert.match(
  source,
  /className="flex w-\[310px\] max-w-\[42vw\] shrink-0 items-center justify-end gap-2"/,
  'command-bar search should be capped at roughly half of its previous maximum width',
);

assert.match(
  source,
  /<label className="relative w-full min-w-0">/,
  'command-bar search label should fill only the capped search container',
);

assert.match(
  source,
  /const QUEUE_ROW_SIZE_OPTIONS[\s\S]*Compact[\s\S]*Small[\s\S]*Medium[\s\S]*Large[\s\S]*DAMN/,
  'queue menu should expose all requested row-size choices including DAMN',
);

assert.match(
  source,
  /onQueueRowSizeChange=\{\(queueRowSize\) => void handleQueueRowSizeChange\(queueRowSize\)\}/,
  'command bar should persist row-size choices through the desktop settings path',
);

assert.match(
  source,
  /const \[sortMode, setSortMode\] = useState<SortMode>\('date:asc'\)/,
  'fresh app sessions should default to Date ascending sort',
);

assert.match(
  source,
  /event\.key !== 'F11'[\s\S]*mainWindow\.toggleMaximize\(\)/,
  'F11 should toggle the native main window maximize state',
);

assert.match(
  source,
  /event\.key !== 'Escape' \|\| view !== 'settings'[\s\S]*requestViewChange\('all'\)/,
  'Escape in Settings should return to All Downloads through the existing view-change guard',
);

assert.match(
  source,
  /strong\s*\?\s*'border border-primary\/60 bg-primary text-primary-foreground shadow-sm hover:bg-primary\/90 active:bg-primary\/80 focus-visible:ring-2 focus-visible:ring-primary\/30'/,
  'the strong toolbar variant should use the primary accent treatment',
);

assert.match(
  source,
  /:\s*'border border-transparent text-muted-foreground hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-muted-foreground'/,
  'non-strong toolbar buttons should keep the muted toolbar treatment',
);
