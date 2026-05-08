import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';

const assetsUrl = new URL('../dist/assets/', import.meta.url);
let files;

try {
  files = await readdir(assetsUrl);
} catch {
  console.log('Skipping production webview chunk test because apps/desktop/dist is not built.');
  process.exit(0);
}

const jsChunks = files.filter((file) => file.endsWith('.js'));
const previewChunks = jsChunks.filter((file) => /^backendPreview-.*\.js$/.test(file));
assert.ok(previewChunks.length > 0, 'desktop build should isolate browser-preview mocks into a lazy backendPreview chunk');

const previewOnlyMarkers = [
  'mock_prompt',
  'Blender 4.1.1 Setup.exe',
  'sdm.progressBatch.',
];

for (const chunk of jsChunks.filter((file) => !previewChunks.includes(file))) {
  const source = await readFile(new URL(`../dist/assets/${chunk}`, import.meta.url), 'utf8');
  for (const marker of previewOnlyMarkers) {
    assert.doesNotMatch(
      source,
      new RegExp(marker.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
      `production webview chunk ${chunk} should not contain browser-preview marker ${marker}`,
    );
  }
}
