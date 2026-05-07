import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';

type Capability = { identifier?: string; windows?: string[]; permissions?: string[] };

const capabilityDir = new URL('../src-tauri/capabilities/', import.meta.url);
const capabilities = Object.fromEntries(
  readdirSync(capabilityDir)
    .filter((name) => name.endsWith('.json'))
    .map((name) => {
      const capability = JSON.parse(readFileSync(new URL(name, capabilityDir), 'utf8')) as Capability;
      return [capability.identifier ?? name.replace(/\.json$/, ''), capability];
    }),
);

const allPermissions = Object.values(capabilities).flatMap((capability) => capability.permissions ?? []);

assert.equal(
  allPermissions.includes('core:default'),
  false,
  'capabilities should not grant the broad core:default permission set',
);

assert.deepEqual(
  capabilities.default?.windows,
  ['main'],
  'main window capability should not also cover lower-trust popup windows',
);

assert.deepEqual(
  capabilities.popups?.windows,
  ['download-prompt', 'download-progress-*', 'torrent-progress-*', 'batch-progress-*'],
  'popup windows should be scoped to a separate lower-privilege capability',
);

assert.deepEqual(
  new Set(capabilities.default?.permissions),
  new Set([
    'core:event:allow-listen',
    'core:event:allow-unlisten',
    'core:app:allow-version',
    'core:window:allow-minimize',
    'core:window:allow-is-maximized',
    'core:window:allow-toggle-maximize',
    'core:window:allow-start-dragging',
    'core:window:allow-close',
  ]),
  'main window should get only the event and titlebar permissions used by the frontend',
);

assert.deepEqual(
  new Set(capabilities.popups?.permissions),
  new Set([
    'core:event:allow-listen',
    'core:event:allow-unlisten',
    'core:window:allow-minimize',
    'core:window:allow-start-dragging',
    'core:window:allow-close',
  ]),
  'popup windows should not receive maximize or resize-state permissions',
);
