import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const appSource = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
const toastAreaSource = await readFile(new URL('../src/ToastArea.svelte', import.meta.url), 'utf8');

assert.match(appSource, /function removeToast\(id: string\)/, 'toast dismissal should be centralized in the app shell');
assert.match(appSource, /toasts = toasts\.filter\(\(toast\) => toast\.id !== id\)/, 'toast dismissal should remove by toast id');
assert.match(toastAreaSource, /window\.setTimeout\(\(\) => onRemove\(toast\.id\), 4200\)/, 'toast auto-close should dismiss by toast id from inside the timer');
assert.match(toastAreaSource, /return \(\) => timers\.forEach\(\(timer\) => window\.clearTimeout\(timer\)\)/, 'toast auto-close timers should be cleaned up by the Svelte effect');
