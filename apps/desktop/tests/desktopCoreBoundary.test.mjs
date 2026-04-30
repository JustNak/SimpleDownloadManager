import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const coreLib = await readFile('apps/desktop-core/src/lib.rs', 'utf8');
const coreManifest = await readFile('apps/desktop-core/Cargo.toml', 'utf8');
const tauriManifest = await readFile('apps/desktop/src-tauri/Cargo.toml', 'utf8');
const tauriDownloadSource = await readFile('apps/desktop/src-tauri/src/download/mod.rs', 'utf8');
const tauriTorrentSource = await readFile('apps/desktop/src-tauri/src/torrent.rs', 'utf8');

assert.doesNotMatch(
  coreLib,
  /src-tauri/,
  'desktop-core modules should be physically owned by desktop-core, not path-included from Tauri',
);

for (const forbiddenDependency of [
  'tauri',
  'tauri-plugin',
  'rfd',
  'winreg',
  'windows-sys',
]) {
  assert.doesNotMatch(
    coreManifest,
    new RegExp(`^${forbiddenDependency}\\b`, 'm'),
    `desktop-core should not depend on ${forbiddenDependency}`,
  );
}

for (const forbiddenTauriHttpImplementationMarker of [
  'async fn run_http_download_attempt',
  'pub async fn probe_browser_handoff_access',
  'struct RangeBackoffRegistry',
  'async fn send_request',
  'fn parse_content_disposition_filename',
  'async fn compute_sha256',
]) {
  assert.doesNotMatch(
    tauriDownloadSource,
    new RegExp(forbiddenTauriHttpImplementationMarker.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
    `Tauri download module should delegate HTTP transfer behavior to desktop-core instead of defining ${forbiddenTauriHttpImplementationMarker}`,
  );
}

for (const forbiddenTauriTorrentDependency of [
  'librqbit',
  'aws-lc-rs',
]) {
  assert.doesNotMatch(
    tauriManifest,
    new RegExp(`^${forbiddenTauriTorrentDependency}\\s*=`, 'm'),
    `Tauri backend should not depend directly on ${forbiddenTauriTorrentDependency}; torrent runtime belongs in desktop-core`,
  );
}

for (const forbiddenTauriTorrentImplementationMarker of [
  'use librqbit::',
  'pub struct TorrentEngine',
  'Session::new_with_opts',
  'AddTorrent::from_cli_argument',
]) {
  assert.doesNotMatch(
    tauriTorrentSource,
    new RegExp(forbiddenTauriTorrentImplementationMarker.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
    `Tauri torrent module should re-export desktop-core torrent behavior instead of defining ${forbiddenTauriTorrentImplementationMarker}`,
  );
}

for (const forbiddenTauriTorrentTransferMarker of [
  'async fn run_torrent_download_attempt',
  'struct TorrentLowThroughputMonitor',
  'struct TorrentRestoreWatchdog',
  'struct TorrentPeerConnectionWatchdog',
  'async fn add_torrent_with_controls',
  'fn torrent_low_throughput_message',
]) {
  assert.doesNotMatch(
    tauriDownloadSource,
    new RegExp(forbiddenTauriTorrentTransferMarker.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
    `Tauri download module should delegate torrent transfer behavior to desktop-core instead of defining ${forbiddenTauriTorrentTransferMarker}`,
  );
}

for (const forbiddenTauriSchedulerMarker of [
  'emit_snapshot(',
  'notification()',
  'start_download_worker',
  'finish_interrupted_job',
  'handle_external_reseed_failure',
  'fail_job(',
  'clear_handoff_auth',
  'run_http_download',
  'run_torrent_download',
]) {
  assert.doesNotMatch(
    tauriDownloadSource,
    new RegExp(forbiddenTauriSchedulerMarker.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
    `Tauri download module should delegate transfer scheduler and worker handling to desktop-core instead of containing ${forbiddenTauriSchedulerMarker}`,
  );
}
