import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { readFileSync } from 'node:fs';

const finalizeModule = new URL('../src-tauri/src/download/bulk_finalize.rs', import.meta.url);

assert.equal(
  existsSync(finalizeModule),
  true,
  'bulk finalization should live in bulk_finalize.rs now that ZIP archive output is gone',
);

const archiveSource = readFileSync(finalizeModule, 'utf8');

assert.match(
  archiveSource,
  /CREATE_NO_WINDOW/,
  'bundled 7-Zip extraction should use CREATE_NO_WINDOW on Windows to avoid console popups',
);

assert.match(
  archiveSource,
  /command\.creation_flags\(CREATE_NO_WINDOW\)/,
  '7-Zip extraction command should apply the hidden-window creation flag before spawning',
);
