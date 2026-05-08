import assert from 'node:assert/strict';
import {
  defaultBulkArchiveNameForUrls,
  defaultBulkOutputNameForUrls,
  normalizeBulkOutputName,
} from '../src/bulkArchiveNaming.ts';

assert.equal(
  defaultBulkArchiveNameForUrls([
    'https://fuckingfast.co/ecw0lw398okf#I_Am_Jesus_Christ_--_fitgirl-repacks.site_--_.part01.rar',
  ]),
  'I_Am_Jesus_Christ_--_fitgirl-repacks.site_--_',
  'bulk folder name should come from the first multipart filename fragment',
);

assert.equal(
  defaultBulkArchiveNameForUrls([
    'https://example.com/files/Payload.001',
  ]),
  'Payload',
  'bulk folder name should strip .001 multipart suffixes',
);

assert.equal(
  defaultBulkArchiveNameForUrls([
    'https://example.com/files/readme.pdf',
  ]),
  'bulk-download',
  'non-multipart bulk links should keep the generic folder fallback name',
);

assert.equal(
  defaultBulkOutputNameForUrls([
    'https://fuckingfast.co/ecw0lw398okf#I_Am_Jesus_Christ_--_fitgirl-repacks.site_--_.part01.rar',
  ], 'folder'),
  'I_Am_Jesus_Christ_--_fitgirl-repacks.site_--_',
  'folder output name should use the multipart stem without forcing a .zip suffix',
);

assert.equal(
  normalizeBulkOutputName('I_Am_Jesus.zip', 'folder'),
  'I_Am_Jesus.zip',
  'folder output normalization should not force or strip extensions typed by the user',
);
