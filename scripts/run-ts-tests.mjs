import { readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';

const __filename = fileURLToPath(import.meta.url);
const repoRoot = path.resolve(path.dirname(__filename), '..');
const testRoots = [
  path.join(repoRoot, 'apps', 'desktop', 'tests'),
  path.join(repoRoot, 'apps', 'extension', 'tests'),
];

const testFiles = (await Promise.all(testRoots.map(findTestFiles))).flat()
  .sort((left, right) => left.localeCompare(right));

for (const testFile of testFiles) {
  const relativePath = path.relative(repoRoot, testFile);
  console.log(`Running ${relativePath}`);
  await runNode(testFile);
}

async function findTestFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...await findTestFiles(fullPath));
      continue;
    }

    if (/\.(?:test|spec)\.(?:mjs|ts)$/.test(entry.name)) {
      files.push(fullPath);
    }
  }

  return files;
}

function runNode(testFile) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [testFile], {
      cwd: repoRoot,
      stdio: 'inherit',
    });

    child.on('error', reject);
    child.on('exit', (code) => {
      if (code === 0) {
        resolve();
        return;
      }
      reject(new Error(`${path.relative(repoRoot, testFile)} failed with exit code ${code}`));
    });
  });
}
