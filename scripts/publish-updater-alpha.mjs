import { access, readFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  updaterMetadataFilename,
  updaterReleasePaths,
  updaterReleaseTag,
} from './updater-release.mjs';

const __filename = fileURLToPath(import.meta.url);
const root = path.resolve(path.dirname(__filename), '..');
const packageJson = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
const paths = updaterReleasePaths(root, packageJson.version);

await Promise.all([
  access(paths.installerPath),
  access(paths.signaturePath),
  access(paths.metadataPath),
]);

const releaseExists = await runGh(['release', 'view', updaterReleaseTag], { allowFailure: true });
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
  ]);
}

await runGh([
  'release',
  'upload',
  updaterReleaseTag,
  paths.installerPath,
  paths.signaturePath,
  paths.metadataPath,
  '--clobber',
]);

console.log(`Uploaded ${updaterMetadataFilename}, installer, and signature to ${updaterReleaseTag}.`);

function runGh(args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn('gh', args, {
      cwd: root,
      stdio: options.allowFailure ? 'ignore' : 'inherit',
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
