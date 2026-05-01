import assert from 'node:assert/strict';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';

let verifierModule;
try {
  verifierModule = await import('../../../scripts/verify-release-slint.mjs');
} catch (error) {
  assert.fail(`Slint release verifier should exist: ${error instanceof Error ? error.message : error}`);
}

const {
  verifySlintReleaseArtifacts,
  slintRequiredArtifactPaths,
} = verifierModule;
const root = path.resolve('.tmp', `sdm-slint-verify-${process.pid}`);

try {
  await rm(root, { recursive: true, force: true });
  await createValidFixture(root);

  const paths = await slintRequiredArtifactPaths({ root });
  assert.equal(
    path.relative(root, paths.installerPath).replaceAll(path.sep, '/'),
    'release/slint/bundle/nsis/simple-download-manager_0.3.48-alpha_x64-setup.exe',
    'verifier should expect the Slint cargo-packager installer name',
  );

  const result = await verifySlintReleaseArtifacts({ root });
  assert.equal(result.installerName, 'simple-download-manager_0.3.48-alpha_x64-setup.exe');
  assert.equal(result.metadataPath, path.join(root, 'release', 'slint', 'latest-alpha-slint.json'));
  assert.equal(result.transitionMetadataPath, path.join(root, 'release', 'slint', 'latest-alpha.json'));

  await rm(paths.installerPath);
  await assert.rejects(
    () => verifySlintReleaseArtifacts({ root }),
    /Missing Slint release artifact: installer .*simple-download-manager_0\.3\.48-alpha_x64-setup\.exe/,
    'missing installer should report the installer path',
  );

  await createValidFixture(root);
  await rm(paths.signaturePath);
  await assert.rejects(
    () => verifySlintReleaseArtifacts({ root }),
    /Missing Slint release artifact: installer signature .*\.exe\.sig/,
    'missing signature should report the signature path',
  );

  await createValidFixture(root);
  await rm(paths.stagedSidecarPath);
  await assert.rejects(
    () => verifySlintReleaseArtifacts({ root }),
    /Missing Slint release artifact: staged native-host sidecar .*simple-download-manager-native-host\.exe/,
    'missing sidecar should report the staged native-host sidecar path',
  );

  await createValidFixture(root);
  await rm(paths.transitionMetadataPath);
  await assert.rejects(
    () => verifySlintReleaseArtifacts({ root }),
    /Missing Slint release artifact: Tauri transition updater metadata .*latest-alpha\.json/,
    'missing transition metadata should report the transition feed path',
  );

  await createValidFixture(root);
  await writeText('release/slint/latest-alpha.json', JSON.stringify({
    version: '0.3.48-alpha',
    platforms: {
      'windows-x86_64': {
        url: 'https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-alpha/Simple.Download.Manager_0.3.48-alpha_x64-setup.exe',
        signature: 'signature',
      },
    },
  }));
  await assert.rejects(
    () => verifySlintReleaseArtifacts({ root }),
    /Tauri transition updater metadata references Simple\.Download\.Manager_0\.3\.48-alpha_x64-setup\.exe; expected simple-download-manager_0\.3\.48-alpha_x64-setup\.exe/,
    'transition metadata should reference the Slint installer, not the legacy Tauri artifact',
  );

  await createValidFixture(root);
  await writeText('release/slint/latest-alpha.json', JSON.stringify({
    version: '0.3.48-alpha',
    platforms: {
      'windows-x86_64': {
        url: 'https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-alpha/simple-download-manager_0.3.48-alpha_x64-setup.exe',
        signature: 'signature',
        format: 'nsis',
      },
    },
  }));
  await assert.rejects(
    () => verifySlintReleaseArtifacts({ root }),
    /Tauri transition updater metadata must not include a format field/,
    'transition metadata should keep the Tauri-compatible feed shape',
  );
} finally {
  await rm(root, { recursive: true, force: true });
}

async function createValidFixture(fixtureRoot) {
  await rm(fixtureRoot, { recursive: true, force: true });
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
