import type { TransferKind } from './types';
import type { BulkOutputKind } from './bulkArchiveNaming';

export type AddJobOptions =
  | string
  | null
  | undefined
  | {
      expectedSha256?: string | null;
      transferKind?: TransferKind;
    };

export interface AddJobsOptions {
  resolveHosterLinks?: boolean;
  startPaused?: boolean;
  bulkOutputKind?: BulkOutputKind;
}

export function buildAddJobCommandArgs(url: string, options?: AddJobOptions) {
  const expectedSha256 = typeof options === 'string' || options === null ? options : options?.expectedSha256;
  const transferKind = typeof options === 'object' && options !== null && 'transferKind' in options
    ? options.transferKind
    : inferTransferKindForUrl(url);

  return {
    url,
    expectedSha256: expectedSha256 ? normalizeExpectedSha256(expectedSha256) : null,
    ...(transferKind === 'torrent' ? { transferKind } : {}),
  };
}

function normalizeExpectedSha256(value: string): string {
  const normalized = value.trim().toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(normalized)) {
    throw new Error('SHA-256 checksum must be 64 hexadecimal characters.');
  }
  return normalized;
}

export function inferTransferKindForUrl(url: string): TransferKind {
  try {
    const parsed = new URL(url);
    if (parsed.protocol === 'magnet:' || parsed.pathname.toLowerCase().endsWith('.torrent')) {
      return 'torrent';
    }
  } catch {
    return 'http';
  }

  return 'http';
}

export function buildAddJobsCommandArgs(
  urls: string[],
  bulkArchiveName?: string,
  options: AddJobsOptions = {},
) {
  return {
    urls: urls.map((url) => url.trim()).filter(Boolean),
    bulkArchiveName: bulkArchiveName?.trim() || undefined,
    resolveHosterLinks: options.resolveHosterLinks ? true : undefined,
    ...(options.startPaused ? { startPaused: true } : {}),
    ...(options.bulkOutputKind ? { bulkOutputKind: options.bulkOutputKind } : {}),
  };
}
