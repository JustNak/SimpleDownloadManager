import { cp, mkdir, readdir, rm, stat, writeFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const defaultRepoRoot = path.resolve(__dirname, '..');

export function firefoxTestPackagePaths(repoRoot = defaultRepoRoot) {
  const packageRoot = path.join(repoRoot, 'release', 'firefox-test');
  return {
    repoRoot,
    sourceDir: path.join(repoRoot, 'apps', 'extension', 'dist', 'firefox'),
    packageRoot,
    extensionDir: path.join(packageRoot, 'extension'),
    zipPath: path.join(packageRoot, 'simple-download-manager-firefox-test.zip'),
    readmePath: path.join(packageRoot, 'README.md'),
  };
}

export function createFirefoxTestReadme(paths) {
  return `# Simple Download Manager Firefox Test Bundle

This is an unsigned temporary-test bundle for Firefox.

## Load In Firefox

1. Open Firefox.
2. Go to about:debugging#/runtime/this-firefox.
3. Click "Load Temporary Add-on".
4. Select one of these:
   - ${path.join(paths.extensionDir, 'manifest.json')}
   - ${paths.zipPath}

Firefox temporary add-ons are removed after the browser restarts unless you load them again.

## Notes

- The ZIP contains the extension files at the archive root, not inside a parent folder.
- Standard persistent Firefox installation requires Mozilla signing.
- This package intentionally does not include Mozilla signing or an installable signed XPI.
`;
}

export async function packageFirefoxTest(repoRoot = defaultRepoRoot) {
  const paths = firefoxTestPackagePaths(repoRoot);
  await assertFirefoxBuildExists(paths.sourceDir);
  await mkdir(paths.packageRoot, { recursive: true });
  await syncFirefoxExtensionFiles(paths.sourceDir, paths.extensionDir);
  await writeFile(paths.readmePath, createFirefoxTestReadme(paths), 'utf8');
  await createZipFromDirectory(paths.extensionDir, paths.zipPath);
  return paths;
}

async function assertFirefoxBuildExists(sourceDir) {
  await stat(path.join(sourceDir, 'manifest.json')).catch(() => {
    throw new Error(`Firefox build output was not found at ${sourceDir}. Run npm run build:extension first.`);
  });
}

export async function syncFirefoxExtensionFiles(sourceDir, extensionDir) {
  await rm(extensionDir, { recursive: true, force: true });
  await mkdir(extensionDir, { recursive: true });
  const sourceEntries = await readdir(sourceDir, { withFileTypes: true });

  for (const entry of sourceEntries) {
    await cp(path.join(sourceDir, entry.name), path.join(extensionDir, entry.name), { recursive: true });
  }
}

async function createZipFromDirectory(sourceDir, zipPath) {
  if (process.platform !== 'win32') {
    throw new Error('Firefox test packaging currently uses PowerShell Compress-Archive on Windows.');
  }

  await runPowerShell([
    '-NoProfile',
    '-ExecutionPolicy',
    'Bypass',
    '-Command',
    [
      '$ErrorActionPreference = "Stop"',
      `Set-Location -LiteralPath ${quotePowerShellLiteral(sourceDir)}`,
      `Compress-Archive -Path * -DestinationPath ${quotePowerShellLiteral(zipPath)} -Force`,
    ].join('; '),
  ]);
}

function quotePowerShellLiteral(value) {
  return `'${value.replaceAll("'", "''")}'`;
}

function runPowerShell(args) {
  return new Promise((resolve, reject) => {
    const child = spawn('powershell.exe', args, {
      cwd: defaultRepoRoot,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';

    child.stdout.on('data', (chunk) => {
      stdout += chunk;
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk;
    });
    child.on('error', reject);
    child.on('exit', (code) => {
      if (code === 0) {
        resolve();
        return;
      }

      reject(new Error(`PowerShell failed with exit code ${code}.\n${stdout}${stderr}`));
    });
  });
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const paths = await packageFirefoxTest();
  console.log(`Firefox test package written to ${paths.packageRoot}`);
  console.log(`Unpacked extension: ${paths.extensionDir}`);
  console.log(`Temporary ZIP: ${paths.zipPath}`);
}
