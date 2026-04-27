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
