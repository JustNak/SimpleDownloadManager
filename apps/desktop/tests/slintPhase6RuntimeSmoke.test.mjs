import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const packageJson = JSON.parse(await readFile('package.json', 'utf8'));
const phase6SmokeScript = await readFile('scripts/smoke-phase6-slint.ps1', 'utf8');
const tracker = await readFile('docs/slint-migration-checklist.md', 'utf8');

assert.equal(
  packageJson.scripts['release:windows'],
  'npm run release:windows:slint',
  'Phase 6 smoke should run after Slint remains the default Windows release path',
);

assert.equal(
  packageJson.scripts['publish:updater-alpha'],
  'node ./scripts/publish-updater-alpha.mjs',
  'Phase 6B should not cut over the updater publish default',
);

assert.equal(
  packageJson.scripts['smoke:phase6:slint'],
  'pwsh -NoProfile -ExecutionPolicy Bypass -File "./scripts/smoke-phase6-slint.ps1" -CheckOnly',
  'Phase 6 Slint smoke check should be explicit and non-default',
);

assert.equal(
  packageJson.scripts['smoke:phase6:slint:full'],
  'pwsh -NoProfile -ExecutionPolicy Bypass -File "./scripts/smoke-phase6-slint.ps1" -Full',
  'Phase 6 full Slint runtime smoke should require an explicit full command',
);

assert.match(
  phase6SmokeScript,
  /param\([\s\S]*\[switch\]\$CheckOnly[\s\S]*\[switch\]\$Build[\s\S]*\[switch\]\$RuntimeSmoke[\s\S]*\[switch\]\$Full/,
  'Phase 6 smoke orchestrator should expose check-only, build, runtime-smoke, and full modes',
);
assert.match(
  phase6SmokeScript,
  /param\([\s\S]*\[switch\]\$StartupRegistrySmoke[\s\S]*\[switch\]\$InteractiveTrayCheck[\s\S]*\[switch\]\$TrayConfirmed[\s\S]*\[switch\]\$RequireCompletionEvidence/,
  'Phase 6 smoke orchestrator should expose strict completion evidence flags',
);

assert.match(
  phase6SmokeScript,
  /release:windows:slint/,
  'Phase 6 smoke should build only through the explicit Slint release command',
);

assert.doesNotMatch(
  phase6SmokeScript,
  /release:windows(?!:slint)|build-release\.ps1|publish-updater-alpha\.mjs/,
  'Phase 6 smoke should not call legacy Tauri release or publish paths',
);

assert.match(
  phase6SmokeScript,
  /verify-release-slint\.mjs/,
  'Phase 6 smoke should verify Slint release artifacts before runtime checks',
);

assert.match(
  phase6SmokeScript,
  /scripts\\e2e-native-host\.ps1/,
  'Phase 6 smoke should validate native-host handoff through the installed Slint app and sidecar',
);

assert.match(
  phase6SmokeScript,
  /MYAPP_DATA_DIR/,
  'Phase 6 smoke should run Slint with an isolated app data directory',
);

assert.match(
  phase6SmokeScript,
  /\[System\.IO\.Path\]::GetTempPath\(\)[\s\S]*SimpleDownloadManager-SlintPhase6/,
  'Phase 6 smoke should default to isolated temp roots',
);

assert.match(
  phase6SmokeScript,
  /desktop-single-instance[\s\S]*show_window/,
  'Phase 6 smoke should preserve the duplicate-instance wake request contract',
);

assert.match(
  phase6SmokeScript,
  /--autostart[\s\S]*Simple Download Manager/,
  'Phase 6 smoke should record startup registration command evidence',
);

assert.match(
  phase6SmokeScript,
  /MYAPP_ENABLE_SMOKE_COMMANDS[\s\S]*--smoke-sync-autostart=enable[\s\S]*--smoke-sync-autostart=disable/,
  'Phase 6 startup smoke should exercise the installed Slint app guarded smoke command',
);

assert.doesNotMatch(
  phase6SmokeScript,
  /Set-ItemProperty[\s\S]*CurrentVersion\\Run|New-ItemProperty[\s\S]*CurrentVersion\\Run/,
  'Phase 6 startup smoke should not bypass Slint by writing startup registry values directly',
);

assert.match(
  phase6SmokeScript,
  /isPhase6CompletionEligible|RequireCompletionEvidence/,
  'Phase 6 smoke should distinguish runtime reports from final completion-eligible reports',
);

assert.match(
  phase6SmokeScript,
  /\$completionGaps = @\(Get-Phase6CompletionGaps \$report\)[\s\S]*Phase 6 completion evidence is incomplete\./,
  'Phase 6 strict completion reporting should handle an empty PowerShell gap list without crashing',
);

assert.match(
  phase6SmokeScript,
  /manual_required/,
  'Phase 6 smoke should keep tray evidence manual-required unless reliable automation is added',
);

assert.match(
  phase6SmokeScript,
  /\$TrayConfirmed -and -not \$InteractiveTrayCheck[\s\S]*Operator pre-confirmed close-to-tray, tray Open, and tray Exit behavior/,
  'Phase 6 smoke should allow already-provided operator tray confirmation to be recorded non-interactively',
);

assert.match(
  phase6SmokeScript,
  /slint-phase6-smoke-report\.mjs/,
  'Phase 6 smoke should write structured reports through the Phase 6 report helper',
);

assert.match(
  phase6SmokeScript,
  /InvalidOperationException[\s\S]*already exited or disposed/,
  'Phase 6 process cleanup should be idempotent when PowerShell sees an already exited or disposed process object',
);

assert.match(
  tracker,
  /Phase 6B/i,
  'tracker should record the Phase 6B runtime acceptance harness after implementation',
);
