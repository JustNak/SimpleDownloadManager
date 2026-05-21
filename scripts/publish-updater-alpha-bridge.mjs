import { access, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import {
  releaseChannels,
  updaterReleasePaths,
} from './updater-release.mjs';
import {
  assertGitHubCliAvailable,
  ensureUpdaterRelease,
  missingGitHubCliMessage,
  runGh,
} from './publish-updater-beta.mjs';

const __filename = fileURLToPath(import.meta.url);
const defaultRoot = path.resolve(path.dirname(__filename), '..');

export async function publishUpdaterAlphaBridge(repoRoot = defaultRoot) {
  const packageJson = JSON.parse(await readFile(path.join(repoRoot, 'package.json'), 'utf8'));
  const paths = updaterReleasePaths(repoRoot, packageJson.version, releaseChannels.alphaBridge);
  const uploadPaths = updaterAlphaBridgeUploadPaths(paths);

  try {
    await assertGitHubCliAvailable((args, options) => runGh(args, { ...options, cwd: repoRoot }));
  } catch (error) {
    if (error instanceof Error && /GitHub CLI \(gh\) was not found/.test(error.message)) {
      throw new Error(missingGitHubCliMessage('npm run publish:updater-alpha-bridge'));
    }
    throw error;
  }

  await Promise.all(uploadPaths.map((uploadPath) => access(uploadPath)));
  await ensureUpdaterRelease(releaseChannels.alphaBridge, repoRoot);

  await runGh([
    'release',
    'upload',
    releaseChannels.alphaBridge.metadataReleaseTag,
    ...uploadPaths,
    '--clobber',
  ], { cwd: repoRoot });

  console.log(`Uploaded ${releaseChannels.alphaBridge.metadataFilename} to ${releaseChannels.alphaBridge.metadataReleaseTag}.`);
}

export function updaterAlphaBridgeUploadPaths(paths) {
  return [paths.metadataPath];
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    await publishUpdaterAlphaBridge();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
