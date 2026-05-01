import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const packageJson = JSON.parse(await readFile('package.json', 'utf8'));
const tauriReleaseScript = await readFile('scripts/build-release.ps1', 'utf8');
const slintReleaseScript = await readFile('scripts/build-release-slint.ps1', 'utf8');
const slintSmokeScript = await readFile('scripts/smoke-release-slint.ps1', 'utf8');
const slintPhase5SmokeScript = await readFile('scripts/smoke-phase5-slint.ps1', 'utf8');
const slintPublishScript = await readFile('scripts/publish-updater-alpha-slint.mjs', 'utf8');
const slintPhase5ReportHelper = await readFile('scripts/slint-phase5-smoke-report.mjs', 'utf8');
const packagerConfig = await readFile('apps/desktop-slint/Packager.toml', 'utf8');
const slintNsisTemplate = await readFile('apps/desktop-slint/nsis/installer.nsi', 'utf8');

assert.equal(
  packageJson.scripts['release:windows'],
  'pwsh -ExecutionPolicy Bypass -File "./scripts/build-release.ps1"',
  'default Windows release path should remain the legacy Tauri script until Slint smoke tests pass',
);

assert.equal(
  packageJson.scripts['release:windows:tauri'],
  packageJson.scripts['release:windows'],
  'explicit Tauri release script should preserve the existing default release command',
);

assert.equal(
  packageJson.scripts['release:windows:slint'],
  'pwsh -ExecutionPolicy Bypass -File "./scripts/build-release-slint.ps1"',
  'Slint should have an explicit parallel release command',
);

assert.equal(
  packageJson.scripts['smoke:phase5:slint'],
  'pwsh -NoProfile -ExecutionPolicy Bypass -File "./scripts/smoke-phase5-slint.ps1" -CheckOnly',
  'Phase 5 Slint smoke check should be explicit and non-default',
);

assert.equal(
  packageJson.scripts['smoke:phase5:slint:full'],
  'pwsh -NoProfile -ExecutionPolicy Bypass -File "./scripts/smoke-phase5-slint.ps1" -Full',
  'Phase 5 full Slint smoke should require an explicit full command',
);

assert.equal(
  packageJson.scripts['publish:updater-alpha:slint'],
  'node ./scripts/publish-updater-alpha-slint.mjs',
  'Slint updater publishing should stay behind an explicit command',
);

assert.match(
  tauriReleaseScript,
  /tauri:build/,
  'legacy Tauri release script should still build through Tauri',
);

assert.doesNotMatch(
  tauriReleaseScript,
  /cargo\s+packager|cargo-packager/i,
  'legacy Tauri release script should not be converted to cargo-packager',
);

assert.match(
  slintReleaseScript,
  /\$desktopSlintRoot\s*=\s*Join-Path \$workspaceRoot 'apps\\desktop-slint'/,
  'Slint release script should package the Slint desktop crate',
);

assert.match(
  slintReleaseScript,
  /cargo install cargo-packager --locked/,
  'Slint release script should fail clearly with the cargo-packager install command',
);

assert.match(
  slintReleaseScript,
  /makensis/,
  'Slint release script should fail clearly when NSIS makensis is missing',
);

assert.match(
  slintReleaseScript,
  /Test-ReleasePrerequisites/,
  'Slint release script should aggregate prerequisite checks before any builds',
);

assert.match(
  slintReleaseScript,
  /CARGO_PACKAGER_SIGN_PRIVATE_KEY/,
  'Slint release script should accept cargo-packager signing env',
);

assert.match(
  slintReleaseScript,
  /TAURI_SIGNING_PRIVATE_KEY/,
  'Slint release script should fall back to the existing Tauri signing key env',
);

assert.match(
  slintReleaseScript,
  /CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD[\s\S]*TAURI_SIGNING_PRIVATE_KEY_PASSWORD/,
  'Slint release script should map the existing signing password env for cargo-packager',
);

assert.match(
  slintReleaseScript,
  /prepare-release-slint\.mjs/,
  'Slint release script should stage Slint packaging resources before cargo-packager runs',
);

assert.doesNotMatch(
  slintReleaseScript,
  /\.\\scripts\\prepare-release\.mjs/,
  'Slint release script should not use the Tauri release staging helper',
);

assert.match(
  slintReleaseScript,
  /Invoke-ReleaseCommand 'cargo' @\('packager', '--release'\)/,
  'Slint release script should run cargo packager --release',
);

