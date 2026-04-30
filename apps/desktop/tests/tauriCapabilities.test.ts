import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const capability = JSON.parse(
  readFileSync(new URL('../src-tauri/capabilities/default.json', import.meta.url), 'utf8'),
) as { windows?: string[]; permissions?: string[] };

assert.ok(
  capability.windows?.includes('download-progress-*'),
  'download progress popup windows should be covered by the default capability',
);

assert.ok(
  capability.windows?.includes('torrent-progress-*'),
  'torrent progress popup windows should be covered by the default capability',
);

assert.ok(
  capability.windows?.includes('batch-progress-*'),
  'batch progress popup windows should be covered by the default capability',
);

assert.ok(
  capability.permissions?.includes('core:window:allow-start-dragging'),
  'popup titlebars need permission to start native window dragging',
);
