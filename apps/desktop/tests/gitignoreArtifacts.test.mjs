import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const gitignore = await readFile(path.join(repoRoot, '.gitignore'), 'utf8');
const ignoredPaths = new Set(
  gitignore
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith('#')),
);

for (const artifactPath of [
  'output/playwright/',
  'playwright-report/',
  'test-results/',
  'blob-report/',
  'playwright/.cache/',
]) {
  assert.equal(
    ignoredPaths.has(artifactPath),
    true,
    `.gitignore should ignore Playwright artifact path ${artifactPath}`,
  );
}
