import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import {
  createFirefoxTestReadme,
  firefoxTestPackagePaths,
  syncFirefoxExtensionFiles,
} from '../../../scripts/package-firefox-test.mjs';

const repoRoot = path.resolve();
const paths = firefoxTestPackagePaths(repoRoot);

assert.equal(paths.sourceDir, path.join(repoRoot, 'apps', 'extension', 'dist', 'firefox'));
assert.equal(paths.packageRoot, path.join(repoRoot, 'release', 'firefox-test'));
assert.equal(paths.extensionDir, path.join(repoRoot, 'release', 'firefox-test', 'extension'));
assert.equal(paths.zipPath, path.join(repoRoot, 'release', 'firefox-test', 'simple-download-manager-firefox-test.zip'));
assert.equal(paths.readmePath, path.join(repoRoot, 'release', 'firefox-test', 'README.md'));

const readme = createFirefoxTestReadme(paths);
assert.match(readme, /about:debugging#\/runtime\/this-firefox/);
assert.match(readme, /Load Temporary Add-on/);
assert.match(readme, /extension\\manifest\.json/);
assert.match(readme, /simple-download-manager-firefox-test\.zip/);
assert.match(readme, /temporary/i);
assert.match(readme, /Mozilla signing/i);

const tempParent = path.join(repoRoot, '.tmp');
await mkdir(tempParent, { recursive: true });
const tempRoot = await mkdtemp(path.join(tempParent, 'firefox-test-package-'));
try {
  const sourceDir = path.join(tempRoot, 'source');
  const extensionDir = path.join(tempRoot, 'extension');
  await mkdir(path.join(sourceDir, 'icons'), { recursive: true });
  await writeFile(path.join(sourceDir, 'manifest.json'), '{}', 'utf8');
  await writeFile(path.join(sourceDir, 'background.js'), '', 'utf8');
  await writeFile(path.join(sourceDir, 'icons', 'icon-16.png'), 'icon', 'utf8');
  await mkdir(extensionDir, { recursive: true });
  await writeFile(path.join(extensionDir, 'stale.js'), '', 'utf8');

  await syncFirefoxExtensionFiles(sourceDir, extensionDir);

  assert.equal((await stat(path.join(extensionDir, 'manifest.json'))).isFile(), true);
  assert.equal((await stat(path.join(extensionDir, 'icons', 'icon-16.png'))).isFile(), true);
  await assert.rejects(
    readFile(path.join(extensionDir, 'stale.js')),
    /ENOENT/,
    'Firefox test package sync should remove stale files from previous builds',
  );
} finally {
  await rm(tempRoot, { recursive: true, force: true });
}
