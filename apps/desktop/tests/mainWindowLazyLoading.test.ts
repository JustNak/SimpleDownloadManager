import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';

const mainSource = await readFile(new URL('../src/main.tsx', import.meta.url), 'utf8');
const appSource = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');
const backendSource = await readFile(new URL('../src/backend.ts', import.meta.url), 'utf8');
const backendMockUrl = new URL('../src/backendMock.ts', import.meta.url);

assert.ok(
  existsSync(backendMockUrl),
  'browser-preview mock behavior should live in backendMock.ts',
);

const backendMockSource = await readFile(backendMockUrl, 'utf8');

assert.doesNotMatch(
  mainSource,
  /import\s+App\s+from\s+['"]\.\/App['"]/,
  'main renderer should not statically import the full App bundle for every popup window',
);

for (const moduleName of [
  'App',
  'DownloadPromptWindow',
  'DownloadProgressWindow',
  'BatchProgressWindow',
  'TorrentProgressWindow',
]) {
  assert.match(
    mainSource,
    new RegExp(`import\\(['"]\\./${moduleName}['"]\\)`),
    `main renderer should dynamically import ${moduleName}`,
  );
}

assert.doesNotMatch(
  appSource,
  /import\s+\{\s*SettingsPage\s*\}\s+from\s+['"]\.\/SettingsPage['"]/,
  'main App should lazy-load SettingsPage instead of putting settings code in the initial queue bundle',
);

assert.doesNotMatch(
  appSource,
  /import\s+\{\s*AddDownloadModal\s*,/,
  'main App should lazy-load AddDownloadModal instead of putting modal code in the initial queue bundle',
);

assert.match(
  appSource,
  /import\(['"]\.\/SettingsPage['"]\)/,
  'SettingsPage should be dynamically imported when needed',
);

assert.match(
  appSource,
  /import\(['"]\.\/AddDownloadModal['"]\)/,
  'AddDownloadModal should be dynamically imported when needed',
);

assert.doesNotMatch(
  backendSource,
  /let\s+mockState\s*:/,
  'production backend bridge should not allocate browser-preview mock state at module load',
);

assert.match(
  backendMockSource,
  /let\s+mockState\s*:/,
  'browser-preview mock state should live in backendMock.ts',
);
