import { mkdir } from 'node:fs/promises';
import { repoPath, runCommand } from './lib/run.mjs';

const scenarioTemp = repoPath('.tmp', 'scenario-tests');

await mkdir(scenarioTemp, { recursive: true });

const code = await runCommand('cargo', [
  'test',
  '--manifest-path',
  'apps/desktop/src-tauri/Cargo.toml',
  'scenario_',
  '--',
  '--nocapture',
], {
  env: {
    TEMP: scenarioTemp,
    TMP: scenarioTemp,
    TMPDIR: scenarioTemp,
  },
});
process.exit(code);
