import { access, readFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import {
  linuxBundleArtifactNames,
  releaseChannels,
  updaterReleasePaths,
} from './updater-release.mjs';

const __filename = fileURLToPath(import.meta.url);
const defaultRoot = path.resolve(path.dirname(__filename), '..');

export function missingGitHubCliMessage(command = 'npm run publish:updater-beta') {
  return [
    'GitHub CLI (gh) was not found on PATH.',
    'Install it from https://cli.github.com/ or with: winget install --id GitHub.cli -e',
    'Then authenticate with: gh auth login',
    `After that, rerun: ${command}`,
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

export async function ensureUpdaterRelease(channel, repoRoot = defaultRoot) {
  const releaseExists = await runGh(['release', 'view', channel.metadataReleaseTag], { allowFailure: true, cwd: repoRoot });
  if (releaseExists === 0) return;

  await runGh([
    'release',
    'create',
    channel.metadataReleaseTag,
    '--title',
    channel.releaseTitle,
    '--notes',
    channel.releaseNotes,
    '--prerelease',
    '--latest=false',
  ], { cwd: repoRoot });
}

export async function publishUpdaterBeta(repoRoot = defaultRoot) {
  const packageJson = JSON.parse(await readFile(path.join(repoRoot, 'package.json'), 'utf8'));
  const paths = updaterReleasePaths(repoRoot, packageJson.version, releaseChannels.beta);
  const uploadPaths = updaterBetaUploadPaths(paths);

  await assertGitHubCliAvailable((args, options) => runGh(args, { ...options, cwd: repoRoot }));

  await Promise.all(uploadPaths.map((uploadPath) => access(uploadPath)));

  await ensureUpdaterRelease(releaseChannels.beta, repoRoot);

  await runGh([
    'release',
    'upload',
    releaseChannels.beta.metadataReleaseTag,
    ...uploadPaths,
    '--clobber',
  ], { cwd: repoRoot });

  console.log(`Uploaded ${releaseChannels.beta.metadataFilename}, installer, and signature to ${releaseChannels.beta.metadataReleaseTag}.`);
}

export function updaterBetaUploadPaths(paths) {
  const uploadPaths = paths.installers.flatMap((installer) => [
    installer.installerPath,
    installer.signaturePath,
  ]);

  for (const installer of paths.installers) {
    if (!installer.target.updaterPlatform?.startsWith('linux-')) {
      continue;
    }

    const packageNames = linuxBundleArtifactNames(versionFromLinuxAppImageName(installer.installerName), installer.target);
    uploadPaths.push(
      path.join(paths.releaseRoot, 'bundle', 'deb', packageNames.deb),
      path.join(paths.releaseRoot, 'bundle', 'rpm', packageNames.rpm),
    );
  }

  return [
    ...uploadPaths,
    paths.metadataPath,
  ];
}

function versionFromLinuxAppImageName(installerName) {
  const match = installerName.match(/^Simple Download Manager_(.+)_amd64\.AppImage$/);
  if (!match) {
    throw new Error(`Unsupported Linux AppImage asset name: ${installerName}`);
  }
  return match[1];
}

export function runGh(args, options = {}) {
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
    await publishUpdaterBeta();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
