import { access, readFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import {
  updaterMetadataFilename,
  updaterReleasePaths,
  updaterReleaseTag,
} from './updater-release.mjs';

const __filename = fileURLToPath(import.meta.url);
const defaultRoot = path.resolve(path.dirname(__filename), '..');

export function missingGitHubCliMessage() {
  return [
    'GitHub CLI (gh) was not found on PATH.',
    'Install it from https://cli.github.com/ or with: winget install --id GitHub.cli -e',
    'Then authenticate with: gh auth login',
    'After that, rerun: npm run publish:updater-alpha',
  ].join('\n');
}

export function isMissingGitHubCliError(error) {
  return Boolean(
    error
      && typeof error === 'object'
      && error.code === 'ENOENT'
      && (error.path === 'gh' || String(error.syscall ?? '').includes('spawn gh')),
  );
}

export async function assertGitHubCliAvailable(runCommand = runGh) {
  try {
    await runCommand(['--version'], { stdio: 'ignore' });
  } catch (error) {
    if (isMissingGitHubCliError(error)) {
      throw new Error(missingGitHubCliMessage());
    }

    throw error;
  }
}

export async function publishUpdaterAlpha(repoRoot = defaultRoot) {
  const packageJson = JSON.parse(await readFile(path.join(repoRoot, 'package.json'), 'utf8'));
  const paths = updaterReleasePaths(repoRoot, packageJson.version);

  await assertGitHubCliAvailable((args, options) => runGh(args, { ...options, cwd: repoRoot }));

  await Promise.all([
    access(paths.installerPath),
    access(paths.signaturePath),
    access(paths.metadataPath),
  ]);

  const releaseExists = await runGh(['release', 'view', updaterReleaseTag], { allowFailure: true, cwd: repoRoot });
  if (releaseExists !== 0) {
    await runGh([
      'release',
      'create',
      updaterReleaseTag,
      '--title',
      'Simple Download Manager Alpha Updates',
      '--notes',
      'Static updater feed for alpha builds.',
      '--prerelease',
      '--latest=false',
    ], { cwd: repoRoot });
  }

  await runGh([
    'release',
    'upload',
    updaterReleaseTag,
    paths.installerPath,
    paths.signaturePath,
    paths.metadataPath,
    '--clobber',
  ], { cwd: repoRoot });

  console.log(`Uploaded ${updaterMetadataFilename}, installer, and signature to ${updaterReleaseTag}.`);
}

function runGh(args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn('gh', args, {
      cwd: options.cwd ?? defaultRoot,
      stdio: options.stdio ?? (options.allowFailure ? 'ignore' : 'inherit'),
    });

    child.on('error', reject);
    child.on('exit', (code) => {
      if (code === 0 || options.allowFailure) {
        resolve(code ?? 1);
        return;
      }
      reject(new Error(`gh ${args.join(' ')} failed with exit code ${code}`));
    });
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    await publishUpdaterAlpha();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
