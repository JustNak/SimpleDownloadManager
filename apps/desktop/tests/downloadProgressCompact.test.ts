import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const progressSource = readFileSync(new URL('../src/DownloadProgressWindow.svelte', import.meta.url), 'utf8');
const backendSource = readFileSync(new URL('../src/backend.ts', import.meta.url), 'utf8');
const windowsSource = readFileSync(new URL('../src-tauri/src/windows.rs', import.meta.url), 'utf8');

assert.match(progressSource, /@render ProgressView\(/, 'download progress controller should render the compact progress view');
assert.match(progressSource, /#snippet ProgressView/, 'normal downloads should render through a named compact progress view');
assert.doesNotMatch(progressSource, /TorrentingProgressView|Torrent session|Uploaded|Ratio/, 'normal download progress popup should not include torrent-specific UI');
assert.match(progressSource, /PopupTitlebar title="Download progress"/, 'normal progress popup should keep the Download progress title');
assert.match(progressSource, /Metric\('Speed'[\s\S]*Metric\('ETA'[\s\S]*Metric\('Size'/, 'compact download view should keep speed, ETA, and size metrics visible in a flat metric rail');
assert.doesNotMatch(progressSource, /function MetricGrid|rounded border border-border bg-background/, 'progress popup metrics should be flat instead of boxed cards');
assert.match(progressSource, /grid grid-cols-3 gap-2 border-t border-border\/35 bg-background\/30/, 'progress popup metric rail should use softer boxed-in shading and a muted top separator');
assert.match(windowsSource, /width:\s*460\.0,[\s\S]*height:\s*280\.0,/, 'download progress popup geometry should stay compact for normal downloads');
assert.match(backendSource, /width=460,height=280/, 'browser fallback download progress popup should keep the compact 460x280 geometry');
