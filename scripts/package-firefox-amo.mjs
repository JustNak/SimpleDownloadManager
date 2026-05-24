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
    listingMetadataPath: path.join(packageRoot, 'AMO_LISTING_METADATA.json'),
    privacyPolicyPath: path.join(packageRoot, 'PRIVACY_POLICY.md'),
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
    { source: 'docs/privacy-policy.md' },
    { source: 'apps/extension' },
    { source: 'packages/protocol' },
  ];
}

export function createFirefoxAmoListingMetadata() {
  return {
    categories: {
      firefox: ['download-management'],
    },
    default_locale: 'en-US',
    description: {
      'en-US': [
        'Simple Download Manager connects Firefox downloads to the local Simple Download Manager desktop app.',
        '',
        'Use it to send eligible browser downloads to the desktop queue, choose between prompt and silent handoff modes, keep selected sites in Firefox, and opt in to Protected Downloads for sites that require browser session headers.',
        '',
        'Requires the Simple Download Manager desktop app and native messaging host to be installed on the same Windows device. The extension does not use remote code, analytics, advertising, tracking, or remote configuration.',
      ].join('\n'),
    },
    developer_comments: {
      'en-US': [
        'This listed AMO submission uses the generated Firefox ZIP at the archive root and includes a source ZIP for reviewer rebuilds.',
        'The extension requires a local native desktop app through Firefox native messaging; it does not transmit data to a remote server.',
        'See AMO_REVIEWER_NOTES.md, PRIVACY_POLICY.md, and apps/extension/FIREFOX_GUIDELINES.md for the detailed permission and data-transmission rationale.',
      ].join('\n'),
    },
    homepage: {
      'en-US': 'https://github.com/JustNak/SimpleDownloadManager',
    },
    is_experimental: false,
    name: {
      'en-US': 'Simple Download Manager',
    },
    requires_payment: false,
    slug: 'simple-download-manager',
    summary: {
      'en-US': 'Send Firefox downloads to the local Simple Download Manager desktop app.',
    },
    tags: [
      'download',
      'download-manager',
      'native-messaging',
    ],
    version: {
      license: 'all-rights-reserved',
      release_notes: {
        'en-US': createFirefoxAmoReleaseNotes(),
      },
      approval_notes: createFirefoxAmoReviewerNotes(),
    },
  };
}

export function createFirefoxAmoReleaseNotes() {
  return [
    'Improved Firefox download capture for server, Canvas/Instructure, blob/data, and <a download> flows.',
    'Captured downloads now return to Firefox only when the user clicks Swap.',
  ].join('\n');
}

export function createFirefoxAmoReadme(paths) {
  return `# Simple Download Manager Firefox AMO Upload

This directory contains the Firefox artifacts for public listing on addons.mozilla.org.

## Files

- Upload ZIP: ${paths.uploadZipPath}
- Source ZIP for review: ${paths.sourceZipPath}
- Listing metadata for web-ext listed submission: ${paths.listingMetadataPath}
- Privacy policy for the AMO listing: ${paths.privacyPolicyPath}
- Reviewer notes: ${paths.reviewerNotesPath}
- Firefox guideline file: apps/extension/FIREFOX_GUIDELINES.md

The upload ZIP contains extension files at the archive root. Do not use the temporary-test package for AMO upload.
The Firefox manifest sets strict_min_version to Firefox 142.0 so required data transmission is disclosed through Firefox's built-in install consent flow without Firefox Android compatibility warnings.

## Validate Locally

\`\`\`powershell
web-ext lint --source-dir apps\\extension\\dist\\firefox
\`\`\`

## Submit For Public AMO Listing

\`\`\`powershell
web-ext sign --source-dir apps\\extension\\dist\\firefox --channel=listed --amo-metadata=release\\firefox-amo\\AMO_LISTING_METADATA.json --api-key=$env:WEB_EXT_API_KEY --api-secret=$env:WEB_EXT_API_SECRET
\`\`\`

Use AMO API credentials from Developer Hub. The command submits the generated Firefox extension for public listing rather than downloading a self-distributed XPI.

## AMO UI Flow

Open AMO Developer Hub, submit a new add-on, choose "On this site", upload simple-download-manager-firefox-upload.zip, and upload simple-download-manager-firefox-source.zip when source is requested.
Use AMO_LISTING_METADATA.json as the source of truth for the listing summary, description, category, tags, homepage, and license. Paste PRIVACY_POLICY.md into the privacy policy field and AMO_REVIEWER_NOTES.md into the reviewer notes field so reviewers can verify the native messaging, download interception, and data transmission behavior quickly. If AMO asks for a support website, use https://github.com/JustNak/SimpleDownloadManager/issues. Keep apps/extension/FIREFOX_GUIDELINES.md in the source package for reviewer context.
`;
}

