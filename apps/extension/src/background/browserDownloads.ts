import { isErrorResponse, type HostToExtensionResponse } from '@myapp/protocol';

export type BrowserDownloadFilenameSuggestion = {
  filename?: string;
  conflictAction?: 'uniquify' | 'overwrite' | 'prompt';
};
export type BrowserDownloadFilenameSuggest = (suggestion?: BrowserDownloadFilenameSuggestion) => void;
export interface BrowserDownloadFilenameInterceptionApi<TItem = unknown> {
  onDeterminingFilename: {
    addListener(listener: (item: TItem, suggest: BrowserDownloadFilenameSuggest) => true): void;
  };
}
export type BrowserDownloadFilenameInterceptionCandidate<TItem = unknown> =
  Partial<BrowserDownloadFilenameInterceptionApi<TItem>> | null | undefined;
export type AsyncFilenameInterceptionHandler<TItem> = (
  item: TItem,
  suggest: BrowserDownloadFilenameSuggest,
) => Promise<void> | void;

export interface BrowserDownloadsCleanupApi {
  cancel(downloadId: number): Promise<unknown>;
  erase(query: { id: number }): Promise<unknown>;
}

export function createAsyncFilenameInterceptionListener<TItem>(
  handler: AsyncFilenameInterceptionHandler<TItem>,
): (item: TItem, suggest: BrowserDownloadFilenameSuggest) => true {
  return (item, suggest) => {
    void handler(item, suggest);
    return true;
  };
}

export function selectFilenameInterceptionApi<TItem>(
  polyfillDownloads: BrowserDownloadFilenameInterceptionCandidate<TItem>,
  rawDownloads: BrowserDownloadFilenameInterceptionCandidate<TItem>,
): BrowserDownloadFilenameInterceptionApi<TItem> | null {
  if (rawDownloads?.onDeterminingFilename) {
    return rawDownloads as BrowserDownloadFilenameInterceptionApi<TItem>;
  }

  if (polyfillDownloads?.onDeterminingFilename) {
    return polyfillDownloads as BrowserDownloadFilenameInterceptionApi<TItem>;
  }

  return null;
}

export function shouldDiscardBrowserDownloadAfterHandoff(response: HostToExtensionResponse): boolean {
  return !isErrorResponse(response)
    && response.type === 'accepted'
    && response.payload.status !== 'canceled';
}

export async function discardBrowserDownload(
  downloads: BrowserDownloadsCleanupApi,
  downloadId: number,
): Promise<void> {
  await downloads.cancel(downloadId).catch(() => undefined);
  await downloads.erase({ id: downloadId }).catch(() => undefined);
}

export async function discardBrowserDownloadBeforeFilenameRelease(
  downloads: BrowserDownloadsCleanupApi,
  downloadId: number,
  releaseFilename: () => void,
): Promise<void> {
  let canceledBeforeRelease = false;

  try {
    await downloads.cancel(downloadId);
    canceledBeforeRelease = true;
  } catch {
    canceledBeforeRelease = false;
  }

  releaseFilename();

  if (!canceledBeforeRelease) {
    await downloads.cancel(downloadId).catch(() => undefined);
  }

  await downloads.erase({ id: downloadId }).catch(() => undefined);
}
