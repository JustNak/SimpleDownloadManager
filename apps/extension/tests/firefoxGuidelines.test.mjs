import assert from 'node:assert/strict';
import { readFile, stat } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const guidelinePath = path.join(repoRoot, 'apps', 'extension', 'FIREFOX_GUIDELINES.md');
await stat(guidelinePath);

const guide = await readFile(guidelinePath, 'utf8');

for (const expected of [
  'AMO Review',
  'manifest_version',
  'version_name',
  'nativeMessaging',
  'webRequestBlocking',
  'data_collection_permissions',
  'authenticated handoff',
  'No remote code',
  'npm run build:extension',
  'npm run lint:firefox',
  'npm run package:firefox-amo',
]) {
  assert.match(guide, new RegExp(expected), `Firefox guideline should mention ${expected}`);
}
