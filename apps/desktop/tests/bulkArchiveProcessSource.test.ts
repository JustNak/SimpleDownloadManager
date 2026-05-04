import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const archiveSource = readFileSync(new URL('../src-tauri/src/download/archive.rs', import.meta.url), 'utf8');

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
