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
