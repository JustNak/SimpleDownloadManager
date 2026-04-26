import { isErrorResponse, type ExtensionIntegrationSettings, type HostToExtensionResponse } from '@myapp/protocol';

export type BrowserDownloadFilenameSuggestion = {
  filename?: string;
  conflictAction?: 'uniquify' | 'overwrite' | 'prompt';
};
export type BrowserDownloadFilenameSuggest = (suggestion?: BrowserDownloadFilenameSuggestion) => void;
export type BrowserDownloadState = 'in_progress' | 'interrupted' | 'complete' | string;
export type BrowserDownloadSearchItem = {
  id: number;
  url?: string;
  finalUrl?: string;
  filename?: string;
  state?: BrowserDownloadState;
  exists?: boolean;
};
export type BrowserDownloadOptions = {
  url: string;
  filename?: string;
  conflictAction?: 'uniquify' | 'overwrite' | 'prompt';
  saveAs?: boolean;
};
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
  search(query: { id: number }): Promise<BrowserDownloadSearchItem[]>;
  removeFile?(downloadId: number): Promise<unknown>;
  erase(query: { id: number }): Promise<unknown>;
}

export interface BrowserDownloadsRestartApi {
  download(options: BrowserDownloadOptions): Promise<number>;
}

export type BrowserDownloadReplayItem = {
  id: number;
  url?: string;
  finalUrl?: string;
  filename?: string;
};
export type BrowserDownloadPolicyItem = {
  url?: string;
  finalUrl?: string;
  filename?: string;
};
export type FirefoxWebRequestHeader = {
  name: string;
  value?: string;
};
export type FirefoxWebRequestDownloadDetails = {
  url: string;
  method?: string;
  type?: string;
  responseHeaders?: FirefoxWebRequestHeader[];
  incognito?: boolean;
};
export type FirefoxWebRequestDownloadCandidate = {
  url: string;
  filename?: string;
  totalBytes?: number;
  incognito: boolean;
};

export type BrowserDownloadBypassState = {
  downloadIds: Set<number>;
  urls: Map<string, number>;
};

const FIREFOX_DOWNLOAD_RESOURCE_TYPES = new Set(['main_frame', 'sub_frame']);
const FIREFOX_DOWNLOAD_MIME_TYPES = new Set([
  'application/octet-stream',
  'application/zip',
  'application/x-zip-compressed',
  'application/x-7z-compressed',
  'application/x-rar-compressed',
  'application/x-tar',
  'application/gzip',
  'application/x-bzip2',
  'application/x-xz',
  'application/pdf',
  'application/x-msdownload',
  'application/x-msi',
]);

export function createBrowserDownloadBypassState(): BrowserDownloadBypassState {
  return {
    downloadIds: new Set<number>(),
    urls: new Map<string, number>(),
  };
}

export function shouldBypassBrowserDownload(
  item: BrowserDownloadReplayItem,
  bypass: BrowserDownloadBypassState,
): boolean {
  if (bypass.downloadIds.delete(item.id)) {
    consumeBypassUrl(bypass, item.finalUrl);
    consumeBypassUrl(bypass, item.url);
    return true;
  }

  return consumeBypassUrl(bypass, item.finalUrl) || consumeBypassUrl(bypass, item.url);
}

export function markBrowserDownloadBypassUrl(bypass: BrowserDownloadBypassState, url: string): void {
  addBypassUrl(bypass, url);
}

export function revokeBrowserDownloadBypassUrl(bypass: BrowserDownloadBypassState, url: string): void {
  revokeBypassUrl(bypass, url);
}

export function shouldBypassBrowserDownloadUrl(url: string | undefined, bypass: BrowserDownloadBypassState): boolean {
  return consumeBypassUrl(bypass, url);
}

export function browserDownloadUrl(item: BrowserDownloadPolicyItem): string | undefined {
  if (isHttpUrl(item.finalUrl)) {
    return item.finalUrl;
  }

  return isHttpUrl(item.url) ? item.url : undefined;
}

