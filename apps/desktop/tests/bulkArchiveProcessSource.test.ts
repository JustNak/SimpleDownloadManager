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

assert.doesNotMatch(
  archiveSource,
  /write_zip_archive|finish_prepared_zip_output|ZipCentralDirectoryEntry/,
  'bulk finalization should not keep the removed ZIP archive writer path',
);

assert.doesNotMatch(
  archiveSource,
  /BulkArchiveOutputKind::Archive\s*=>/,
  'bulk finalization should not branch into archive output after File Combine was reduced to folder output',
);

assert.match(
  archiveSource,
  /std::fs::rename/,
  'folder finalization should use move-first filesystem renames before falling back to copy',
);
