import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const packageJson = JSON.parse(await readFile('package.json', 'utf8'));
const tauriReleaseScript = await readFile('scripts/build-release.ps1', 'utf8');
const slintReleaseScript = await readFile('scripts/build-release-slint.ps1', 'utf8');
const slintSmokeScript = await readFile('scripts/smoke-release-slint.ps1', 'utf8');
const slintPhase5SmokeScript = await readFile('scripts/smoke-phase5-slint.ps1', 'utf8');
const slintPublishScript = await readFile('scripts/publish-updater-alpha-slint.mjs', 'utf8');
const slintPhase5ReportHelper = await readFile('scripts/slint-phase5-smoke-report.mjs', 'utf8');
const packagerConfig = await readFile('apps/desktop-slint/packager.toml', 'utf8');
const slintNsisTemplate = await readFile('apps/desktop-slint/nsis/installer.nsi', 'utf8');

assert.equal(
  packageJson.scripts['build:desktop'],
  'npm run build:desktop:slint',
  'default desktop build should target the primary Slint desktop app',
);

assert.equal(
  packageJson.scripts['build:desktop:tauri'],
  'npm run build --workspace @myapp/desktop',
  'legacy Tauri desktop build should remain available explicitly',
);

assert.equal(
  packageJson.scripts['build:desktop:slint'],
  'cargo build --manifest-path apps/desktop-slint/Cargo.toml',
  'Slint desktop build should remain available explicitly',
);

assert.equal(
  packageJson.scripts['release:windows'],
  'npm run release:windows:slint',
  'default Windows release path should target Slint after the passed Phase 5 smoke gate',
);