export function shouldHandleBrowserDownload(
  item: BrowserDownloadPolicyItem,
  settings: ExtensionIntegrationSettings,
): boolean {
  const url = browserDownloadUrl(item);
  if (!url) {
    return false;
  }

  const isTorrentBrowserDownload = isTorrentUrl(url) || isTorrentFilename(item.filename);
  return settings.enabled
    && settings.downloadHandoffMode !== 'off'
    && !isHostExcluded(url, settings.excludedHosts)
    && (isTorrentBrowserDownload || !isFileExtensionIgnored(url, item.filename, settings.ignoredFileExtensions));
}

export function firefoxWebRequestDownloadCandidate(
  details: FirefoxWebRequestDownloadDetails,
  settings: ExtensionIntegrationSettings,
): FirefoxWebRequestDownloadCandidate | null {
  if (details.method && details.method.toUpperCase() !== 'GET') {
    return null;
  }

  if (details.type && !FIREFOX_DOWNLOAD_RESOURCE_TYPES.has(details.type)) {
    return null;
  }

  const url = browserDownloadUrl({ url: details.url });
  if (!url) {
    return null;
  }

  const contentDisposition = headerValue(details.responseHeaders, 'content-disposition');
  const contentType = normalizeContentType(headerValue(details.responseHeaders, 'content-type'));
  const filename = filenameFromContentDisposition(contentDisposition) ?? basenameFromUrl(url);
  const isAttachment = /\battachment\b/i.test(contentDisposition ?? '');
  const hasDownloadMimeType = Boolean(contentType && FIREFOX_DOWNLOAD_MIME_TYPES.has(contentType));

  if (!isAttachment && !hasDownloadMimeType) {
    return null;
  }

  if (!shouldHandleBrowserDownload({ url, filename }, settings)) {
    return null;
  }

  return {
    url,
    filename,
    totalBytes: positiveIntegerHeader(details.responseHeaders, 'content-length'),
    incognito: details.incognito ?? false,
  };
}

export function createAsyncFilenameInterceptionListener<TItem>(
  handler: AsyncFilenameInterceptionHandler<TItem>,
): (item: TItem, suggest: BrowserDownloadFilenameSuggest) => true {
  return (item, suggest) => {
    const suggestOnce = createSuggestOnce(suggest);
    void Promise.resolve(handler(item, suggestOnce)).catch(() => {
      suggestOnce();
    });
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
  await removeCompletedBrowserDownloadFile(downloads, downloadId);
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

  await removeCompletedBrowserDownloadFile(downloads, downloadId);
  await downloads.erase({ id: downloadId }).catch(() => undefined);
}

export async function restartBrowserDownload(
  downloads: BrowserDownloadsRestartApi,
  item: BrowserDownloadReplayItem,
  bypass: BrowserDownloadBypassState,
): Promise<number> {
  const url = browserDownloadUrl(item);
  if (!url) {
    throw new Error('Could not return the download to the browser because the original URL is missing.');
  }

  addBypassUrl(bypass, url);

  try {
    const options: BrowserDownloadOptions = {
      url,
      conflictAction: 'uniquify',
      saveAs: false,
    };
    const filename = basenameOnly(item.filename);
    if (filename) {
      options.filename = filename;
    }

    const downloadId = await downloads.download(options);
    bypass.downloadIds.add(downloadId);
    return downloadId;
  } catch (error) {
    revokeBypassUrl(bypass, url);
    throw error;
  }
}

function createSuggestOnce(suggest: BrowserDownloadFilenameSuggest): BrowserDownloadFilenameSuggest {
  let called = false;
  return (suggestion) => {
    if (called) {
      return;
    }

    called = true;
    suggest(suggestion);
  };
}

async function removeCompletedBrowserDownloadFile(
  downloads: BrowserDownloadsCleanupApi,
  downloadId: number,
): Promise<void> {
  const items = await downloads.search({ id: downloadId }).catch(() => []);
  const item = items[0];
  if (!item || item.state !== 'complete' || item.exists === false) {
    return;
  }

  await downloads.removeFile?.(downloadId).catch(() => undefined);
}

function addBypassUrl(bypass: BrowserDownloadBypassState, url: string): void {
  bypass.urls.set(url, (bypass.urls.get(url) ?? 0) + 1);
}

function revokeBypassUrl(bypass: BrowserDownloadBypassState, url: string): void {
  const count = bypass.urls.get(url) ?? 0;
  if (count <= 1) {
    bypass.urls.delete(url);
    return;
  }

  bypass.urls.set(url, count - 1);
}

function consumeBypassUrl(bypass: BrowserDownloadBypassState, url: string | undefined): boolean {
  if (!url) {
    return false;
  }

  const count = bypass.urls.get(url) ?? 0;
  if (count === 0) {
    return false;
  }

  revokeBypassUrl(bypass, url);
  return true;
}

function headerValue(headers: FirefoxWebRequestHeader[] | undefined, name: string): string | undefined {
  const header = headers?.find((candidate) => candidate.name.toLowerCase() === name);
  return header?.value;
}

function normalizeContentType(value: string | undefined): string | undefined {
  return value?.split(';', 1)[0]?.trim().toLowerCase() || undefined;
}

function positiveIntegerHeader(headers: FirefoxWebRequestHeader[] | undefined, name: string): number | undefined {
  const value = headerValue(headers, name);
  if (!value) {
    return undefined;
  }

  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : undefined;
}

function filenameFromContentDisposition(value: string | undefined): string | undefined {
  if (!value) {
    return undefined;
  }

  const encodedFilename = /(?:^|;)\s*filename\*\s*=\s*(?:UTF-8''|)([^;]+)/i.exec(value)?.[1];
  if (encodedFilename) {
    return decodeFilename(encodedFilename);
  }

  const quotedFilename = /(?:^|;)\s*filename\s*=\s*"([^"]+)"/i.exec(value)?.[1];
  if (quotedFilename) {
    return basenameOnly(quotedFilename);
  }

  const plainFilename = /(?:^|;)\s*filename\s*=\s*([^;]+)/i.exec(value)?.[1];
  return plainFilename ? basenameOnly(plainFilename) : undefined;
}

