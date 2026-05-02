import {
  isErrorResponse,
  isUrlHostExcludedByPatterns,
  type ExtensionIntegrationSettings,
  type HandoffAuth,
  type HostToExtensionResponse,
} from '@myapp/protocol';

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
  requestId?: string;
  url: string;
  method?: string;
  type?: string;
  responseHeaders?: FirefoxWebRequestHeader[];
  incognito?: boolean;
};
export type FirefoxWebRequestDownloadCandidate = {
  requestId?: string;
  url: string;
  filename?: string;
  totalBytes?: number;
  incognito: boolean;
};
export type BrowserDownloadHandoffMetadata = {
  suggestedFilename?: string;
  totalBytes?: number;
  handoffAuth?: HandoffAuth;
};

type BrowserDownloadBypassUrlEntry = {
  count: number;
  expiresAt: number;
};

export type BrowserDownloadBypassState = {
  downloadIds: Map<number, number>;
  urls: Map<string, BrowserDownloadBypassUrlEntry>;
  ttlMs: number;
};

export type BrowserDownloadHandoffResolution =
  | { action: 'discard' }
  | { action: 'restore' }
  | { action: 'record_error_and_restore'; response: Extract<HostToExtensionResponse, { ok: false }> };

const DEFAULT_BROWSER_DOWNLOAD_BYPASS_TTL_MS = 60_000;
const FIREFOX_DOWNLOAD_RESOURCE_TYPES = new Set(['main_frame', 'sub_frame']);
const FIREFOX_DOWNLOAD_MIME_TYPES = new Set([
  'application/octet-stream',
  'application/zip',
  'application/x-zip-compressed',
  'application/vnd.rar',
  'application/x-7z-compressed',
  'application/x-rar-compressed',
  'application/x-tar',
  'application/gzip',
  'application/x-bzip2',
  'application/x-xz',
  'application/zstd',
  'application/pdf',
  'application/java-archive',
  'application/vnd.android.package-archive',
  'application/vnd.ms-cab-compressed',
  'application/vnd.microsoft.portable-executable',
  'application/x-apple-diskimage',
  'application/x-debian-package',
  'application/x-iso9660-image',
  'application/x-msdownload',
  'application/x-msi',
  'application/x-msdos-program',
  'application/x-redhat-package-manager',
]);

export function createBrowserDownloadBypassState(ttlMs = DEFAULT_BROWSER_DOWNLOAD_BYPASS_TTL_MS): BrowserDownloadBypassState {
  return {
    downloadIds: new Map<number, number>(),
    urls: new Map<string, BrowserDownloadBypassUrlEntry>(),
    ttlMs,
  };
}

export function shouldBypassBrowserDownload(
  item: BrowserDownloadReplayItem,
  bypass: BrowserDownloadBypassState,
  now = Date.now(),
): boolean {
  pruneExpiredBypassEntries(bypass, now);

  if (consumeBypassId(bypass, item.id, now)) {
    consumeBypassUrl(bypass, item.finalUrl, now);
    consumeBypassUrl(bypass, item.url, now);
    return true;
  }

  return consumeBypassUrl(bypass, item.finalUrl, now) || consumeBypassUrl(bypass, item.url, now);
}

export function markBrowserDownloadBypassUrl(
  bypass: BrowserDownloadBypassState,
  url: string,
  now = Date.now(),
): void {
  addBypassUrl(bypass, url, now);
}

export function markBrowserDownloadBypassId(
  bypass: BrowserDownloadBypassState,
  downloadId: number,
  now = Date.now(),
): void {
  pruneExpiredBypassEntries(bypass, now);
  bypass.downloadIds.set(downloadId, now + bypass.ttlMs);
}

export function revokeBrowserDownloadBypassUrl(bypass: BrowserDownloadBypassState, url: string): void {
  revokeBypassUrl(bypass, url);
}

export function shouldBypassBrowserDownloadUrl(
  url: string | undefined,
  bypass: BrowserDownloadBypassState,
  now = Date.now(),
): boolean {
  pruneExpiredBypassEntries(bypass, now);
  return consumeBypassUrl(bypass, url, now);
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
    && !isBrowserExtensionPackage(url, item.filename)
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
    ...(details.requestId ? { requestId: details.requestId } : {}),
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
    && (
      response.payload.status === 'queued'
      || response.payload.status === 'duplicate_existing_job'
      || response.payload.status === 'dismissed'
    );
}

export function shouldRestoreBrowserDownloadAfterPromptSwap(response: HostToExtensionResponse): boolean {
  return !isErrorResponse(response)
    && response.type === 'accepted'
    && response.payload.status === 'canceled';
}

export function shouldRestoreBrowserDownloadAfterFailedProtectedHandoff(response: HostToExtensionResponse): boolean {
  return isErrorResponse(response) && response.code === 'PROTECTED_DOWNLOAD_AUTH_REQUIRED';
}

export function classifyBrowserDownloadHandoffResolution(
  response: HostToExtensionResponse,
): BrowserDownloadHandoffResolution {
  if (isErrorResponse(response)) {
    return { action: 'record_error_and_restore', response };
  }

  if (shouldDiscardBrowserDownloadAfterHandoff(response)) {
    return { action: 'discard' };
  }

  return { action: 'restore' };
}

