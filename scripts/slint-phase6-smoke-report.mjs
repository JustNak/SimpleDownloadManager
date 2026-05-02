import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const phase6SmokeReportStatuses = ['passed', 'blocked', 'failed'];
export const phase6TrayStatuses = ['passed', 'manual_required'];

export function phase6SmokeReportDirectory(root = process.cwd()) {
  return path.join(root, 'release', 'slint', 'smoke');
}

export function createPhase6SmokeReport({
  status,
  checkedCommands = [],
  artifacts = {},
  installRoot = null,
  dataDir = null,
  nativeHostHandoff = null,
  singleInstance = null,
  startupRegistration = null,
  stateMigration = null,
  tray = null,
  cleanup = null,
  prerequisiteGaps = [],
  failedCommand = null,
  exitCode = null,
  message = null,
  timestamp = new Date().toISOString(),
} = {}) {
  return {
    status,
    timestamp,
    checkedCommands,
    artifacts,
    installRoot,
    dataDir,
    nativeHostHandoff,
    singleInstance,
    startupRegistration,
    stateMigration,
    tray,
    cleanup,
    prerequisiteGaps,
    failedCommand,
    exitCode,
    message,
  };
}

export function validatePhase6SmokeReport(report) {
  if (!phase6SmokeReportStatuses.includes(report?.status)) {
    throw new Error(`Phase 6 smoke report status must be one of: ${phase6SmokeReportStatuses.join(', ')}`);
  }
  if (!report.timestamp) {
    throw new Error('Phase 6 smoke reports require a timestamp.');
  }
  if (!Array.isArray(report.checkedCommands)) {
    throw new Error('Phase 6 smoke reports require checkedCommands to be an array.');
  }

  if (report.status === 'passed') {
    validatePassedReport(report);
  } else if (report.status === 'blocked') {
    if (!Array.isArray(report.prerequisiteGaps) || report.prerequisiteGaps.length === 0) {
      throw new Error('blocked Phase 6 reports require prerequisiteGaps.');
    }
  } else if (report.status === 'failed') {
    if (!report.failedCommand) {
      throw new Error('failed Phase 6 reports require failedCommand.');
    }
    if (typeof report.exitCode !== 'number') {
      throw new Error('failed Phase 6 reports require numeric exitCode.');
    }
    if (!report.message) {
      throw new Error('failed Phase 6 reports require message.');
    }
  }

  return report;
}

export function isPhase6CompletionEligible(report) {
  try {
    validatePhase6SmokeReport(report);
  } catch {
    return false;
  }

  return report.status === 'passed'
    && report.startupRegistration?.registryMutated === true
    && report.startupRegistration?.removedAfterSmoke === true
    && report.tray?.status === 'passed';
}

export async function writePhase6SmokeReport({ root = process.cwd(), report } = {}) {
  validatePhase6SmokeReport(report);
  const outputDir = phase6SmokeReportDirectory(root);
  await mkdir(outputDir, { recursive: true });
  const reportPath = path.join(outputDir, `slint-phase6-smoke-${safeTimestamp(report.timestamp)}.json`);
  await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  return reportPath;
}

function validatePassedReport(report) {
  for (const artifactKey of [
    'installerPath',
    'signaturePath',
    'transitionMetadataPath',
    'nativeMetadataPath',
  ]) {
    if (!report.artifacts?.[artifactKey]) {
      throw new Error(`passed Phase 6 reports require artifacts.${artifactKey}.`);
    }
  }
  if (!report.installRoot) {
    throw new Error('passed Phase 6 reports require installRoot.');
  }
  if (!report.dataDir) {
    throw new Error('passed Phase 6 reports require dataDir.');
  }
  if (!report.nativeHostHandoff?.pingOk || !report.nativeHostHandoff?.enqueueOk) {
    throw new Error('passed Phase 6 reports require successful native-host ping and enqueue handoff.');
  }
  if (!report.singleInstance?.duplicateExited || !report.singleInstance?.originalAlive) {
    throw new Error('passed Phase 6 reports require duplicate-instance wake evidence.');
  }
  if (
    !report.startupRegistration?.command ||
    !report.startupRegistration?.matchesInstalledExe ||
    !report.startupRegistration?.hasAutostartArg
  ) {
    throw new Error('passed Phase 6 reports require startup registration command evidence.');
  }
  if (
    !report.stateMigration?.stablePath ||
    !report.stateMigration?.loadedLegacyState ||
    !report.stateMigration?.rewroteCurrentState
  ) {
    throw new Error('passed Phase 6 reports require state migration evidence.');
  }
  if (!phase6TrayStatuses.includes(report.tray?.status)) {
    throw new Error('passed Phase 6 reports require tray evidence or documented manual_required status.');
  }
  if (report.tray.status === 'manual_required' && !report.tray.note) {
    throw new Error('passed Phase 6 reports require a note when tray status is manual_required.');
  }
  if (!report.cleanup?.appExited || !report.cleanup?.registryEntriesRemoved) {
    throw new Error('passed Phase 6 reports require cleanup evidence.');
  }
}

function safeTimestamp(timestamp) {
  return timestamp.replace(/[^0-9A-Za-z.-]/g, '-');
}

function parseCliArgs(argv) {
  const args = {
    root: process.cwd(),
    reportJson: null,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--root') {
      args.root = argv[index + 1];
      index += 1;
      continue;
    }
    if (arg === '--report-json') {
      args.reportJson = argv[index + 1];
      index += 1;
      continue;
    }
    if (arg === '--help' || arg === '-h') {
      args.help = true;
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }
  return args;
}

function printUsage() {
  console.log('Usage: node scripts/slint-phase6-smoke-report.mjs --report-json <json> [--root <path>]');
}

const currentFile = fileURLToPath(import.meta.url);
if (process.argv[1] && path.resolve(process.argv[1]) === currentFile) {
  try {
    const args = parseCliArgs(process.argv.slice(2));
    if (args.help) {
      printUsage();
      process.exit(0);
    }
    if (!args.reportJson) {
      throw new Error('--report-json is required.');
    }
    const report = JSON.parse(args.reportJson);
    const reportPath = await writePhase6SmokeReport({
      root: args.root,
      report,
    });
    console.log(reportPath);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
