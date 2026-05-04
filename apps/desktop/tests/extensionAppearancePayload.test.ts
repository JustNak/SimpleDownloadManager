import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const storageSource = readFileSync(new URL('../src-tauri/src/storage/mod.rs', import.meta.url), 'utf8');
const ipcSource = readFileSync(new URL('../src-tauri/src/ipc/mod.rs', import.meta.url), 'utf8');
const lifecycleSource = readFileSync(new URL('../src-tauri/src/state/lifecycle.rs', import.meta.url), 'utf8');

assert.match(storageSource, /pub struct AppearanceSettings/, 'desktop storage should define an extension-safe appearance settings payload');
assert.match(storageSource, /pub theme: Theme/, 'appearance settings should carry the desktop theme enum');
assert.match(storageSource, /pub accent_color: String/, 'appearance settings should carry the normalized accent color');
assert.match(lifecycleSource, /pub async fn appearance_settings/, 'shared state should expose current appearance settings for extension status responses');
assert.match(ipcSource, /appearanceSettings/, 'desktop ready responses should serialize appearance settings for the extension');
