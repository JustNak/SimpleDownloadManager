import { mkdir } from 'node:fs/promises';
import { repoPath, runCommand } from './lib/run.mjs';

const liveTemp = repoPath('.tmp', 'live-http-test');

if (!process.env.SDM_HTTP_BENCH_URL) {
  console.log('Skipping live HTTP benchmark: SDM_HTTP_BENCH_URL is not set.');
  process.exit(0);
}

await mkdir(liveTemp, { recursive: true });

const code = await runCommand('cargo', [
  'test',
  '--manifest-path',
  'apps/desktop/src-tauri/Cargo.toml',
  'http_bench::tests::live_http_benchmark_from_env',
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
