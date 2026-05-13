import { mkdir } from 'node:fs/promises';
import { repoPath, runCommand } from './lib/run.mjs';

const rustTemp = repoPath('.tmp', 'cargo-test');
const rustEnv = {
  TEMP: rustTemp,
  TMP: rustTemp,
  TMPDIR: rustTemp,
};

await mkdir(rustTemp, { recursive: true });

const desktopCode = await runCommand('cargo', [
  'test',
  '--manifest-path',
  'apps/desktop/src-tauri/Cargo.toml',
], {
  env: rustEnv,
});

if (desktopCode !== 0) {
  process.exit(desktopCode);
}

const nativeHostCode = await runCommand('cargo', [
  'test',
  '--manifest-path',
  'apps/native-host/Cargo.toml',
], {
  env: rustEnv,
});

process.exit(nativeHostCode);
