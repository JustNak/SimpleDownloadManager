import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

let amoPackaging;
try {
  amoPackaging = await import('../../../scripts/package-firefox-amo.mjs');
} catch (error) {
  assert.fail(`Firefox AMO packaging module should exist: ${error instanceof Error ? error.message : error}`);
}

const {
  createFirefoxAmoReadme,
  createFirefoxAmoReviewerNotes,
  createFirefoxAmoSourceReadme,
  firefoxAmoPackagePaths,
  firefoxAmoSourceEntries,
} = amoPackaging;

const repoRoot = path.resolve();
const paths = firefoxAmoPackagePaths(repoRoot);

assert.equal(paths.sourceDir, path.join(repoRoot, 'apps', 'extension', 'dist', 'firefox'));
assert.equal(paths.packageRoot, path.join(repoRoot, 'release', 'firefox-amo'));
assert.equal(paths.uploadDir, path.join(repoRoot, 'release', 'firefox-amo', 'upload'));
assert.equal(paths.sourceReviewDir, path.join(repoRoot, 'release', 'firefox-amo', 'source'));
assert.equal(paths.uploadZipPath, path.join(repoRoot, 'release', 'firefox-amo', 'simple-download-manager-firefox-upload.zip'));
assert.equal(paths.sourceZipPath, path.join(repoRoot, 'release', 'firefox-amo', 'simple-download-manager-firefox-source.zip'));
assert.equal(paths.readmePath, path.join(repoRoot, 'release', 'firefox-amo', 'README.md'));
assert.equal(paths.reviewerNotesPath, path.join(repoRoot, 'release', 'firefox-amo', 'AMO_REVIEWER_NOTES.md'));

const entries = firefoxAmoSourceEntries();
assert.deepEqual(
  entries.map((entry) => entry.source),
  [
    'package.json',
    'package-lock.json',
    'tsconfig.base.json',
    'config/release.json',
    'apps/extension',
    'packages/protocol',
  ],
);
assert.equal(
  entries.some((entry) => /(?:^|[/\\])dist(?:[/\\]|$)|(?:^|[/\\])release(?:[/\\]|$)|(?:^|[/\\])node_modules(?:[/\\]|$)|(?:^|[/\\])target(?:[/\\]|$)/.test(entry.source)),
  false,
  'source package entries should not include generated or heavy directories',
);

const uploadReadme = createFirefoxAmoReadme(paths);
assert.match(uploadReadme, /AMO Developer Hub/);
assert.match(uploadReadme, /On your own/);
assert.match(uploadReadme, /simple-download-manager-firefox-upload\.zip/);
assert.match(uploadReadme, /simple-download-manager-firefox-source\.zip/);
assert.match(uploadReadme, /web-ext lint --source-dir apps\\extension\\dist\\firefox/);
assert.match(uploadReadme, /web-ext sign --source-dir apps\\extension\\dist\\firefox --channel=unlisted/);
assert.match(uploadReadme, /AMO_REVIEWER_NOTES\.md/);
assert.match(uploadReadme, /Firefox 142/);

const reviewerNotes = createFirefoxAmoReviewerNotes();
assert.match(reviewerNotes, /Native messaging/);
assert.match(reviewerNotes, /webRequestBlocking/);
assert.match(reviewerNotes, /<all_urls>/);
assert.match(reviewerNotes, /browsingActivity/);
assert.match(reviewerNotes, /websiteActivity/);
assert.match(reviewerNotes, /websiteContent/);
assert.match(reviewerNotes, /No remote code/);
assert.match(reviewerNotes, /local native desktop app/);
assert.match(reviewerNotes, /wildcard excluded host patterns/);

const sourceReadme = createFirefoxAmoSourceReadme();
assert.match(sourceReadme, /npm ci/);
assert.match(sourceReadme, /npm run build --workspace @myapp\/extension/);
assert.match(sourceReadme, /apps\/extension\/dist\/firefox/);
assert.match(sourceReadme, /uploaded extension ZIP/);

const rootPackage = JSON.parse(await readFile(path.join(repoRoot, 'package.json'), 'utf8'));
assert.equal(
  rootPackage.scripts['package:firefox-amo'],
  'npm run build:extension && node ./scripts/package-firefox-amo.mjs',
);
assert.equal(
  rootPackage.scripts['lint:firefox'],
  'node ./scripts/lint-firefox.mjs',
);

const lintScript = await readFile(path.join(repoRoot, 'scripts', 'lint-firefox.mjs'), 'utf8');
assert.match(lintScript, /NO_UPDATE_NOTIFIER/);
assert.match(lintScript, /apps[\\/]extension[\\/]dist[\\/]firefox/);
