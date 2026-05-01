import assert from 'node:assert/strict';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

let updaterRelease;
try {
  updaterRelease = await import('../../../scripts/updater-release.mjs');
} catch (error) {
  assert.fail(`Updater release helper should exist: ${error instanceof Error ? error.message : error}`);
}

const {
  createLatestAlphaJson,
  createSlintLatestAlphaJson,
  githubReleaseAssetName,
  requireSigningEnvironment,
  slintWindowsInstallerName,
  slintUpdaterMetadataFilename,
  updaterAssetUrl,
  updaterMetadataFilename,
  updaterReleasePaths,
  updaterReleaseTag,
  windowsInstallerName,
  writeSlintReleaseFeeds,
  writeSlintLatestAlphaJson,
  writeSlintTransitionLatestAlphaJson,
} = updaterRelease;

assert.equal(updaterReleaseTag, 'updater-alpha');
assert.equal(updaterMetadataFilename, 'latest-alpha.json');
assert.equal(slintUpdaterMetadataFilename, 'latest-alpha-slint.json');
assert.equal(
  windowsInstallerName('0.3.48-alpha'),
  'Simple Download Manager_0.3.48-alpha_x64-setup.exe',
  'Tauri updater installer name should remain product-name based',
);
assert.equal(
  slintWindowsInstallerName('0.3.48-alpha'),
  'simple-download-manager_0.3.48-alpha_x64-setup.exe',
  'Slint updater installer name should match cargo-packager NSIS artifact naming',
);

const installerName = 'Simple Download Manager_0.3.48-alpha_x64-setup.exe';
assert.equal(
  githubReleaseAssetName(installerName),
  'Simple.Download.Manager_0.3.48-alpha_x64-setup.exe',
);
assert.equal(
  updaterAssetUrl(
    'JustNak/SimpleDownloadManager',
    updaterReleaseTag,
    githubReleaseAssetName(installerName),
  ),
  'https://github.com/JustNak/SimpleDownloadManager/releases/download/updater-alpha/Simple.Download.Manager_0.3.48-alpha_x64-setup.exe',
);

const latest = createLatestAlphaJson({
  version: '0.3.48-alpha',
  notes: 'Alpha update',
  pubDate: '2026-04-27T00:00:00.000Z',
  url: updaterAssetUrl(
    'JustNak/SimpleDownloadManager',
    updaterReleaseTag,
    githubReleaseAssetName(installerName),
  ),
  signature: 'signed-content',
});

assert.equal(latest.version, '0.3.48-alpha');
assert.equal(latest.notes, 'Alpha update');
assert.equal(latest.pub_date, '2026-04-27T00:00:00.000Z');
assert.deepEqual(Object.keys(latest.platforms), ['windows-x86_64']);
assert.equal(latest.platforms['windows-x86_64'].signature, 'signed-content');
assert.match(latest.platforms['windows-x86_64'].url, /Simple\.Download\.Manager_0\.3\.48-alpha_x64-setup\.exe$/);
assert.equal(
  latest.platforms['windows-x86_64'].format,
  undefined,
  'Tauri updater metadata should keep the original feed shape',
);

const slintLatest = createSlintLatestAlphaJson({
  version: '0.3.48-alpha',
  notes: 'Alpha update',
  pubDate: '2026-04-27T00:00:00.000Z',
  url: latest.platforms['windows-x86_64'].url,
  signature: 'signed-content',
});

assert.equal(
  slintLatest.platforms['windows-x86_64'].format,
  'nsis',
  'Slint cargo-packager updater metadata should identify the NSIS installer format',
);

const slintPaths = updaterReleasePaths('repo-root', '0.3.48-alpha', {
  metadataFilename: slintUpdaterMetadataFilename,
  releaseSubdir: 'slint',
  installerName: slintWindowsInstallerName('0.3.48-alpha'),
});
assert.equal(
  slintPaths.installerPath,
  path.join('repo-root', 'release', 'slint', 'bundle', 'nsis', 'simple-download-manager_0.3.48-alpha_x64-setup.exe'),
  'Slint updater paths should resolve under release/slint using cargo-packager artifact names',
);

