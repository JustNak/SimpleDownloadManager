import assert from 'node:assert/strict';
import {
  defaultBulkArchiveNameForUrls,
  defaultBulkOutputNameForUrls,
  normalizeArchiveName,
  normalizeBulkOutputName,
  stripZipExtension,
} from '../src/bulkArchiveNaming.ts';

assert.equal(
  defaultBulkArchiveNameForUrls([
    'https://fuckingfast.co/ecw0lw398okf#I_Am_Jesus_Christ_--_fitgirl-repacks.site_--_.part01.rar',
  ]),
  'I_Am_Jesus_Christ_--_fitgirl-repacks.site_--_.zip',
  'bulk archive name should come from the first multipart filename fragment',
);

assert.equal(
  defaultBulkArchiveNameForUrls([
    'https://example.com/files/Payload.001',
  ]),
  'Payload.zip',
  'bulk archive name should strip .001 multipart suffixes',
);

assert.equal(
  defaultBulkArchiveNameForUrls([
    'https://example.com/files/readme.pdf',
  ]),
  'bulk-download.zip',
  'non-archive bulk links should keep the generic fallback name',
);

assert.equal(
  normalizeArchiveName('Bad<Name>.zip'),
  'BadName.zip',
  'archive name normalization should remove Windows-unsafe characters',
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

assert.equal(
  normalizeBulkOutputName('I_Am_Jesus', 'archive'),
  'I_Am_Jesus.zip',
  'archive output normalization should append .zip',
);

assert.equal(
  stripZipExtension('I_Am_Jesus.zip'),
  'I_Am_Jesus',
  'switching from archive to folder should use the zip stem',
);
