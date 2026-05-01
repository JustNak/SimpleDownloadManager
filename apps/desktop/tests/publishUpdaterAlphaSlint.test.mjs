import assert from 'node:assert/strict';
import { mkdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';

let publishUpdaterAlphaSlint;
try {
  publishUpdaterAlphaSlint = await import('../../../scripts/publish-updater-alpha-slint.mjs');
} catch (error) {
  assert.fail(`Slint updater publish helper should be importable without running gh: ${error instanceof Error ? error.message : error}`);
}

const {
  collectSlintPublishArtifacts,
  publishUpdaterAlphaSlint: publishSlint,
} = publishUpdaterAlphaSlint;

const root = path.resolve('.tmp', `sdm-slint-publish-${process.pid}`);

try {
  await rm(root, { recursive: true, force: true });
  await createValidFixture();

  const artifacts = await collectSlintPublishArtifacts({ repoRoot: root });
  assert.equal(artifacts.releaseTag, 'updater-alpha');
  assert.deepEqual(
    artifacts.uploadPaths.map((artifactPath) => path.relative(root, artifactPath).replaceAll(path.sep, '/')),
    [
      'release/slint/bundle/nsis/simple-download-manager_0.3.48-alpha_x64-setup.exe',
      'release/slint/bundle/nsis/simple-download-manager_0.3.48-alpha_x64-setup.exe.sig',
      'release/slint/latest-alpha.json',
      'release/slint/latest-alpha-slint.json',
    ],
    'Slint publish helper should upload only Slint installer/signature and both Slint feeds',
  );

  const dryRun = await publishSlint({
    repoRoot: root,
    dryRun: true,
    log: () => {},
    runCommand: async () => {
      assert.fail('dry-run should not execute gh commands');
    },
  });
  assert.equal(dryRun.dryRun, true);
  assert.equal(dryRun.uploadPaths.length, 4);

  const commands = [];
  await publishSlint({
    repoRoot: root,
    log: () => {},
    runCommand: async (args) => {
      commands.push(args);
      return 0;
    },
  });

  assert.deepEqual(commands[0], ['--version']);
  assert.deepEqual(commands[1], ['release', 'view', 'updater-alpha']);
  assert.equal(commands[2][0], 'release');
  assert.equal(commands[2][1], 'upload');
  assert.equal(commands[2][2], 'updater-alpha');
  assert.equal(commands[2].at(-1), '--clobber');
  assert(commands[2].some((arg) => arg.endsWith('latest-alpha.json')));
  assert(commands[2].some((arg) => arg.endsWith('latest-alpha-slint.json')));
  assert(
    commands[2].every((arg) => !String(arg).includes('Simple Download Manager_')),
    'Slint publish upload should not reference the legacy Tauri installer name',
  );
} finally {
  await rm(root, { recursive: true, force: true });
}

async function createValidFixture() {
  await writeText('package.json', JSON.stringify({ version: '0.3.48-alpha' }));
  await writeText('release/slint/bundle/nsis/simple-download-manager_0.3.48-alpha_x64-setup.exe', 'installer');
  await writeText('release/slint/bundle/nsis/simple-download-manager_0.3.48-alpha_x64-setup.exe.sig', 'signature');
  await writeText('release/slint/staging/simple-download-manager-native-host.exe', 'native-host');
  await writeText('release/slint/staging/resources/install/install.md', 'install docs');
  await writeText('release/slint/staging/resources/install/register-native-host.ps1', 'register');
  await writeText('release/slint/staging/resources/install/unregister-native-host.ps1', 'unregister');
  await writeText('release/slint/staging/resources/install/chromium.template.json', '{}');
  await writeText('release/slint/staging/resources/install/edge.template.json', '{}');
  await writeText('release/slint/staging/resources/install/firefox.template.json', '{}');
  await writeText('release/slint/staging/resources/install/release.json', '{}');
  await writeText('release/slint/simple-download-manager-chromium-extension.zip', 'chromium');
  await writeText('release/slint/simple-download-manager-firefox-extension.zip', 'firefox');
  await writeText('release/slint/latest-alpha.json', JSON.stringify({
    version: '0.3.48-alpha',
    platforms: {
      'windows-x86_64': {
        url: 'https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-alpha/simple-download-manager_0.3.48-alpha_x64-setup.exe',
        signature: 'signature',
      },
    },
  }));
  await writeText('release/slint/latest-alpha-slint.json', JSON.stringify({
    version: '0.3.48-alpha',
    platforms: {
      'windows-x86_64': {
        url: 'https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-alpha/simple-download-manager_0.3.48-alpha_x64-setup.exe',
        signature: 'signature',
        format: 'nsis',
      },
    },
  }));
}

async function writeText(relativePath, content) {
  const target = path.join(root, relativePath);
  await mkdir(path.dirname(target), { recursive: true });
  await writeFile(target, content, 'utf8');
}
