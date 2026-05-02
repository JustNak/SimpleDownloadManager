export interface DiagnosticsRefreshOptions {
  silent?: boolean;
  force?: boolean;
}

export const DIAGNOSTICS_REFRESH_INTERVAL_MS = 30_000;

export function shouldNotifyDiagnosticsRefreshFailure(options: DiagnosticsRefreshOptions = {}): boolean {
  return options.silent !== true;
}

export function shouldRefreshDiagnostics(
  now: number,
  lastRefreshedAt: number,
  options: DiagnosticsRefreshOptions = {},
): boolean {
  if (options.force || options.silent !== true) return true;
  return now - lastRefreshedAt >= DIAGNOSTICS_REFRESH_INTERVAL_MS;
}
