import assert from 'node:assert/strict';
import { stat, readFile } from 'node:fs/promises';
import path from 'node:path';

const repoRoot = path.resolve();
const rootPackage = JSON.parse(await readFile(path.join(repoRoot, 'package.json'), 'utf8'));

assert.equal(
  rootPackage.scripts['test:extension'],
  'node ./scripts/run-extension-tests.mjs',
  'root package should expose an extension-only test command',
);
assert.equal(
  rootPackage.scripts['verify:extension'],
  'npm run typecheck --workspace @myapp/extension && npm run build:extension && npm run test:extension && npm run lint:firefox',
  'root package should expose a single extension-only verification command',
);
assert.equal((await stat(path.join(repoRoot, 'scripts', 'run-extension-tests.mjs'))).isFile(), true);