export function createFirefoxAmoPrivacyPolicy() {
  return `# Simple Download Manager Firefox Extension Privacy Policy

Simple Download Manager is a companion browser extension for the local Simple Download Manager desktop app.

## Data Sent To The Local Desktop App

When the extension is enabled and a download is eligible for handoff, it may send the following data from Firefox to the local native desktop app through Firefox native messaging:

- Download URL.
- Suggested filename and content length when Firefox exposes them.
- Page URL, page title, referrer, entry point, extension version, and incognito flag when available.
- User actions such as context-menu handoff, popup handoff, browser-download prompt, accepted handoff, canceled prompt, or fallback.
- Extension settings such as capture mode, excluded sites, ignored file extensions, Protected Downloads settings, badge preference, and progress-window preference.
- If Protected Downloads is enabled for a configured site, bounded browser request headers for the exact download being handed off, such as Cookie or Authorization headers after extension-side filtering.

## Local-Only Use

The extension sends this data only to the local native desktop app installed on the same device. The extension does not transmit data to a remote server, does not use analytics, does not use advertising, does not track users, and does not use remote configuration.

## Storage

The extension stores its settings in Firefox extension storage. Protected-download request headers are held only in extension memory for a short time, are capped, and are cleared when Protected Downloads is disabled.

## User Controls

Users can disable browser download interception, choose prompt or automatic handoff, exclude sites, ignore file extensions, disable Protected Downloads, and remove protected-download sites from the extension options page.
`;
}

export function createFirefoxAmoReviewerNotes() {
  return `# AMO Reviewer Notes

This is a public AMO listing for a companion extension to the local native desktop app. It uses Native messaging only. No remote code, analytics, tracking, ads, or remote config.

Permissions support download handoff: downloads manages captured items, webRequest/webRequestBlocking intercept qualifying Firefox download responses and exact protected-download headers, and <all_urls> covers arbitrary user download sites with local filtering.

Data disclosures include browsingActivity, websiteActivity, and websiteContent because download URL/page metadata, response headers, filename hints, and Protected Downloads headers for the exact browser download are sent to the local app only. Users can disable capture and configure wildcard excluded host patterns.

Strict handoff: captured downloads are canceled/discarded unless the user clicks Swap in the desktop prompt. Protected headers are memory-only, bounded, exact-request, and sent only to the local app.

Source package includes apps/extension/FIREFOX_GUIDELINES.md.
`;
}

export function createFirefoxAmoSourceReadme() {
  return `# Simple Download Manager Firefox Source Package

This source package contains the files required to reproduce the uploaded extension ZIP.
Firefox-specific review and packaging guidance is in apps/extension/FIREFOX_GUIDELINES.md. The listing privacy policy source is docs/privacy-policy.md.

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
  await writeFile(paths.listingMetadataPath, JSON.stringify(createFirefoxAmoListingMetadata(), null, 2), 'utf8');
  await writeFile(paths.privacyPolicyPath, createFirefoxAmoPrivacyPolicy(), 'utf8');
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

export async function copyFirefoxExtensionFiles(sourceDir, extensionDir) {
  await mkdir(extensionDir, { recursive: true });
  const entries = await readdir(sourceDir, { withFileTypes: true });

  for (const entry of entries) {
    await cp(path.join(sourceDir, entry.name), path.join(extensionDir, entry.name), { recursive: true });
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
