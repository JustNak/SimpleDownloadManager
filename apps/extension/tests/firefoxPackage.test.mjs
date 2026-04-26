import assert from 'node:assert/strict';
import path from 'node:path';
import {
  createFirefoxTestReadme,
  firefoxTestPackagePaths,
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
