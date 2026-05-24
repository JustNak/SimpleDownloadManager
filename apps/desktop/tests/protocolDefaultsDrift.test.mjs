import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const repoRoot = path.resolve(path.dirname(__filename), '..', '..', '..');

const protocolSource = await readFile(path.join(repoRoot, 'packages/protocol/src/index.ts'), 'utf8');
const desktopDefaultsSource = await readFile(path.join(repoRoot, 'apps/desktop/src/defaultSettings.ts'), 'utf8');
const extensionDefaultsSource = await readFile(path.join(repoRoot, 'apps/extension/src/shared/defaultExtensionSettings.ts'), 'utf8');
const storageSource = await readFile(path.join(repoRoot, 'apps/desktop/src-tauri/src/storage/mod.rs'), 'utf8');

assert.match(protocolSource, /export const DEFAULT_EXTENSION_LISTEN_PORT = 1420;/, 'protocol should own the extension listen-port default');
assert.match(protocolSource, /export const DEFAULT_EXTENSION_EXCLUDED_HOSTS = \[\] as const;/, 'protocol should own the extension excluded-host default');
assert.match(protocolSource, /export const DEFAULT_PROTECTED_DOWNLOAD_AUTH_HOSTS = \['gofile\.io', '\*\.instructure\.com'\] as const;/, 'protocol should own the built-in protected-download host default');

assert.match(desktopDefaultsSource, /export const DEFAULT_EXTENSION_LISTEN_PORT = 1420;/, 'desktop preview defaults should stay aligned with the protocol listen-port default');
assert.match(desktopDefaultsSource, /export const DEFAULT_EXTENSION_EXCLUDED_HOSTS = \[\] as const;/, 'desktop preview defaults should stay aligned with Rust and extension excluded-host defaults');
assert.match(desktopDefaultsSource, /export const DEFAULT_PROTECTED_DOWNLOAD_AUTH_HOSTS = \['gofile\.io', '\*\.instructure\.com'\] as const;/, 'desktop preview defaults should stay aligned with protected-download host defaults');
assert.match(desktopDefaultsSource, /excludedHosts: \[\.\.\.DEFAULT_EXTENSION_EXCLUDED_HOSTS\]/, 'desktop preview settings should use the centralized desktop excluded-host default');
assert.match(desktopDefaultsSource, /authenticatedHandoffEnabled: true/, 'desktop preview defaults should enable Protected Downloads');
assert.match(desktopDefaultsSource, /protectedDownloadAuthScope: 'allowlist'/, 'desktop preview defaults should use allowlist Protected Downloads');
assert.match(desktopDefaultsSource, /authenticatedHandoffHosts: \[\.\.\.DEFAULT_PROTECTED_DOWNLOAD_AUTH_HOSTS\]/, 'desktop preview defaults should include built-in protected-download hosts');

assert.match(extensionDefaultsSource, /DEFAULT_EXTENSION_LISTEN_PORT/, 'extension defaults should import the listen-port default from protocol');
assert.match(extensionDefaultsSource, /DEFAULT_EXTENSION_EXCLUDED_HOSTS/, 'extension defaults should import excluded-host defaults from protocol');
assert.match(extensionDefaultsSource, /DEFAULT_PROTECTED_DOWNLOAD_AUTH_HOSTS/, 'extension defaults should import protected-download host defaults from protocol');

assert.match(storageSource, /const DEFAULT_EXCLUDED_HOSTS: &\[&str\] = &\[\];/, 'Rust settings defaults should include the same excluded host as TypeScript defaults');
assert.match(storageSource, /const DEFAULT_PROTECTED_DOWNLOAD_AUTH_HOSTS: &\[&str\] = &\["gofile\.io", "\*\.instructure\.com"\];/, 'Rust settings defaults should include the built-in protected-download hosts');
assert.match(storageSource, /pub fn default_extension_listen_port\(\) -> u32 \{\s*1420\s*\}/, 'Rust settings defaults should use the same listen port as protocol');