const slintMetadataRoot = path.resolve('.tmp', `sdm-slint-updater-${process.pid}`);
try {
  await rm(slintMetadataRoot, { recursive: true, force: true });
  await mkdir(
    path.join(slintMetadataRoot, 'release', 'slint', 'bundle', 'nsis'),
    { recursive: true },
  );
  await writeFile(
    path.join(slintMetadataRoot, 'package.json'),
    JSON.stringify({ version: '0.3.48-alpha' }),
    'utf8',
  );
  await writeFile(
    path.join(
      slintMetadataRoot,
      'release',
      'slint',
      'bundle',
      'nsis',
      'simple-download-manager_0.3.48-alpha_x64-setup.exe.sig',
    ),
    'slint-signature',
    'utf8',
  );

  const { metadata, paths } = await writeSlintLatestAlphaJson({
    root: slintMetadataRoot,
    notes: 'Slint update',
    pubDate: '2026-05-01T00:00:00.000Z',
  });
  assert.equal(
    path.relative(slintMetadataRoot, paths.metadataPath).replaceAll(path.sep, '/'),
    'release/slint/latest-alpha-slint.json',
    'Slint metadata should be written beside Slint release artifacts',
  );
  assert.match(
    metadata.platforms['windows-x86_64'].url,
    /simple-download-manager_0\.3\.48-alpha_x64-setup\.exe$/,
    'Slint metadata URL should reference the cargo-packager installer name',
  );
  assert.equal(metadata.platforms['windows-x86_64'].signature, 'slint-signature');
  assert.equal(
    metadata.platforms['windows-x86_64'].format,
    'nsis',
    'Slint-native metadata should retain the cargo-packager updater format',
  );

  const { metadata: transitionMetadata, paths: transitionPaths } = await writeSlintTransitionLatestAlphaJson({
    root: slintMetadataRoot,
    notes: 'Tauri to Slint transition update',
    pubDate: '2026-05-01T00:00:00.000Z',
  });
  assert.equal(
    path.relative(slintMetadataRoot, transitionPaths.metadataPath).replaceAll(path.sep, '/'),
    'release/slint/latest-alpha.json',
    'Slint transition metadata should use the Tauri feed filename under release/slint',
  );
  assert.match(
    transitionMetadata.platforms['windows-x86_64'].url,
    /simple-download-manager_0\.3\.48-alpha_x64-setup\.exe$/,
    'Slint transition metadata should reference the Slint cargo-packager installer',
  );
  assert.equal(transitionMetadata.platforms['windows-x86_64'].signature, 'slint-signature');
  assert.equal(
    transitionMetadata.platforms['windows-x86_64'].format,
    undefined,
    'Slint transition metadata should keep the Tauri-compatible feed shape without format',
  );

  const feeds = await writeSlintReleaseFeeds({
    root: slintMetadataRoot,
    notes: 'Shared Slint update',
    pubDate: '2026-05-01T00:00:00.000Z',
  });
  assert.equal(
    path.relative(slintMetadataRoot, feeds.transition.paths.metadataPath).replaceAll(path.sep, '/'),
    'release/slint/latest-alpha.json',
    'Slint feed writer should write the transition feed',
  );
  assert.equal(
    path.relative(slintMetadataRoot, feeds.native.paths.metadataPath).replaceAll(path.sep, '/'),
    'release/slint/latest-alpha-slint.json',
    'Slint feed writer should write the Slint-native feed',
  );
  assert.equal(
    feeds.transition.metadata.platforms['windows-x86_64'].url,
    feeds.native.metadata.platforms['windows-x86_64'].url,
    'transition and Slint-native feeds should point at the same Slint installer',
  );
  assert.equal(
    feeds.transition.metadata.platforms['windows-x86_64'].signature,
    feeds.native.metadata.platforms['windows-x86_64'].signature,
    'transition and Slint-native feeds should use the same signature',
  );
  assert.equal(feeds.transition.metadata.platforms['windows-x86_64'].format, undefined);
  assert.equal(feeds.native.metadata.platforms['windows-x86_64'].format, 'nsis');

  await rm(path.join(
    slintMetadataRoot,
    'release',
    'slint',
    'bundle',
    'nsis',
    'simple-download-manager_0.3.48-alpha_x64-setup.exe.sig',
  ));
  await assert.rejects(
    () => writeSlintLatestAlphaJson({ root: slintMetadataRoot }),
    /release[\\/]+slint[\\/]+bundle[\\/]+nsis[\\/]+simple-download-manager_0\.3\.48-alpha_x64-setup\.exe\.sig/,
    'missing Slint signature errors should include the expected signature path',
  );
} finally {
  await rm(slintMetadataRoot, { recursive: true, force: true });
}

assert.throws(
  () => requireSigningEnvironment({}),
  /TAURI_SIGNING_PRIVATE_KEY is required/,
  'release builds should fail clearly when updater signing is not configured',
);

const rootPackage = JSON.parse(await readFile('package.json', 'utf8'));
assert.equal(
  rootPackage.scripts['publish:updater-alpha'],
  'node ./scripts/publish-updater-alpha.mjs',
  'release tooling should keep the legacy Tauri GitHub updater publish command',
);
assert.equal(
  rootPackage.scripts['publish:updater-alpha:slint'],
  'node ./scripts/publish-updater-alpha-slint.mjs',
  'release tooling should expose an explicit Slint updater publish command',
);

const updaterReleaseSource = await readFile('scripts/updater-release.mjs', 'utf8');
assert.match(
  updaterReleaseSource,
  /--slint/,
  'updater metadata helper should expose a Slint metadata CLI mode',
);
assert.match(
  updaterReleaseSource,
  /writeSlintReleaseFeeds/,
  'Slint metadata CLI mode should write both transition and Slint-native feeds',
);

await assert.doesNotReject(
  () => importReleaseHelpersWithoutArgvEntrypoint(),
  'release helpers should be importable when process.argv[1] is absent in node eval contexts used by smoke scripts',
);

async function importReleaseHelpersWithoutArgvEntrypoint() {
  const originalArgv = [...process.argv];
  process.argv.length = 1;
  try {
    const cacheBust = `?argv-missing-test=${Date.now()}`;
    await import(`${pathToFileURL(path.resolve('scripts/updater-release.mjs')).href}${cacheBust}`);
    await import(`${pathToFileURL(path.resolve('scripts/verify-release-slint.mjs')).href}${cacheBust}`);
  } finally {
    process.argv.length = 0;
    process.argv.push(...originalArgv);
  }
}
