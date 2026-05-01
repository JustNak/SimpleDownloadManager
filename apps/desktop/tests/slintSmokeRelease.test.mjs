import assert from 'node:assert/strict';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';

let smokeModule;
try {
  smokeModule = await import('../../../scripts/smoke-release-slint.mjs');
} catch (error) {
  assert.fail(`Slint installer smoke helper should exist: ${error instanceof Error ? error.message : error}`);
}

const {
  nativeHostRegistryTargets,
  slintSmokeInstallLayout,
  validateInstalledNativeHostLayout,
} = smokeModule;

const installRoot = path.resolve('.tmp', `sdm-slint-smoke-${process.pid}`);

try {
  await rm(installRoot, { recursive: true, force: true });
  await createValidInstalledFixture();

  const layout = slintSmokeInstallLayout(installRoot);
  assert.equal(
    path.relative(installRoot, layout.appExe).replaceAll(path.sep, '/'),
    'simple-download-manager.exe',
    'smoke layout should resolve the installed Slint app exe',
  );
  assert.equal(
    path.relative(installRoot, layout.sidecar).replaceAll(path.sep, '/'),
    'simple-download-manager-native-host.exe',
    'smoke layout should resolve the installed native-host sidecar',
  );
  assert.equal(
    path.relative(installRoot, layout.resources.installDir).replaceAll(path.sep, '/'),
    'resources/install',
    'smoke layout should resolve staged install resources',
  );
  assert.equal(
    path.relative(installRoot, layout.manifests.chrome).replaceAll(path.sep, '/'),
    'native-messaging/com.myapp.download_manager.chrome.json',
    'smoke layout should resolve the Chrome manifest path',
  );
  assert.equal(
    path.relative(installRoot, layout.uninstaller).replaceAll(path.sep, '/'),
    'uninstall.exe',
    'smoke layout should resolve the NSIS uninstaller',
  );

  assert.deepEqual(nativeHostRegistryTargets(installRoot), [
    {
      browser: 'Chrome',
      key: 'Software\\Google\\Chrome\\NativeMessagingHosts\\com.myapp.download_manager',
      manifest: layout.manifests.chrome,
    },
    {
      browser: 'Edge',
      key: 'Software\\Microsoft\\Edge\\NativeMessagingHosts\\com.myapp.download_manager',
      manifest: layout.manifests.edge,
    },
    {
      browser: 'Firefox',
      key: 'Software\\Mozilla\\NativeMessagingHosts\\com.myapp.download_manager',
      manifest: layout.manifests.firefox,
    },
  ]);

  const validation = await validateInstalledNativeHostLayout({ installRoot });
  assert.equal(validation.sidecarPath, layout.sidecar);
  assert.equal(validation.manifests.length, 3);
  assert.equal(validation.manifests[0].browser, 'Chrome');

  await rm(layout.sidecar);
  await assert.rejects(
    () => validateInstalledNativeHostLayout({ installRoot }),
    /Missing installed Slint native-host sidecar .*simple-download-manager-native-host\.exe/,
    'missing installed sidecar should report the sidecar path',
  );

  await createValidInstalledFixture();
  await rm(layout.manifests.edge);
  await assert.rejects(
    () => validateInstalledNativeHostLayout({ installRoot }),
    /Missing Edge native-host manifest .*com\.myapp\.download_manager\.edge\.json/,
    'missing manifest should report the browser and path',
  );

  await createValidInstalledFixture();
  await writeFile(layout.manifests.chrome, '{not-json', 'utf8');
  await assert.rejects(
    () => validateInstalledNativeHostLayout({ installRoot }),
    /Invalid Chrome native-host manifest JSON/,
    'invalid manifest JSON should report the browser name',
  );

  await createValidInstalledFixture();
  await writeManifest(layout.manifests.firefox, {
    name: 'com.myapp.download_manager',
    description: 'Simple Download Manager native messaging host',
    path: path.join(installRoot, 'other-native-host.exe'),
    type: 'stdio',
    allowed_extensions: ['simple-download-manager@example.com'],
  });
  await assert.rejects(
    () => validateInstalledNativeHostLayout({ installRoot }),
    /Firefox native-host manifest path does not point at the installed sidecar/,
    'wrong manifest host path should be rejected',
  );

  await createValidInstalledFixture();
  await writeManifest(layout.manifests.chrome, {
    name: 'com.myapp.download_manager',
    description: 'Simple Download Manager native messaging host',
    path: layout.sidecar,
    type: 'stdio',
    allowed_origins: ['chrome-extension://wrong-extension-id/'],
  });
  await assert.rejects(
    () => validateInstalledNativeHostLayout({ installRoot }),
    /Chrome native-host manifest allowed_origins must include chrome-extension:\/\/pkaojpfpjieklhinoibjibmjldohlmbb\//,
    'wrong Chrome extension origin should be rejected',
  );
} finally {
  await rm(installRoot, { recursive: true, force: true });
}

async function createValidInstalledFixture() {
  await rm(installRoot, { recursive: true, force: true });
  await writeText('simple-download-manager.exe', 'slint-app');
  await writeText('simple-download-manager-native-host.exe', 'native-host');
  await writeText('uninstall.exe', 'uninstaller');
  await writeText('resources/install/install.md', '# Install docs\n');
  await writeText('resources/install/register-native-host.ps1', 'register\n');
  await writeText('resources/install/unregister-native-host.ps1', 'unregister\n');
  await writeText('resources/install/release.json', JSON.stringify({
    nativeHostName: 'com.myapp.download_manager',
    chromiumExtensionId: 'pkaojpfpjieklhinoibjibmjldohlmbb',
    edgeExtensionId: 'pkaojpfpjieklhinoibjibmjldohlmbb',
    firefoxExtensionId: 'simple-download-manager@example.com',
    sidecarBinaryName: 'simple-download-manager-native-host.exe',
  }, null, 2));

  const layout = slintSmokeInstallLayout(installRoot);
  await writeManifest(layout.manifests.chrome, {
    name: 'com.myapp.download_manager',
    description: 'Simple Download Manager native messaging host',
    path: layout.sidecar,
    type: 'stdio',
    allowed_origins: ['chrome-extension://pkaojpfpjieklhinoibjibmjldohlmbb/'],
  });
  await writeManifest(layout.manifests.edge, {
    name: 'com.myapp.download_manager',
    description: 'Simple Download Manager native messaging host',
    path: layout.sidecar,
    type: 'stdio',
    allowed_origins: ['chrome-extension://pkaojpfpjieklhinoibjibmjldohlmbb/'],
  });
  await writeManifest(layout.manifests.firefox, {
    name: 'com.myapp.download_manager',
    description: 'Simple Download Manager native messaging host',
    path: layout.sidecar,
    type: 'stdio',
    allowed_extensions: ['simple-download-manager@example.com'],
  });
}

async function writeText(relativePath, content) {
  const target = path.join(installRoot, relativePath);
  await mkdir(path.dirname(target), { recursive: true });
  await writeFile(target, content, 'utf8');
}

async function writeManifest(target, manifest) {
  await mkdir(path.dirname(target), { recursive: true });
  await writeFile(target, JSON.stringify(manifest, null, 2), 'utf8');
}
