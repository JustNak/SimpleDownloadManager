import path from 'node:path';
import { findTestFiles, repoPath, repoRoot, runNodeTest } from './lib/run.mjs';

const extensionTestRoot = repoPath('apps', 'extension', 'tests');

const testFiles = (await findTestFiles(extensionTestRoot))
  .sort((left, right) => left.localeCompare(right));

for (const testFile of testFiles) {
  const relativePath = path.relative(repoRoot, testFile);
  console.log(`Running ${relativePath}`);
  const code = await runNodeTest(testFile);
  if (code !== 0) {
    throw new Error(`${relativePath} failed with exit code ${code}`);
  }
}
