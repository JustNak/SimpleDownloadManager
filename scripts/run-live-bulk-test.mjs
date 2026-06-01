import { mkdir } from 'node:fs/promises';
import { repoPath, runCommand } from './lib/run.mjs';

const liveTemp = repoPath('.tmp', 'live-bulk-test');

if (!process.env.SDM_BULK_BENCH_URLS) {
  console.log('Skipping live bulk benchmark: SDM_BULK_BENCH_URLS is not set.');
  process.exit(0);
}

await mkdir(liveTemp, { recursive: true });

const code = await runCommand('cargo', [
  'test',
  '--manifest-path',
  'apps/desktop/src-tauri/Cargo.toml',
  'live_bulk_download_rounds_from_env_cleanup_each_round',
  '--',
  '--ignored',
  '--exact',
  '--nocapture',
], {
  env: {
    TEMP: liveTemp,
    TMP: liveTemp,
    TMPDIR: liveTemp,
  },
});
process.exit(code);
