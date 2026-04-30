import type { DiagnosticsSnapshot } from './types';

export function formatDiagnosticsReport(diagnostics: DiagnosticsSnapshot): string {
  const lines = [
    'Simple Download Manager Diagnostics',
    `Connection State: ${diagnostics.connectionState}`,
    `Last Host Contact: ${diagnostics.lastHostContactSecondsAgo ?? 'never'} seconds ago`,
    `Queue Total: ${diagnostics.queueSummary.total}`,
    `Queue Active: ${diagnostics.queueSummary.active}`,
    `Queue Needs Attention: ${diagnostics.queueSummary.attention}`,
    `Queue Queued: ${diagnostics.queueSummary.queued}`,
    `Queue Downloading: ${diagnostics.queueSummary.downloading}`,
    `Queue Completed: ${diagnostics.queueSummary.completed}`,
    `Queue Failed: ${diagnostics.queueSummary.failed}`,
    `Host Registration Status: ${diagnostics.hostRegistration.status}`,
    '',
    'Host Registration Entries:',
  ];

  for (const entry of diagnostics.hostRegistration.entries) {
    lines.push(`- ${entry.browser}`);
    lines.push(`  Registry: ${entry.registryPath}`);
    lines.push(`  Manifest: ${entry.manifestPath ?? 'missing'}`);
    lines.push(`  Manifest Exists: ${entry.manifestExists}`);
    lines.push(`  Host Binary: ${entry.hostBinaryPath ?? 'missing'}`);
    lines.push(`  Host Binary Exists: ${entry.hostBinaryExists}`);
  }

  lines.push('', 'Torrent Diagnostics:');
  const torrents = diagnostics.torrentDiagnostics ?? [];
  if (torrents.length === 0) {
    lines.push('- none');
  } else {
    for (const torrent of torrents) {
      const torrentDiagnostics = torrent.diagnostics;
      lines.push(`- ${torrent.jobId} ${torrent.filename}`);
      if (torrent.infoHash) lines.push(`  Info Hash: ${torrent.infoHash}`);
      lines.push(`  Live Peers: ${torrentDiagnostics.livePeers}`);
      lines.push(`  Seen Peers: ${torrentDiagnostics.seenPeers}`);
      lines.push(`  Contributing Peers: ${torrentDiagnostics.contributingPeers}`);
      lines.push(`  Peer Error Events: ${torrentDiagnostics.peerErrors}`);
      lines.push(`  Peers With Errors: ${torrentDiagnostics.peersWithErrors}`);
      lines.push(`  Peer Connection Attempts: ${torrentDiagnostics.peerConnectionAttempts}`);
      lines.push(`  Queued Peers: ${torrentDiagnostics.queuedPeers}`);
      lines.push(`  Connecting Peers: ${torrentDiagnostics.connectingPeers}`);
      lines.push(`  Dead Peers: ${torrentDiagnostics.deadPeers}`);
      lines.push(`  Not Needed Peers: ${torrentDiagnostics.notNeededPeers}`);
      lines.push(`  Session Download Speed: ${torrentDiagnostics.sessionDownloadSpeed} B/s`);
      lines.push(`  Session Upload Speed: ${torrentDiagnostics.sessionUploadSpeed} B/s`);
      if (typeof torrentDiagnostics.averagePieceDownloadMillis === 'number') {
        lines.push(`  Average Piece Download: ${torrentDiagnostics.averagePieceDownloadMillis} ms`);
      }
      lines.push(`  Listen Port: ${formatTorrentListenPort(torrentDiagnostics.listenPort, torrentDiagnostics.listenerFallback)}`);
      const samples = torrentDiagnostics.peerSamples ?? [];
      if (samples.length > 0) {
        lines.push('  Peer Samples:');
        for (const sample of samples) {
          lines.push(`  - ${sample.state} fetched ${sample.fetchedBytes} bytes, errors ${sample.errors}, pieces ${sample.downloadedPieces}, attempts ${sample.connectionAttempts}`);
        }
      }
    }
  }

  lines.push('', 'Recent Events:');
  const events = diagnostics.recentEvents ?? [];
  if (events.length === 0) {
    lines.push('- none');
  } else {
    for (const event of events) {
      const job = event.jobId ? ` ${event.jobId}` : '';
      lines.push(`- ${formatDiagnosticTimestamp(event.timestamp)} ${event.level} ${event.category}${job} ${event.message}`);
    }
  }

  return lines.join('\n');
}

function formatDiagnosticTimestamp(timestamp: number): string {
  if (!Number.isFinite(timestamp) || timestamp <= 0) return 'unknown-time';
  return new Date(timestamp).toISOString();
}

function formatTorrentListenPort(listenPort: number | undefined, listenerFallback: boolean): string {
  if (typeof listenPort === 'number') {
    return listenerFallback ? `${listenPort} (fallback active)` : String(listenPort);
  }

  return listenerFallback ? 'unavailable (fallback active)' : 'unavailable';
}
