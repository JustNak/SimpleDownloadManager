import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, '..');
const webExtBin = path.join(repoRoot, 'node_modules', 'web-ext', 'bin', 'web-ext.js');
const firefoxSourceDir = 'apps/extension/dist/firefox';

const child = spawn(
  process.execPath,
  [webExtBin, 'lint', '--source-dir', firefoxSourceDir],
  {
    cwd: repoRoot,
    env: {
      ...process.env,
      NO_UPDATE_NOTIFIER: '1',
    },
    stdio: 'inherit',
  },
);

child.on('error', (error) => {
  console.error(error);
  process.exit(1);
});

child.on('exit', (code) => {
  process.exit(code ?? 1);
});
