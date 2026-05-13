import { mkdir } from 'node:fs/promises';
import { repoPath, runCommand } from './lib/run.mjs';

const soakTemp = repoPath('.tmp', 'soak-tests');
const minutes = parseMinutes(process.argv.slice(2));
const deadline = Date.now() + minutes * 60 * 1000;
const npmCommand = 'npm';
const gate = [
  ['typecheck', ['run', 'typecheck']],
  ['test:ts', ['run', 'test:ts']],
  ['test:rust', ['run', 'test:rust']],
  ['build:extension', ['run', 'build:extension']],
  ['build:desktop', ['run', 'build:desktop']],
  ['clippy', ['run', 'clippy']],
  ['lint:firefox', ['run', 'lint:firefox']],
];

await mkdir(soakTemp, { recursive: true });

console.log(`Starting soak for ${minutes} minute${minutes === 1 ? '' : 's'}.`);
console.log(`Using TEMP/TMP: ${soakTemp}`);

let cycle = 0;
while (cycle === 0 || Date.now() < deadline) {
  cycle += 1;
  const cycleStarted = Date.now();
  console.log(`\n=== Soak cycle ${cycle} started at ${new Date(cycleStarted).toISOString()} ===`);

  const failures = [];
  for (const [name, args] of gate) {
    const code = await runCommand(npmCommand, args, {
      env: {
        TEMP: soakTemp,
        TMP: soakTemp,
        TMPDIR: soakTemp,
      },
      leadingNewline: true,
    });
    if (code !== 0) {
      failures.push({ name, code });
    }
  }

  if (failures.length > 0) {
    console.error(`\nSoak cycle ${cycle} failed:`);
    for (const failure of failures) {
      console.error(`- ${failure.name} exited with code ${failure.code}`);
    }
    process.exit(1);
  }

  const elapsedSeconds = Math.round((Date.now() - cycleStarted) / 1000);
  console.log(`=== Soak cycle ${cycle} completed in ${elapsedSeconds}s ===`);
}

console.log(`\nSoak completed ${cycle} full cycle${cycle === 1 ? '' : 's'} over ${minutes} minute${minutes === 1 ? '' : 's'}.`);

function parseMinutes(args) {
  const minutesFlagIndex = args.indexOf('--minutes');
  const inlineMinutes = args.find((arg) => arg.startsWith('--minutes='));
  const rawValue = inlineMinutes?.slice('--minutes='.length)
    ?? (minutesFlagIndex >= 0 ? args[minutesFlagIndex + 1] : undefined)
    ?? '60';
  const parsed = Number(rawValue);

  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new Error(`--minutes must be a positive number, got ${rawValue}`);
  }

  return parsed;
}
