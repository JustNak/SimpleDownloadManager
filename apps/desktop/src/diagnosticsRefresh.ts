export interface DiagnosticsRefreshOptions {
  silent?: boolean;
}

export function shouldNotifyDiagnosticsRefreshFailure(options: DiagnosticsRefreshOptions = {}): boolean {
  return options.silent !== true;
}
