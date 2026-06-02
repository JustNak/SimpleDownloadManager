import type { DesktopSnapshot } from './backend';

export interface InitialAppData {
  snapshot: DesktopSnapshot | null;
  snapshotError: unknown | null;
}

export async function loadInitialAppData(
  getSnapshot: () => Promise<DesktopSnapshot>,
): Promise<InitialAppData> {
  const snapshotResult = await Promise.resolve()
    .then(() => getSnapshot())
    .then(
      (snapshot) => ({ status: 'fulfilled' as const, value: snapshot }),
      (reason) => ({ status: 'rejected' as const, reason }),
    );

  return {
    snapshot: snapshotResult.status === 'fulfilled' ? snapshotResult.value : null,
    snapshotError: snapshotResult.status === 'rejected' ? snapshotResult.reason : null,
  };
}