function decodeFilename(value: string): string | undefined {
  const cleaned = value.trim().replace(/^"|"$/g, '');
  try {
    return basenameOnly(decodeURIComponent(cleaned));
  } catch {
    return basenameOnly(cleaned);
  }
}

function basenameOnly(path: string | undefined): string | undefined {
  if (!path) return undefined;
  const normalized = path.replaceAll('\\', '/');
  return normalized.split('/').filter(Boolean).pop();
}

function isHttpUrl(url: string | undefined): url is string {
  if (!url) return false;
  try {
    const parsed = new URL(url);
    return parsed.protocol === 'http:' || parsed.protocol === 'https:';
  } catch {
    return false;
  }
}

function isHostExcluded(url: string, excludedHosts: string[]): boolean {
  const hostname = new URL(url).hostname.toLowerCase();
  return excludedHosts.some((host) => hostname === host || hostname.endsWith(`.${host}`));
}

function isFileExtensionIgnored(url: string, filename: string | undefined, ignoredExtensions: string[] = []): boolean {
  const extensions = ignoredExtensions.map(normalizeFileExtension).filter(Boolean);
  if (extensions.length === 0) {
    return false;
  }

  const candidates = [basenameOnly(filename), basenameFromUrl(url)]
    .filter((candidate): candidate is string => Boolean(candidate))
    .map((candidate) => candidate.toLowerCase());

  return candidates.some((candidate) => extensions.some((extension) => candidate.endsWith(`.${extension}`)));
}

function normalizeFileExtension(value: string): string {
  return value.trim().replace(/^\.+/, '').toLowerCase();
}

function basenameFromUrl(url: string): string | undefined {
  try {
    const parsed = new URL(url);
    const pathname = decodeURIComponent(parsed.pathname);
    return basenameOnly(pathname);
  } catch {
    return undefined;
  }
}

function isTorrentUrl(url: string | undefined): boolean {
  if (!url) return false;
  try {
    const parsed = new URL(url);
    if (parsed.protocol === 'magnet:') return true;
    return (parsed.protocol === 'http:' || parsed.protocol === 'https:')
      && parsed.pathname.toLowerCase().endsWith('.torrent');
  } catch {
    return false;
  }
}

function isTorrentFilename(filename: string | undefined): boolean {
  if (!filename) return false;
  return basenameOnly(filename)?.toLowerCase().endsWith('.torrent') ?? false;
}