assert.equal(
  packageJson.scripts['release:windows:tauri'],
  'pwsh -ExecutionPolicy Bypass -File "./scripts/build-release.ps1"',
  'explicit Tauri release script should preserve the legacy Tauri release command',
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
  packageJson.scripts['publish:updater-alpha'],
  'node ./scripts/publish-updater-alpha.mjs',
  'default updater publishing should remain on the legacy Tauri path until a separate publish cutover',
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

assert.match(
  tauriReleaseScript,
  /build:desktop:tauri/,
  'legacy Tauri release script should use the explicit Tauri desktop build alias after Slint becomes the default desktop build',
);

assert.doesNotMatch(
  tauriReleaseScript,
  /@\('run', 'build:desktop'\)/,
  'legacy Tauri release script should not call the Slint-default desktop build alias',
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
  /Resolve-MakensisPath[\s\S]*NSIS\\Bin\\makensis\.exe[\s\S]*NSIS\\makensis\.exe/,
  'Slint release script should find NSIS from standard install paths when makensis is not yet on PATH',
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
  /\.simple-download-manager\\tauri-updater\.key/,
  'Slint release script should also use the legacy Tauri default signing key file',
);

assert.match(
  slintReleaseScript,
  /SDM_TAURI_SIGNING_PRIVATE_KEY_PATH/,
  'Slint release script should honor the existing custom Tauri signing key path env',
);

assert.match(
  slintReleaseScript,
  /CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD[\s\S]*TAURI_SIGNING_PRIVATE_KEY_PASSWORD/,
  'Slint release script should accept both cargo-packager and Tauri signing password env names',
);

assert.match(
  slintReleaseScript,
  /\$null -eq \$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD[\s\S]*\$null -ne \$env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD[\s\S]*\$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = \$env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD/,
  'Slint release script should map cargo-packager password env into Tauri signer env when needed',
);

assert.match(
  slintReleaseScript,
  /Test-TauriSigningConfiguration[\s\S]*node \$tauriCli signer sign \$probePath/,
  'Slint release script should validate signing material with the Tauri signer that owns the existing updater key format',
);

assert.match(
  slintReleaseScript,
  /Remove-Item Env:CARGO_PACKAGER_SIGN_PRIVATE_KEY[\s\S]*Remove-Item Env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD[\s\S]*Invoke-ReleaseCommand 'cargo' @\('packager', '--release'\)[\s\S]*node_modules\/@tauri-apps\/cli\/tauri\.js', 'signer', 'sign'/,
  'Slint release script should package unsigned with cargo-packager and create updater signatures through the Tauri signer',
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
  /Resolve-ExecutablePath[\s\S]*Get-Command[\s\S]*ChangeExtension\(\$resolved\.Source, '\.cmd'\)[\s\S]*\$startInfo\.FileName = Resolve-ExecutablePath \$Command/,
  'Slint smoke script should resolve npm/node/pwsh through Get-Command and prefer .cmd shims before ProcessStartInfo launches them',
);

assert.match(
  slintSmokeScript,
  /cargo-packager[\s\S]*makensis[\s\S]*CARGO_PACKAGER_SIGN_PRIVATE_KEY/,
  'Slint smoke script should report missing cargo-packager, makensis, and signing env clearly',
);

assert.match(
  slintSmokeScript,
  /\.simple-download-manager\\tauri-updater\.key/,
  'Slint smoke script should use the same default Tauri signing key file as the release script',
);

assert.match(
  slintSmokeScript,
  /Resolve-MakensisPath[\s\S]*NSIS\\Bin\\makensis\.exe[\s\S]*NSIS\\makensis\.exe/,
  'Slint installer smoke script should find NSIS from standard install paths when makensis is not yet on PATH',
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
  slintSmokeScript,
  /Wait-RegistryValueMissing[\s\S]*Start-Sleep -Milliseconds 250[\s\S]*Wait-RegistryValueMissing 'HKCU:\\Software\\Google\\Chrome\\NativeMessagingHosts\\com\.myapp\.download_manager'/,
  'Slint smoke script should wait for silent NSIS uninstall registry cleanup before failing',
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

assert.match(
  slintPhase5SmokeScript,
  /Resolve-ExecutablePath[\s\S]*Get-Command[\s\S]*ChangeExtension\(\$resolved\.Source, '\.cmd'\)[\s\S]*\$startInfo\.FileName = Resolve-ExecutablePath \$Command/,
  'Phase 5 smoke orchestrator should resolve npm/node/pwsh through Get-Command and prefer .cmd shims before ProcessStartInfo launches them',
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
  slintPhase5SmokeScript,
  /Resolve-MakensisPath[\s\S]*NSIS\\Bin\\makensis\.exe[\s\S]*NSIS\\makensis\.exe/,
  'Phase 5 smoke orchestrator should find NSIS from standard install paths when makensis is not yet on PATH',
);

assert.match(
  slintPhase5SmokeScript,
  /\.simple-download-manager\\tauri-updater\.key/,
  'Phase 5 smoke orchestrator should use the same default Tauri signing key file as the legacy release path',
);

assert.match(
  slintPhase5SmokeScript,
  /Test-TauriSigningConfiguration[\s\S]*node \$tauriCli signer sign \$probePath/,
  'Phase 5 smoke orchestrator should probe signing with the Tauri signer instead of cargo-packager signer',
);

assert.match(
  slintPhase5SmokeScript,
  /\$requireArtifactsBeforeRun\s*=\s*-not \$Build/,
  'Phase 5 full smoke should not require installer artifacts before the build step runs',
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
  /name\s*=\s*"simple-download-manager"/,
  'Slint packager config should set name explicitly so cargo-packager 0.11.x does not drop the config during discovery',
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

assert.match(
  packagerConfig,
  /binariesDir\s*=\s*"target\/release"/,
  'Slint packager config should resolve the Slint binary from the crate release target directory',
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
  /(?<!\\)\\\{\{/,
  'Slint NSIS template should not escape Handlebars path expressions with a single backslash',
);

assert.doesNotMatch(
  slintNsisTemplate,
  /\$UpdateMode|\$PassiveMode|--installer-configure|MessageBox MB_YESNO/,
  'Slint NSIS template should not port Tauri-only startup option prompts in Phase 5B',
);
