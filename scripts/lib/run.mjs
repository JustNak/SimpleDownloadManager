import { spawn } from 'node:child_process';
import { readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
export const repoRoot = path.resolve(path.dirname(__filename), '..', '..');

export function repoPath(...segments) {
  return path.join(repoRoot, ...segments);
}

export async function findTestFiles(directory) {
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

export function runNodeTest(testFile) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [testFile], {
      cwd: repoRoot,
      stdio: 'inherit',
    });

    child.on('error', reject);
    child.on('exit', (code) => resolve(code ?? 1));
  });
}

export function runCommand(command, args, options = {}) {
  const {
    cwd = repoRoot,
    env,
    log = true,
    leadingNewline = false,
  } = options;

  return new Promise((resolve, reject) => {
    if (log) {
      console.log(`${leadingNewline ? '\n' : ''}$ ${command} ${args.join(' ')}`);
    }

    const [spawnCommand, spawnArgs] = process.platform === 'win32'
      ? [process.env.ComSpec ?? 'cmd.exe', ['/d', '/s', '/c', command, ...args]]
      : [command, args];

    const child = spawn(spawnCommand, spawnArgs, {
      cwd,
      env: env ? { ...process.env, ...env } : process.env,
      stdio: 'inherit',
    });

    child.on('error', reject);
    child.on('exit', (code) => resolve(code ?? 1));
  });
}
