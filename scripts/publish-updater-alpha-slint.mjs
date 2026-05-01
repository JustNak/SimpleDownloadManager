import { access } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import {
  slintRequiredArtifactPaths,
  verifySlintReleaseArtifacts,
} from './verify-release-slint.mjs';
import {
  slintUpdaterMetadataFilename,
  updaterMetadataFilename,
  updaterReleaseTag,
} from './updater-release.mjs';
const __filename = fileURLToPath(import.meta.url);
const defaultRoot = path.resolve(path.dirname(__filename), '..');
const transitionFeedName = updaterMetadataFilename; // latest-alpha.json for existing Tauri clients.
const nativeFeedName = slintUpdaterMetadataFilename; // latest-alpha-slint.json for Slint-native clients.

export function missingGitHubCliMessage() {
  return [
    'GitHub CLI (gh) was not found on PATH.',
    'Install it from https://cli.github.com/ or with: winget install --id GitHub.cli -e',
    'Then authenticate with: gh auth login',
    'After that, rerun: npm run publish:updater-alpha:slint',
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

export async function collectSlintPublishArtifacts({ repoRoot = defaultRoot } = {}) {
  const verified = await verifySlintReleaseArtifacts({ root: repoRoot });
  const paths = await slintRequiredArtifactPaths({ root: repoRoot });
  const uploadPaths = [
    verified.installerPath,
    verified.signaturePath,
    paths.transitionMetadataPath,
    paths.metadataPath,
  ];

  await Promise.all(uploadPaths.map((artifactPath) => access(artifactPath)));

  return {
    releaseTag: updaterReleaseTag,
    uploadPaths,
  };
}

export async function publishUpdaterAlphaSlint({
  repoRoot = defaultRoot,
  dryRun = false,
  runCommand = runGh,
  log = console.log,
} = {}) {
  const artifacts = await collectSlintPublishArtifacts({ repoRoot });

  if (dryRun) {
    log(`Dry run: would upload ${transitionFeedName}, ${nativeFeedName}, installer, and signature to ${artifacts.releaseTag}.`);
    return {
      dryRun: true,
      ...artifacts,
    };
  }

  await assertGitHubCliAvailable((args, options) => runCommand(args, { ...options, cwd: repoRoot }));

  const releaseExists = await runCommand(
    ['release', 'view', artifacts.releaseTag],
    { allowFailure: true, cwd: repoRoot },
  );
  if (releaseExists !== 0) {
    await runCommand([
      'release',
      'create',
      artifacts.releaseTag,
      '--title',
      'Simple Download Manager Alpha Updates',
      '--notes',
      'Static updater feed for alpha builds.',
      '--prerelease',
      '--latest=false',
    ], { cwd: repoRoot });
  }

  await runCommand([
    'release',
    'upload',
    artifacts.releaseTag,
    ...artifacts.uploadPaths,
    '--clobber',
  ], { cwd: repoRoot });

  log(`Uploaded Slint transition/native updater feeds, installer, and signature to ${artifacts.releaseTag}.`);
  return {
    dryRun: false,
    ...artifacts,
  };
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

function parseCliArgs(argv) {
  return {
    dryRun: argv.includes('--dry-run'),
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const { dryRun } = parseCliArgs(process.argv.slice(2));
    await publishUpdaterAlphaSlint({ dryRun });
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
