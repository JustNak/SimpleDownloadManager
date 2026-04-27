import assert from 'node:assert/strict';
import {
  addExcludedHosts,
  filterExcludedHosts,
  formatExcludedSitesSummary,
  normalizeHostInput,
  removeExcludedHost,
} from '../src/settingsExcludedSites.ts';

assert.equal(normalizeHostInput(' https://Example.COM/download/file.zip '), 'example.com');
assert.equal(normalizeHostInput('sub.example.com:8443/path?q=1'), 'sub.example.com');
assert.equal(normalizeHostInput(' https://*.Example.COM/download/file.zip '), '*.example.com');
assert.equal(normalizeHostInput('download*.Example.COM'), 'download*.example.com');
assert.equal(normalizeHostInput('   '), '');

const added = addExcludedHosts(['example.com'], [
  'https://cdn.example.com/file.zip',
  'example.com',
  'https://*.Example.com/releases',
  '*.example.com',
  'mirror.example.org/path',
  '',
]);

assert.deepEqual(added.hosts, ['example.com', 'cdn.example.com', '*.example.com', 'mirror.example.org']);
assert.deepEqual(added.addedHosts, ['cdn.example.com', '*.example.com', 'mirror.example.org']);
assert.deepEqual(added.duplicateHosts, ['example.com', '*.example.com']);

assert.deepEqual(removeExcludedHost(added.hosts, 'cdn.example.com'), ['example.com', '*.example.com', 'mirror.example.org']);
assert.deepEqual(filterExcludedHosts(added.hosts, '*.example'), ['*.example.com']);
assert.deepEqual(filterExcludedHosts(added.hosts, 'mirror'), ['mirror.example.org']);
assert.equal(formatExcludedSitesSummary([]), 'No excluded sites');
assert.equal(formatExcludedSitesSummary(['example.com']), '1 excluded site');
assert.equal(formatExcludedSitesSummary(['example.com', 'mirror.example.org']), '2 excluded sites');
