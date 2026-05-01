import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const smokeReportStatuses = ['passed', 'blocked', 'failed'];

export function defaultSmokeReportDir({ root = defaultRepoRoot() } = {}) {
  return path.join(root, 'release', 'slint', 'smoke');
}

export function createSmokeReport({
  status,
  timestamp = new Date().toISOString(),
  checkedCommands = [],
  artifacts = {},
  prerequisiteGaps = [],
  installRoot = null,
  registryProbes = [],
  uninstallCleanup = null,
  failedCommand = null,
  exitCode = null,
  message = null,
} = {}) {
  return {
    status,
    timestamp,
    checkedCommands,
    artifacts,
    prerequisiteGaps,
    installRoot,
    registryProbes,
    uninstallCleanup,
    failedCommand,
    exitCode,
    message,
  };
}

export function validateSmokeReport(report) {
  if (!smokeReportStatuses.includes(report?.status)) {
    throw new Error(`Phase 5 smoke report status must be one of: ${smokeReportStatuses.join(', ')}`);
  }
  if (!report.timestamp) {
    throw new Error('Phase 5 smoke reports require a timestamp.');
  }
  if (!Array.isArray(report.checkedCommands)) {
    throw new Error('Phase 5 smoke reports require checkedCommands to be an array.');
  }

  if (report.status === 'passed') {
    for (const artifactKey of [
      'installerPath',
      'signaturePath',
      'transitionFeedPath',
      'slintFeedPath',
    ]) {
      if (!report.artifacts?.[artifactKey]) {
        throw new Error(`passed smoke reports require artifacts.${artifactKey}.`);
      }
    }
    if (!report.installRoot) {
      throw new Error('passed smoke reports require installRoot.');
    }
    const browsers = new Set((report.registryProbes ?? []).map((probe) => probe.browser));
    for (const browser of ['Chrome', 'Edge', 'Firefox']) {
      if (!browsers.has(browser)) {
        throw new Error('passed smoke reports require Chrome, Edge, and Firefox registry probes.');
      }
    }
    if (typeof report.uninstallCleanup?.registryEntriesRemoved !== 'boolean') {
      throw new Error('passed smoke reports require uninstallCleanup.registryEntriesRemoved.');
    }
  }

  if (report.status === 'blocked') {
    if (!Array.isArray(report.prerequisiteGaps) || report.prerequisiteGaps.length === 0) {
      throw new Error('blocked smoke reports require prerequisiteGaps.');
    }
  }

  if (report.status === 'failed') {
    if (!report.failedCommand) {
      throw new Error('failed smoke reports require failedCommand.');
    }
    if (typeof report.exitCode !== 'number') {
      throw new Error('failed smoke reports require numeric exitCode.');
    }
    if (!report.message) {
      throw new Error('failed smoke reports require message.');
    }
  }

  return report;
}

export async function writeSmokeReport({
  root = defaultRepoRoot(),
  report,
  outputDir = defaultSmokeReportDir({ root }),
} = {}) {
  validateSmokeReport(report);
  await mkdir(outputDir, { recursive: true });
  const reportPath = path.join(outputDir, `slint-phase5-smoke-${safeTimestamp(report.timestamp)}.json`);
  await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  return { reportPath, report };
}

function safeTimestamp(timestamp) {
  return timestamp.replace(/[:.]/g, '-');
}

async function readStdin() {
  const chunks = [];
  for await (const chunk of process.stdin) {
    chunks.push(chunk);
  }
  return Buffer.concat(chunks.map((chunk) => Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))).toString('utf8');
}

function parseCliArgs(argv) {
  const args = {
    write: false,
    root: defaultRepoRoot(),
    outputDir: null,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--write') {
      args.write = true;
      continue;
    }
    if (arg === '--root') {
      args.root = path.resolve(argv[index + 1]);
      index += 1;
      continue;
    }
    if (arg === '--output-dir') {
      args.outputDir = path.resolve(argv[index + 1]);
      index += 1;
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }
  return args;
}

function defaultRepoRoot() {
  const __filename = fileURLToPath(import.meta.url);
  return path.resolve(path.dirname(__filename), '..');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const args = parseCliArgs(process.argv.slice(2));
    const report = createSmokeReport(JSON.parse(await readStdin()));
    if (args.write) {
      const result = await writeSmokeReport({
        root: args.root,
        report,
        outputDir: args.outputDir ?? undefined,
      });
      console.log(result.reportPath);
    } else {
      validateSmokeReport(report);
      console.log(JSON.stringify(report, null, 2));
    }
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