assert.match(
  slintReleaseScript,
  /build:extension/,
  'Slint release script should keep extension bundle generation in the release path',
);

assert.match(
  slintReleaseScript,
  /apps\\native-host/,
  'Slint release script should build and include the native-host sidecar',
);

assert.match(
  slintReleaseScript,
  /updater-release\.mjs'?,\s*'--slint'/,
  'Slint release script should write Slint updater metadata feeds',
);

assert.match(
  slintReleaseScript,
  /verify-release-slint\.mjs/,
  'Slint release script should verify generated Slint artifacts before reporting success',
);

assert.match(
  slintSmokeScript,
  /release:windows:slint/,
  'Slint smoke script should build through the explicit Slint release command when requested',
);

assert.doesNotMatch(
  slintSmokeScript,
  /release:windows(?!"?:slint)|build-release\.ps1/,
  'Slint smoke script should not call the default Tauri release command or legacy release script',
);

assert.match(
  slintSmokeScript,
  /param\([\s\S]*\[switch\]\$Build[\s\S]*\[switch\]\$CheckOnly/,
  'Slint smoke script should expose build and check-only modes',
);

assert.match(
  slintSmokeScript,
  /cargo-packager[\s\S]*makensis[\s\S]*CARGO_PACKAGER_SIGN_PRIVATE_KEY/,
  'Slint smoke script should report missing cargo-packager, makensis, and signing env clearly',
);

assert.match(
  slintSmokeScript,
  /verify-release-slint\.mjs/,
  'Slint smoke script should verify installer and signature artifacts before install',
);

assert.match(
  slintSmokeScript,
  /\[System\.IO\.Path\]::GetTempPath\(\)[\s\S]*SimpleDownloadManager-SlintSmoke/,
  'Slint smoke script should default to an isolated temp install directory',
);

assert.doesNotMatch(
  slintSmokeScript,
  /Program Files|AppData\\Local\\Programs\\Simple Download Manager/,
  'Slint smoke script should not target the real production install directory by default',
);

assert.match(
  slintSmokeScript,
  /\/S[\s\S]*\/D=/,
  'Slint smoke script should install the NSIS package silently into the smoke install root',
);

assert.match(
  slintSmokeScript,
  /HKCU:\\Software\\Google\\Chrome\\NativeMessagingHosts\\com\.myapp\.download_manager[\s\S]*HKCU:\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\com\.myapp\.download_manager[\s\S]*HKCU:\\Software\\Mozilla\\NativeMessagingHosts\\com\.myapp\.download_manager/,
  'Slint smoke script should probe all browser native-host registry entries',
);

assert.match(
  slintSmokeScript,
  /uninstall\.exe/,
  'Slint smoke script should run the generated uninstaller during the smoke flow',
);

assert.match(
  slintPhase5SmokeScript,
  /param\([\s\S]*\[switch\]\$CheckOnly[\s\S]*\[switch\]\$Build[\s\S]*\[switch\]\$InstallSmoke[\s\S]*\[switch\]\$PublishDryRun[\s\S]*\[switch\]\$Full/,
  'Phase 5 smoke orchestrator should expose check-only, build, install-smoke, publish-dry-run, and full modes',
);

assert.match(
  slintPhase5SmokeScript,
  /release:windows:slint/,
  'Phase 5 smoke orchestrator should build only through the explicit Slint release command',
);

assert.match(
  slintPhase5SmokeScript,
  /publish-updater-alpha-slint\.mjs[\s\S]*--dry-run/,
  'Phase 5 smoke orchestrator should validate Slint publish inputs through the Slint dry-run helper',
);

assert.match(
  slintPhase5SmokeScript,
  /smoke-release-slint\.ps1/,
  'Phase 5 smoke orchestrator should delegate installer registry mutation to the Slint installer smoke script',
);

assert.doesNotMatch(
  slintPhase5SmokeScript,
  /release:windows(?!"?:slint)|build-release\.ps1|publish-updater-alpha\.mjs/,
  'Phase 5 smoke orchestrator should not call legacy Tauri release or publish paths',
);

assert.match(
  slintPhase5SmokeScript,
  /\[System\.IO\.Path\]::GetTempPath\(\)[\s\S]*SimpleDownloadManager-SlintPhase5/,
  'Phase 5 smoke orchestrator should default to an isolated temp install directory',
);

assert.match(
  slintPhase5SmokeScript,
  /slint-phase5-smoke-report\.mjs/,
  'Phase 5 smoke orchestrator should write structured reports through the report helper',
);

assert.match(
  slintPhase5ReportHelper,
  /status[\s\S]*passed[\s\S]*blocked[\s\S]*failed/,
  'Phase 5 smoke report helper should normalize passed, blocked, and failed reports',
);

assert.match(
  slintPublishScript,
  /verify-release-slint\.mjs/,
  'Slint publish helper should validate Slint release artifacts before upload',
);

assert.match(
  slintPublishScript,
  /latest-alpha\.json[\s\S]*latest-alpha-slint\.json/,
  'Slint publish helper should upload both transition and Slint-native updater feeds',
);

assert.match(
  slintPublishScript,
  /--dry-run/,
  'Slint publish helper should support dry-run validation without GitHub upload',
);

assert.doesNotMatch(
  slintPublishScript,
  /publish-updater-alpha\.mjs|release[\\/]+bundle[\\/]+nsis[\\/]+Simple Download Manager/,
  'Slint publish helper should not call or package the legacy Tauri updater path',
);

assert.match(
  packagerConfig,
  /formats\s*=\s*\["nsis"\]/,
  'Slint packager config should produce an NSIS installer',
);

assert.match(
  packagerConfig,
  /productName\s*=\s*"Simple Download Manager"/,
  'Slint packager config should keep the product name stable',
);

assert.match(
  packagerConfig,
  /identifier\s*=\s*"com\.myapp\.downloadmanager"/,
  'Slint packager config should keep the stable app identifier',
);

assert.match(
  packagerConfig,
  /simple-download-manager(?:\.exe)?/,
  'Slint packager config should package the Slint binary',
);

assert.match(
  packagerConfig,
  /beforePackagingCommand\s*=\s*"cargo build --release --manifest-path Cargo\.toml"/,
  'Slint packager config should use a manifest path relative to apps/desktop-slint',
);

assert.doesNotMatch(
  packagerConfig,
  /--manifest-path apps\/desktop-slint\/Cargo\.toml/,
  'Slint packager config should not use a root-relative manifest path when cargo-packager runs inside apps/desktop-slint',
);

assert.match(
  packagerConfig,
  /release\/slint|release\\slint/,
  'Slint packager output should stay under release/slint',
);

assert.match(
  packagerConfig,
  /release\/slint\/staging|release\\slint\\staging/,
  'Slint packager resources should come from isolated Slint staging',
);

assert.doesNotMatch(
  packagerConfig,
  /src-tauri\/resources\/install|src-tauri\\resources\\install/,
  'Slint packager config should not consume Tauri prepared install resources',
);

assert.match(
  packagerConfig,
  /\[nsis\][\s\S]*template\s*=\s*"nsis\/installer\.nsi"/,
  'Slint packager config should use the Slint NSIS template',
);

assert.match(
  packagerConfig,
  /simple-download-manager-native-host\.exe/,
  'Slint packager config should include the native-host sidecar',
);

assert.match(
  packagerConfig,
  /target\s*=\s*"resources\/install"/,
  'Slint packager config should include the staged install resources directory',
);

assert.match(
  packagerConfig,
  /latest-alpha-slint\.json/,
  'Slint packager config should track the Slint updater metadata path',
);

assert.match(
  slintNsisTemplate,
  /simple-download-manager-native-host\.exe/,
  'Slint NSIS template should look up the native-host sidecar',
);

assert.match(
  slintNsisTemplate,
  /register-native-host\.ps1[\s\S]*-HostBinaryPath "\$0"[\s\S]*-InstallRoot "\$INSTDIR"/,
  'Slint NSIS template should register the native host after install',
);

assert.match(
  slintNsisTemplate,
  /unregister-native-host\.ps1/,
  'Slint NSIS template should unregister the native host before uninstall',
);

assert.match(
  slintNsisTemplate,
  /\$INSTDIR/,
  'Slint NSIS template should use the install directory for hook paths',
);

assert.doesNotMatch(
  slintNsisTemplate,
  /\$UpdateMode|\$PassiveMode|--installer-configure|MessageBox MB_YESNO/,
  'Slint NSIS template should not port Tauri-only startup option prompts in Phase 5B',
);
