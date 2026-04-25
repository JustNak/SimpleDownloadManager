import type { DesktopSnapshot } from './backend';
import type { DiagnosticsSnapshot } from './types';

export interface InitialAppData {
  snapshot: DesktopSnapshot | null;
  diagnostics: DiagnosticsSnapshot | null;
  snapshotError: unknown | null;
  diagnosticsError: unknown | null;
}

export async function loadInitialAppData(
  getSnapshot: () => Promise<DesktopSnapshot>,
  getDiagnostics: () => Promise<DiagnosticsSnapshot>,
): Promise<InitialAppData> {
  const [snapshotResult, diagnosticsResult] = await Promise.allSettled([
    getSnapshot(),
    getDiagnostics(),
  ]);

  return {
    snapshot: snapshotResult.status === 'fulfilled' ? snapshotResult.value : null,
    diagnostics: diagnosticsResult.status === 'fulfilled' ? diagnosticsResult.value : null,
    snapshotError: snapshotResult.status === 'rejected' ? snapshotResult.reason : null,
    diagnosticsError: diagnosticsResult.status === 'rejected' ? diagnosticsResult.reason : null,
  };
}
