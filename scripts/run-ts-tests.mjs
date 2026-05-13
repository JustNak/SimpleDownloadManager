import path from 'node:path';
import { findTestFiles, repoPath, repoRoot, runNodeTest } from './lib/run.mjs';

const testRoots = [
  repoPath('apps', 'desktop', 'tests'),
  repoPath('apps', 'extension', 'tests'),
];

const testFiles = (await Promise.all(testRoots.map(findTestFiles))).flat()
  .sort((left, right) => left.localeCompare(right));

const failures = [];

for (const testFile of testFiles) {
  const relativePath = path.relative(repoRoot, testFile);
  console.log(`Running ${relativePath}`);
  const result = await runNodeTest(testFile);
  if (result !== 0) {
    failures.push({ relativePath, code: result });
  }
}

if (failures.length > 0) {
  console.error('\nFailed TS/MJS tests:');
  for (const failure of failures) {
    console.error(`- ${failure.relativePath} exited with code ${failure.code}`);
  }
  process.exit(1);
}
