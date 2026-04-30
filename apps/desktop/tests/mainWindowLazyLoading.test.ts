import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const mainSource = await readFile(new URL('../src/main.tsx', import.meta.url), 'utf8');
const appSource = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');
const backendSource = await readFile(new URL('../src/backend.ts', import.meta.url), 'utf8');

assert.doesNotMatch(
  mainSource,
  /import\s+App\s+from\s+['"]\.\/App['"]/,
  'main renderer should not statically import the full App bundle for every popup window',
);
assert.match(
  mainSource,
  /import\(['"]\.\/App['"]\)/,
  'main renderer should dynamically import the full App only for the main window',
);
assert.match(
  mainSource,
  /import\(['"]\.\/DownloadPromptWindow['"]\)/,
  'download prompt window should be dynamically imported',
);
assert.match(
  mainSource,
  /import\(['"]\.\/DownloadProgressWindow['"]\)/,
  'download progress window should be dynamically imported',
);
assert.match(
  mainSource,
  /import\(['"]\.\/BatchProgressWindow['"]\)/,
  'batch progress window should be dynamically imported',
);

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
