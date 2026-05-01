import assert from 'node:assert/strict';
import { mkdir, readFile, rm } from 'node:fs/promises';
import path from 'node:path';

let reportModule;
try {
  reportModule = await import('../../../scripts/slint-phase5-smoke-report.mjs');
} catch (error) {
  assert.fail(`Slint Phase 5 smoke report helper should exist: ${error instanceof Error ? error.message : error}`);
}

const {
  createSmokeReport,
  defaultSmokeReportDir,
  validateSmokeReport,
  writeSmokeReport,
} = reportModule;

const root = path.resolve('.tmp', `sdm-slint-phase5-report-${process.pid}`);

try {
  await rm(root, { recursive: true, force: true });
  await mkdir(root, { recursive: true });

  const passedReport = createSmokeReport({
    status: 'passed',
    timestamp: '2026-05-01T00:00:00.000Z',
    checkedCommands: [
      'npm run release:windows:slint',
      'node .\\scripts\\publish-updater-alpha-slint.mjs --dry-run',
      'pwsh -ExecutionPolicy Bypass -File ".\\scripts\\smoke-release-slint.ps1"',
    ],
    artifacts: {
      installerPath: 'release/slint/bundle/nsis/simple-download-manager_0.3.52-alpha_x64-setup.exe',
      signaturePath: 'release/slint/bundle/nsis/simple-download-manager_0.3.52-alpha_x64-setup.exe.sig',
      transitionFeedPath: 'release/slint/latest-alpha.json',
      slintFeedPath: 'release/slint/latest-alpha-slint.json',
    },
    installRoot: 'C:\\Temp\\SimpleDownloadManager-SlintSmoke-1234',
    registryProbes: [
      { browser: 'Chrome', key: 'HKCU:\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.myapp.download_manager', valueMatches: true },
      { browser: 'Edge', key: 'HKCU:\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\com.myapp.download_manager', valueMatches: true },
      { browser: 'Firefox', key: 'HKCU:\\Software\\Mozilla\\NativeMessagingHosts\\com.myapp.download_manager', valueMatches: true },
    ],
    uninstallCleanup: {
      registryEntriesRemoved: true,
    },
  });

  assert.equal(passedReport.status, 'passed');
  assert.doesNotThrow(() => validateSmokeReport(passedReport));

  assert.throws(
    () => validateSmokeReport(createSmokeReport({
      ...passedReport,
      artifacts: {
        ...passedReport.artifacts,
        installerPath: '',
      },
    })),
    /passed smoke reports require artifacts\.installerPath/,
    'passed reports should require installer paths',
  );

  assert.throws(
    () => validateSmokeReport(createSmokeReport({
      ...passedReport,
      registryProbes: passedReport.registryProbes.slice(0, 2),
    })),
    /passed smoke reports require Chrome, Edge, and Firefox registry probes/,
    'passed reports should require all browser registry probes',
  );

  const blockedReport = createSmokeReport({
    status: 'blocked',
    timestamp: '2026-05-01T00:00:00.000Z',
    checkedCommands: ['npm run smoke:phase5:slint'],
    prerequisiteGaps: [
      'cargo-packager (install with: cargo install cargo-packager --locked)',
      'makensis (install NSIS and ensure makensis.exe is on PATH)',
      'CARGO_PACKAGER_SIGN_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY',
      'Slint installer artifact release/slint/bundle/nsis/simple-download-manager_0.3.52-alpha_x64-setup.exe',
    ],
  });
  assert.equal(blockedReport.status, 'blocked');
  assert.deepEqual(blockedReport.prerequisiteGaps, [
    'cargo-packager (install with: cargo install cargo-packager --locked)',
    'makensis (install NSIS and ensure makensis.exe is on PATH)',
    'CARGO_PACKAGER_SIGN_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY',
    'Slint installer artifact release/slint/bundle/nsis/simple-download-manager_0.3.52-alpha_x64-setup.exe',
  ]);
  assert.doesNotThrow(() => validateSmokeReport(blockedReport));

  const failedReport = createSmokeReport({
    status: 'failed',
    timestamp: '2026-05-01T00:00:00.000Z',
    checkedCommands: ['node .\\scripts\\verify-release-slint.mjs'],
    failedCommand: 'node .\\scripts\\verify-release-slint.mjs',
    exitCode: 1,
    message: 'Missing Slint release artifact: installer',
  });
  assert.equal(failedReport.failedCommand, 'node .\\scripts\\verify-release-slint.mjs');
  assert.equal(failedReport.exitCode, 1);
  assert.equal(failedReport.message, 'Missing Slint release artifact: installer');
  assert.doesNotThrow(() => validateSmokeReport(failedReport));

  assert.equal(
    path.relative(root, defaultSmokeReportDir({ root })).replaceAll(path.sep, '/'),
    'release/slint/smoke',
    'real smoke reports should default under release/slint/smoke',
  );

  const testOutputDir = path.join(root, '.tmp-smoke-reports');
  const { reportPath } = await writeSmokeReport({
    root,
    report: blockedReport,
    outputDir: testOutputDir,
  });
  assert.equal(
    path.relative(root, reportPath).replaceAll(path.sep, '/'),
    '.tmp-smoke-reports/slint-phase5-smoke-2026-05-01T00-00-00-000Z.json',
    'tests should be able to write reports under .tmp fixtures',
  );
  const written = JSON.parse(await readFile(reportPath, 'utf8'));
  assert.equal(written.status, 'blocked');
} finally {
  await rm(root, { recursive: true, force: true });
}
