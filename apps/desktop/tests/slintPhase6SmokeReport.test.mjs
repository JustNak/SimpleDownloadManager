import assert from 'node:assert/strict';
import { mkdir, rm } from 'node:fs/promises';
import path from 'node:path';

let reportModule;
try {
  reportModule = await import('../../../scripts/slint-phase6-smoke-report.mjs');
} catch (error) {
  assert.fail(`Phase 6 Slint smoke report helper should exist: ${error instanceof Error ? error.message : error}`);
}

const {
  createPhase6SmokeReport,
  isPhase6CompletionEligible,
  phase6SmokeReportDirectory,
  validatePhase6SmokeReport,
  writePhase6SmokeReport,
} = reportModule;

const passedReport = createPhase6SmokeReport({
  status: 'passed',
  checkedCommands: [
    'npm run smoke:phase6:slint',
    'npm run release:windows:slint',
    'pwsh -ExecutionPolicy Bypass -File ".\\scripts\\smoke-release-slint.ps1"',
  ],
  artifacts: {
    installerPath: 'E:/repo/release/slint/bundle/nsis/simple-download-manager_0.3.52-alpha_x64-setup.exe',
    signaturePath: 'E:/repo/release/slint/bundle/nsis/simple-download-manager_0.3.52-alpha_x64-setup.exe.sig',
    transitionMetadataPath: 'E:/repo/release/slint/latest-alpha.json',
    nativeMetadataPath: 'E:/repo/release/slint/latest-alpha-slint.json',
  },
  installRoot: 'C:/Temp/SimpleDownloadManager-Phase6/Install',
  dataDir: 'C:/Temp/SimpleDownloadManager-Phase6/Data',
  nativeHostHandoff: {
    pingOk: true,
    enqueueOk: true,
    enqueueType: 'download_queued',
    jobId: 'job_1',
    statePath: 'C:/Temp/SimpleDownloadManager-Phase6/Data/state.json',
  },
  singleInstance: {
    duplicateExited: true,
    originalAlive: true,
    wakeRequest: 'show_window',
  },
  startupRegistration: {
    command: '"C:\\Temp\\SimpleDownloadManager-Phase6\\Install\\simple-download-manager.exe" --autostart',
    matchesInstalledExe: true,
    hasAutostartArg: true,
    registryMutated: false,
  },
  stateMigration: {
    stablePath: true,
    loadedLegacyState: true,
    rewroteCurrentState: true,
    jobCount: 1,
  },
  tray: {
    status: 'manual_required',
    note: 'Manual tray open/exit confirmation remains required for the final Phase 6 gate.',
  },
  cleanup: {
    appExited: true,
    registryEntriesRemoved: true,
    installRootRemoved: true,
  },
});

assert.equal(passedReport.status, 'passed');
assert.doesNotThrow(
  () => validatePhase6SmokeReport(passedReport),
  'passed Phase 6 reports should accept complete runtime evidence plus documented manual tray status',
);
assert.equal(
  isPhase6CompletionEligible(passedReport),
  false,
  'manual-required tray evidence should not be eligible for final Phase 6 completion',
);

const completionReport = createPhase6SmokeReport({
  ...passedReport,
  startupRegistration: {
    ...passedReport.startupRegistration,
    registryMutated: true,
    removedAfterSmoke: true,
  },
  tray: {
    status: 'passed',
    note: 'Operator confirmed close-to-tray, tray open, and tray exit.',
  },
});
assert.equal(
  isPhase6CompletionEligible(completionReport),
  true,
  'Phase 6 completion should require strict startup registry and tray evidence',
);

assert.equal(
  isPhase6CompletionEligible({
    ...completionReport,
    startupRegistration: {
      ...completionReport.startupRegistration,
      registryMutated: false,
    },
  }),
  false,
  'Phase 6 completion should reject reports that only record startup command shape',
);

assert.throws(
  () => validatePhase6SmokeReport({
    ...passedReport,
    nativeHostHandoff: { ...passedReport.nativeHostHandoff, enqueueOk: false },
  }),
  /passed Phase 6 reports require successful native-host ping and enqueue handoff/,
  'passed reports should require native-host handoff evidence',
);

assert.throws(
  () => validatePhase6SmokeReport({
    ...passedReport,
    singleInstance: { ...passedReport.singleInstance, duplicateExited: false },
  }),
  /passed Phase 6 reports require duplicate-instance wake evidence/,
  'passed reports should require duplicate-instance wake evidence',
);

assert.throws(
  () => validatePhase6SmokeReport({
    ...passedReport,
    stateMigration: { ...passedReport.stateMigration, rewroteCurrentState: false },
  }),
  /passed Phase 6 reports require state migration evidence/,
  'passed reports should require state migration evidence',
);

assert.throws(
  () => validatePhase6SmokeReport({
    ...passedReport,
    tray: { status: 'missing' },
  }),
  /passed Phase 6 reports require tray evidence or documented manual_required status/,
  'passed reports should require explicit tray evidence or a manual-required note',
);

assert.doesNotThrow(
  () => validatePhase6SmokeReport(createPhase6SmokeReport({
    status: 'blocked',
    checkedCommands: ['npm run smoke:phase6:slint'],
    prerequisiteGaps: ['Slint installer artifact is missing'],
  })),
  'blocked reports should accept a consolidated prerequisite gap list',
);

assert.throws(
  () => validatePhase6SmokeReport(createPhase6SmokeReport({
    status: 'blocked',
    checkedCommands: ['npm run smoke:phase6:slint'],
    prerequisiteGaps: [],
  })),
  /blocked Phase 6 reports require prerequisiteGaps/,
  'blocked reports should require all missing prerequisites in one list',
);

assert.doesNotThrow(
  () => validatePhase6SmokeReport(createPhase6SmokeReport({
    status: 'failed',
    checkedCommands: ['npm run smoke:phase6:slint:full'],
    failedCommand: 'npm run smoke:phase6:slint:full',
    exitCode: 1,
    message: 'Runtime smoke failed',
  })),
  'failed reports should preserve command failure details',
);

assert.throws(
  () => validatePhase6SmokeReport(createPhase6SmokeReport({
    status: 'failed',
    checkedCommands: ['npm run smoke:phase6:slint:full'],
    failedCommand: 'npm run smoke:phase6:slint:full',
    exitCode: '1',
    message: 'Runtime smoke failed',
  })),
  /failed Phase 6 reports require numeric exitCode/,
  'failed reports should require a numeric exit code',
);

const reportRoot = path.resolve('.tmp', `sdm-slint-phase6-report-${process.pid}`);
try {
  await rm(reportRoot, { recursive: true, force: true });
  await mkdir(reportRoot, { recursive: true });
  const reportPath = await writePhase6SmokeReport({
    root: reportRoot,
    report: passedReport,
  });

  assert.equal(
    phase6SmokeReportDirectory(reportRoot),
    path.join(reportRoot, 'release', 'slint', 'smoke'),
    'Phase 6 smoke reports should live beside Slint release smoke evidence',
  );
  assert.match(
    path.relative(reportRoot, reportPath).replaceAll(path.sep, '/'),
    /^release\/slint\/smoke\/slint-phase6-smoke-.+\.json$/,
    'Phase 6 report filename should be timestamped and Slint-specific',
  );
} finally {
  await rm(reportRoot, { recursive: true, force: true });
}
