import { pathToFileURL } from 'node:url';
import { publishUpdaterAlphaBridge } from './publish-updater-alpha-bridge.mjs';
export {
  assertGitHubCliAvailable,
  isMissingGitHubCliError,
  missingGitHubCliMessage,
} from './publish-updater-beta.mjs';

export async function publishUpdaterAlpha(repoRoot) {
  return publishUpdaterAlphaBridge(repoRoot);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    await publishUpdaterAlpha();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
