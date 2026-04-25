import { isErrorResponse, type HostToExtensionResponse } from '@myapp/protocol';

export interface BrowserDownloadsCleanupApi {
  cancel(downloadId: number): Promise<unknown>;
  erase(query: { id: number }): Promise<unknown>;
}

export function shouldDiscardBrowserDownloadAfterHandoff(response: HostToExtensionResponse): boolean {
  return !isErrorResponse(response) && response.type === 'accepted';
}

export async function discardBrowserDownload(
  downloads: BrowserDownloadsCleanupApi,
  downloadId: number,
): Promise<void> {
  await downloads.cancel(downloadId).catch(() => undefined);
  await downloads.erase({ id: downloadId }).catch(() => undefined);
}