export function createBrowserDownloadHandoffMetadata(
  item: { filename?: string; totalBytes?: number },
  handoffAuth?: HandoffAuth,
): BrowserDownloadHandoffMetadata {
  const suggestedFilename = basenameOnly(item.filename);
  const totalBytes = typeof item.totalBytes === 'number' && Number.isFinite(item.totalBytes) && item.totalBytes > 0
    ? Math.floor(item.totalBytes)
    : undefined;

  return {
    ...(suggestedFilename ? { suggestedFilename } : {}),
    ...(totalBytes ? { totalBytes } : {}),
    ...(handoffAuth ? { handoffAuth } : {}),
  };
}

export async function discardBrowserDownload(
  downloads: BrowserDownloadsCleanupApi,
  downloadId: number,
): Promise<void> {
  await downloads.cancel(downloadId).catch(() => undefined);
  await removeCompletedBrowserDownloadFile(downloads, downloadId);
  await downloads.erase({ id: downloadId }).catch(() => undefined);
}

export async function cancelBrowserDownloadForDesktopPrompt(
  downloads: Pick<BrowserDownloadsCleanupApi, 'cancel'>,
  downloadId: number,
): Promise<void> {
  await downloads.cancel(downloadId).catch(() => undefined);
}

export async function detachBrowserDownloadForDesktopPrompt(
  downloads: BrowserDownloadsCleanupApi,
  downloadId: number,
  releaseFilename: () => void,
): Promise<void> {
  await discardBrowserDownloadBeforeFilenameRelease(downloads, downloadId, releaseFilename);
}

export async function discardBrowserDownloadBeforeFilenameRelease(
  downloads: BrowserDownloadsCleanupApi,
  downloadId: number,
  releaseFilename: () => void,
): Promise<void> {
  await downloads.cancel(downloadId).catch(() => undefined);

  releaseFilename();

  await downloads.cancel(downloadId).catch(() => undefined);
  await removeCompletedBrowserDownloadFile(downloads, downloadId);
  await downloads.erase({ id: downloadId }).catch(() => undefined);
}

export async function restoreBrowserDownloadAfterPromptFallback(
  downloads: BrowserDownloadsCleanupApi & BrowserDownloadsRestartApi,
  item: BrowserDownloadReplayItem,
  bypass: BrowserDownloadBypassState,
  releaseFilename?: () => void,
): Promise<number> {
  releaseFilename?.();
  await discardBrowserDownload(downloads, item.id);
  return restartBrowserDownload(downloads, item, bypass);
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
    markBrowserDownloadBypassId(bypass, downloadId);
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

function addBypassUrl(bypass: BrowserDownloadBypassState, url: string, now = Date.now()): void {
  pruneExpiredBypassEntries(bypass, now);
  const existing = bypass.urls.get(url);
  bypass.urls.set(url, {
    count: (existing?.count ?? 0) + 1,
    expiresAt: now + bypass.ttlMs,
  });
}

function revokeBypassUrl(bypass: BrowserDownloadBypassState, url: string): void {
  const entry = bypass.urls.get(url);
  if (!entry || entry.count <= 1) {
    bypass.urls.delete(url);
    return;
  }

  bypass.urls.set(url, { count: entry.count - 1, expiresAt: entry.expiresAt });
}

function consumeBypassUrl(bypass: BrowserDownloadBypassState, url: string | undefined, now = Date.now()): boolean {
  if (!url) {
    return false;
  }

  const entry = bypass.urls.get(url);
  if (!entry) {
    return false;
  }

  if (entry.expiresAt < now) {
    bypass.urls.delete(url);
    return false;
  }

  revokeBypassUrl(bypass, url);
  return true;
}

function consumeBypassId(bypass: BrowserDownloadBypassState, downloadId: number, now: number): boolean {
  const expiresAt = bypass.downloadIds.get(downloadId);
  if (expiresAt === undefined) {
    return false;
  }

  bypass.downloadIds.delete(downloadId);
  return expiresAt >= now;
}

function pruneExpiredBypassEntries(bypass: BrowserDownloadBypassState, now: number): void {
  for (const [downloadId, expiresAt] of bypass.downloadIds) {
    if (expiresAt < now) {
      bypass.downloadIds.delete(downloadId);
    }
  }

  for (const [url, entry] of bypass.urls) {
    if (entry.expiresAt < now) {
      bypass.urls.delete(url);
    }
  }
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

  const encodedFilename = /(?:^|;)\s*filename\*\s*=\s*([^;]+)/i.exec(value)?.[1];
  if (encodedFilename) {
    return decodeFilenameStar(encodedFilename);
  }

  const quotedFilename = /(?:^|;)\s*filename\s*=\s*"([^"]+)"/i.exec(value)?.[1];
  if (quotedFilename) {
    return basenameOnly(quotedFilename);
  }

  const plainFilename = /(?:^|;)\s*filename\s*=\s*([^;]+)/i.exec(value)?.[1];
  return plainFilename ? basenameOnly(plainFilename) : undefined;
}

function decodeFilenameStar(value: string): string | undefined {
  const cleaned = value.trim().replace(/^"|"$/g, '');
  const match = /^([^']*)'[^']*'(.*)$/.exec(cleaned);
  return decodeFilename(match?.[2] ?? cleaned);
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
  return isUrlHostExcludedByPatterns(url, excludedHosts);
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

function isBrowserExtensionPackage(url: string, filename: string | undefined): boolean {
  return hasPackageExtension(url, filename, 'xpi');
}

function hasPackageExtension(url: string, filename: string | undefined, extension: string): boolean {
  const candidates = [basenameOnly(filename), basenameFromUrl(url)]
    .filter((candidate): candidate is string => Boolean(candidate))
    .map((candidate) => candidate.toLowerCase());

  return candidates.some((candidate) => candidate.endsWith(`.${extension}`));
}
