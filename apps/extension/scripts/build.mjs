import { build } from 'esbuild';
import { cp, mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const appRoot = path.resolve(__dirname, '..');
const distRoot = path.join(appRoot, 'dist');
const releaseConfig = JSON.parse(
  readFileSync(path.resolve(appRoot, '..', '..', 'config', 'release.json'), 'utf8'),
);
const browserExtensionVersion = '0.2.4';
const displayVersion = '0.2.4-a';

const targets = [
  {
    name: 'chromium',
    manifest: {
      manifest_version: 3,
      name: 'Simple Download Manager',
      version: browserExtensionVersion,
      version_name: displayVersion,
      description: 'Send downloads to the Simple Download Manager desktop app.',
      key: releaseConfig.chromiumExtensionKey,
      permissions: ['contextMenus', 'downloads', 'nativeMessaging', 'storage'],
      background: {
        service_worker: 'background.js',
        type: 'module'
      },
      action: {
        default_title: 'Simple Download Manager',
        default_popup: 'popup.html'
      },
      options_ui: {
        page: 'options.html',
        open_in_tab: true
      }
    }
  },
  {
    name: 'firefox',
    manifest: {
      manifest_version: 2,
      name: 'Simple Download Manager',
      version: browserExtensionVersion,
      version_name: displayVersion,
      description: 'Send downloads to the Simple Download Manager desktop app.',
      permissions: ['contextMenus', 'downloads', 'nativeMessaging', 'storage'],
      background: {
        scripts: ['background.js']
      },
      browser_action: {
        default_title: 'Simple Download Manager',
        default_popup: 'popup.html'
      },
      options_ui: {
        page: 'options.html',
        open_in_tab: true
      },
      browser_specific_settings: {
        gecko: {
          id: releaseConfig.firefoxExtensionId
        }
      }
    }
  }
];

async function buildTarget(target) {
  const outdir = path.join(distRoot, target.name);
  await mkdir(outdir, { recursive: true });

  await build({
    entryPoints: {
      background: path.join(appRoot, 'src', 'background', 'index.ts'),
      popup: path.join(appRoot, 'src', 'popup', 'index.ts'),
      options: path.join(appRoot, 'src', 'options', 'index.ts')
    },
    bundle: true,
    format: 'esm',
    platform: 'browser',
    target: ['es2022'],
    outdir,
    sourcemap: true,
    logLevel: 'info'
  });

  await cp(path.join(appRoot, 'src', 'popup', 'index.html'), path.join(outdir, 'popup.html'));
  await cp(path.join(appRoot, 'src', 'options', 'index.html'), path.join(outdir, 'options.html'));
  await writeFile(path.join(outdir, 'manifest.json'), JSON.stringify(target.manifest, null, 2));
}

await mkdir(distRoot, { recursive: true });
await Promise.all(targets.map(buildTarget));
