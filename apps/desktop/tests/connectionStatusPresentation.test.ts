import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');

assert.match(
  source,
  /function connectionStatusPresentation\(state: ConnectionState\)/,
  'status bar should centralize browser-extension connection presentation',
);

assert.match(
  source,
  /case ConnectionState\.Checking:[\s\S]{0,260}label: 'Checking'[\s\S]{0,260}className: 'text-muted-foreground'/,
  'checking connection state should use neutral muted styling',
);

assert.doesNotMatch(
  source,
  /case ConnectionState\.Checking:[\s\S]{0,260}text-destructive/,
  'checking connection state should not look destructive while the app is polling extension connectivity',
);

assert.match(
  source,
  /case ConnectionState\.Connected:[\s\S]{0,260}label: 'Connected'[\s\S]{0,260}className: 'text-foreground'/,
  'connected state should keep the foreground connected treatment',
);

for (const state of ['HostMissing', 'AppMissing', 'AppUnreachable', 'Error']) {
  assert.match(
    source,
    new RegExp(`case ConnectionState\\.${state}:[\\s\\S]{0,260}className: 'text-destructive'`),
    `${state} should keep the destructive treatment because it is an actionable failure`,
  );
}
