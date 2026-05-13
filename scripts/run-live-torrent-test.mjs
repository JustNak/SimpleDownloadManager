import { mkdir } from 'node:fs/promises';
import { repoPath, runCommand } from './lib/run.mjs';

const liveTemp = repoPath('.tmp', 'live-torrent-test');

if (!process.env.SDM_TORRENT_BENCH_MAGNET) {
  console.log('Skipping live torrent benchmark: SDM_TORRENT_BENCH_MAGNET is not set.');
  process.exit(0);
}

await mkdir(liveTemp, { recursive: true });

const code = await runCommand('cargo', [
  'test',
  '--manifest-path',
  'apps/desktop/src-tauri/Cargo.toml',
  'torrent_bench::tests::live_torrent_benchmark_from_env',
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
