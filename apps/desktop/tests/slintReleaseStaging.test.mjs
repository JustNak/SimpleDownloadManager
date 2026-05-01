import assert from 'node:assert/strict';
import { mkdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';

let releaseHelper;
try {
  releaseHelper = await import('../../../scripts/prepare-release-slint.mjs');
} catch (error) {
  assert.fail(`Slint release staging helper should exist: ${error instanceof Error ? error.message : error}`);
}

const { prepareSlintReleaseResources } = releaseHelper;
const tempRoot = path.resolve('.tmp', `sdm-slint-staging-${process.pid}`);

try {
  await rm(tempRoot, { recursive: true, force: true });
  await mkdir(tempRoot, { recursive: true });

  await createFixtureFile('config/release.json', JSON.stringify({
    nativeHostName: 'com.myapp.download_manager',
    chromiumExtensionId: 'chromium-id',
    edgeExtensionId: 'edge-id',
    firefoxExtensionId: 'firefox-id',
  }, null, 2));
  await createFixtureFile('docs/install.md', '# Install docs\n');
  await createFixtureFile('scripts/register-native-host.ps1', 'register script\n');
  await createFixtureFile('scripts/unregister-native-host.ps1', 'unregister script\n');
  await createFixtureFile('apps/native-host/manifests/chromium.template.json', '{"browser":"chromium"}\n');
  await createFixtureFile('apps/native-host/manifests/edge.template.json', '{"browser":"edge"}\n');
  await createFixtureFile('apps/native-host/manifests/firefox.template.json', '{"browser":"firefox"}\n');
  await createFixtureFile('apps/native-host/target/release/simple-download-manager-native-host.exe', 'native-host-binary');

  const result = await prepareSlintReleaseResources({ root: tempRoot });

  assert.equal(
    path.relative(tempRoot, result.stagingRoot).replaceAll(path.sep, '/'),
    'release/slint/staging',
    'Slint staging root should live under release/slint/staging',
  );
  assert.equal(
    path.relative(tempRoot, result.installResourceDir).replaceAll(path.sep, '/'),
    'release/slint/staging/resources/install',
    'install resources should be staged under resources/install',
  );

  await assertFile(result.sidecarTarget, 'native-host-binary');
  await assertFile(path.join(result.installResourceDir, 'install.md'), '# Install docs\n');
  await assertFile(path.join(result.installResourceDir, 'register-native-host.ps1'), 'register script\n');
  await assertFile(path.join(result.installResourceDir, 'unregister-native-host.ps1'), 'unregister script\n');
  await assertFile(path.join(result.installResourceDir, 'chromium.template.json'), '{"browser":"chromium"}\n');
  await assertFile(path.join(result.installResourceDir, 'edge.template.json'), '{"browser":"edge"}\n');
  await assertFile(path.join(result.installResourceDir, 'firefox.template.json'), '{"browser":"firefox"}\n');

  const releaseJson = JSON.parse(await readFile(path.join(result.installResourceDir, 'release.json'), 'utf8'));
  assert.equal(releaseJson.nativeHostName, 'com.myapp.download_manager');
  assert.equal(releaseJson.chromiumExtensionId, 'chromium-id');
  assert.equal(releaseJson.edgeExtensionId, 'edge-id');
  assert.equal(releaseJson.firefoxExtensionId, 'firefox-id');
  assert.equal(
    releaseJson.sidecarBinaryName,
    'simple-download-manager-native-host.exe',
    'Slint release metadata should point at the un-suffixed sidecar staged beside the app',
  );

  await assertMissing(path.join(tempRoot, 'apps/desktop/src-tauri/resources/install/release.json'));
} finally {
  await rm(tempRoot, { recursive: true, force: true });
}

async function createFixtureFile(relativePath, content) {
  const fullPath = path.join(tempRoot, relativePath);
  await mkdir(path.dirname(fullPath), { recursive: true });
  await writeFile(fullPath, content, 'utf8');
}

async function assertFile(fullPath, expectedContent) {
  assert.equal(await readFile(fullPath, 'utf8'), expectedContent);
}

async function assertMissing(fullPath) {
  await assert.rejects(
    () => stat(fullPath),
    /ENOENT/,
    `${fullPath} should not be created by Slint staging`,
  );
}
