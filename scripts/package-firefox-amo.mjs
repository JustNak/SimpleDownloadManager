import { cp, mkdir, readdir, rm, stat, writeFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const defaultRepoRoot = path.resolve(__dirname, '..');
const excludedSourceSegments = new Set(['node_modules', 'dist', 'release', '.tmp', 'target']);

export function firefoxAmoPackagePaths(repoRoot = defaultRepoRoot) {
  const packageRoot = path.join(repoRoot, 'release', 'firefox-amo');
  return {
    repoRoot,
    sourceDir: path.join(repoRoot, 'apps', 'extension', 'dist', 'firefox'),
    packageRoot,
    uploadDir: path.join(packageRoot, 'upload'),
    sourceReviewDir: path.join(packageRoot, 'source'),
    uploadZipPath: path.join(packageRoot, 'simple-download-manager-firefox-upload.zip'),
    sourceZipPath: path.join(packageRoot, 'simple-download-manager-firefox-source.zip'),
    readmePath: path.join(packageRoot, 'README.md'),
    reviewerNotesPath: path.join(packageRoot, 'AMO_REVIEWER_NOTES.md'),
    sourceReadmePath: path.join(packageRoot, 'source', 'AMO_SOURCE_REVIEW.md'),
  };
}

export function firefoxAmoSourceEntries() {
  return [
    { source: 'package.json' },
    { source: 'package-lock.json' },
    { source: 'tsconfig.base.json' },
    { source: 'config/release.json' },
    { source: 'apps/extension' },
    { source: 'packages/protocol' },
  ];
}

export function createFirefoxAmoReadme(paths) {
  return `# Simple Download Manager Firefox AMO Upload

This directory contains the Firefox artifacts for unlisted Mozilla signing.

## Files

- Upload ZIP: ${paths.uploadZipPath}
- Source ZIP for review: ${paths.sourceZipPath}
- Reviewer notes: ${paths.reviewerNotesPath}

The upload ZIP contains extension files at the archive root. Do not use the temporary-test package for AMO upload.
The Firefox manifest sets strict_min_version to Firefox 142.0 so required data transmission is disclosed through Firefox's built-in install consent flow without Firefox Android compatibility warnings.

## Validate Locally

\`\`\`powershell
web-ext lint --source-dir apps\\extension\\dist\\firefox
\`\`\`

## Sign For Self-Distribution

\`\`\`powershell
web-ext sign --source-dir apps\\extension\\dist\\firefox --channel=unlisted --api-key=$env:AMO_JWT_ISSUER --api-secret=$env:AMO_JWT_SECRET
\`\`\`

## AMO UI Flow

Open AMO Developer Hub, submit a new add-on, choose "On your own", upload simple-download-manager-firefox-upload.zip, upload simple-download-manager-firefox-source.zip if source is requested, then download the signed XPI.
Paste AMO_REVIEWER_NOTES.md into the reviewer notes field so reviewers can verify the native messaging, download interception, and data transmission behavior quickly.
`;
}

export function createFirefoxAmoReviewerNotes() {
  return `# AMO Reviewer Notes

Simple Download Manager is a companion extension for a local native desktop app. No remote code is used. The extension does not use remote configuration, analytics, advertising, or tracking.

## Core Functionality

- Detects user-initiated HTTP(S) browser downloads.
- Sends eligible download URLs, suggested filename hints, content length when available, incognito flag, and user download actions to the local native desktop app through Firefox Native messaging.
- Cancels the original browser download only after the desktop app accepts or queues it. If the user cancels the desktop prompt or the native host is unavailable, the extension restarts the browser download as fallback.
- Provides a context menu and popup/options UI for manually sending URLs to the desktop app.

## Permission Rationale

- Native messaging: required to communicate with the local native desktop app.
- downloads: required to observe, cancel, remove, erase, and restart browser downloads during managed handoff/fallback.
- webRequest and webRequestBlocking: required in Firefox to intercept qualifying attachment/download responses before Firefox opens its default Save As dialog.
- <all_urls>: required because download links can originate from arbitrary HTTP(S) sites; filtering happens in extension code by scheme, excluded host list, wildcard excluded host patterns, ignored extension list, and user settings.
- storage: required to store extension settings such as enabled state, handoff mode, excluded hosts, ignored extensions, and badge preference.
- contextMenus: required for the "Download with Simple Download Manager" link menu action.

## Data Collection Disclosure

The manifest declares required data_collection_permissions for browsingActivity, websiteActivity, and websiteContent because download URLs, page/referrer metadata when available, filename hints, response headers, content length, and download actions are transmitted outside Firefox to the local native desktop app. This transmission is required for the extension to perform download handoff.

The data is sent to the local native desktop app only. The extension does not transmit this data to a remote server.

## User Controls

- Users can disable browser download interception.
- Users can switch handoff mode between prompt and automatic queueing.
- Users can add excluded hosts, wildcard excluded host patterns, and ignored file extensions.
- web.telegram.org is excluded by default.
`;
}

export function createFirefoxAmoSourceReadme() {
  return `# Simple Download Manager Firefox Source Package

This source package contains the files required to reproduce the uploaded extension ZIP.

## Rebuild

\`\`\`powershell
npm ci
npm run build --workspace @myapp/extension
\`\`\`

The rebuilt Firefox output is written to apps/extension/dist/firefox. Compare that directory with the uploaded extension ZIP.

Generated folders such as node_modules, dist, release, .tmp, and Rust target directories are intentionally excluded.
`;
}

export async function packageFirefoxAmo(repoRoot = defaultRepoRoot) {
  const paths = firefoxAmoPackagePaths(repoRoot);
  await assertFirefoxBuildExists(paths.sourceDir);
  await rm(paths.packageRoot, { recursive: true, force: true });
  await mkdir(paths.packageRoot, { recursive: true });
  await copyFirefoxExtensionFiles(paths.sourceDir, paths.uploadDir);
  await createSourceReviewPackage(paths);
  await writeFile(paths.readmePath, createFirefoxAmoReadme(paths), 'utf8');
  await writeFile(paths.reviewerNotesPath, createFirefoxAmoReviewerNotes(), 'utf8');
  await createZipFromDirectory(paths.uploadDir, paths.uploadZipPath);
  await createZipFromDirectory(paths.sourceReviewDir, paths.sourceZipPath);
  return paths;
}

async function assertFirefoxBuildExists(sourceDir) {
  await stat(path.join(sourceDir, 'manifest.json')).catch(() => {
    throw new Error(`Firefox build output was not found at ${sourceDir}. Run npm run build:extension first.`);
  });
}

async function copyFirefoxExtensionFiles(sourceDir, extensionDir) {
  await mkdir(extensionDir, { recursive: true });
  const entries = await readdir(sourceDir, { withFileTypes: true });

  for (const entry of entries) {
    if (!entry.isFile()) {
      continue;
    }

    await cp(path.join(sourceDir, entry.name), path.join(extensionDir, entry.name));
  }
}

async function createSourceReviewPackage(paths) {
  await mkdir(paths.sourceReviewDir, { recursive: true });

  for (const entry of firefoxAmoSourceEntries()) {
    const sourcePath = path.join(paths.repoRoot, entry.source);
    const destinationPath = path.join(paths.sourceReviewDir, entry.source);
    await mkdir(path.dirname(destinationPath), { recursive: true });
    await cp(sourcePath, destinationPath, {
      recursive: true,
      filter: (source) => !hasExcludedSourceSegment(paths.repoRoot, source),
    });
  }

  await writeFile(paths.sourceReadmePath, createFirefoxAmoSourceReadme(), 'utf8');
}

function hasExcludedSourceSegment(repoRoot, sourcePath) {
  const relativePath = path.relative(repoRoot, sourcePath);
  return relativePath.split(path.sep).some((segment) => excludedSourceSegments.has(segment));
}

async function createZipFromDirectory(sourceDir, zipPath) {
  if (process.platform !== 'win32') {
    throw new Error('Firefox AMO packaging currently uses PowerShell Compress-Archive on Windows.');
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
  const paths = await packageFirefoxAmo();
  console.log(`Firefox AMO package written to ${paths.packageRoot}`);
  console.log(`Upload ZIP: ${paths.uploadZipPath}`);
  console.log(`Source ZIP: ${paths.sourceZipPath}`);
}
