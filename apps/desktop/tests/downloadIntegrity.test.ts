import assert from 'node:assert/strict';
import { normalizeSha256Input, validateOptionalSha256 } from '../src/downloadIntegrity.ts';

const validUpper = 'A'.repeat(64);

assert.equal(
  normalizeSha256Input(` ${validUpper} `),
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'valid SHA-256 input should trim and normalize to lowercase',
);

assert.equal(
  validateOptionalSha256(''),
  null,
  'empty checksum input should be treated as no integrity check',
);

assert.equal(
  validateOptionalSha256(validUpper),
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  'valid checksum input should return the normalized digest',
);

assert.throws(
  () => validateOptionalSha256('abc123'),
  /64 hexadecimal characters/,
  'invalid checksum input should produce a clear validation error',
);
