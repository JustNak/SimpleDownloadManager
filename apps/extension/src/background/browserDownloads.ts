import {
  isErrorResponse,
  isUrlHostExcludedByPatterns,
  type BrowserFallback,
  type ExtensionIntegrationSettings,
  type HandoffAuth,
  type HostToExtensionResponse,
  type TransferKind,
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
  mime?: string;
  totalBytes?: number;
  fileSize?: number;
  byExtensionId?: string;
  byExtensionName?: string;
};
export type BrowserDownloadIntentDecision = {
  action: 'capture' | 'ignore';
  reason: string;
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
  statusCode?: number;
  statusLine?: string;
  responseHeaders?: FirefoxWebRequestHeader[];
  incognito?: boolean;
};
export type FirefoxWebRequestDownloadCandidate = {
  requestId?: string;
  url: string;
  filename?: string;
  totalBytes?: number;
  incognito: boolean;
  browserFallback?: BrowserFallback;
};
export type BrowserDownloadHandoffMetadata = {
  suggestedFilename?: string;
  totalBytes?: number;
  handoffAuth?: HandoffAuth;
  transferKind?: TransferKind;
  browserFallback?: BrowserFallback;
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
  | { action: 'record_error'; response: Extract<HostToExtensionResponse, { ok: false }> };

const DEFAULT_BROWSER_DOWNLOAD_BYPASS_TTL_MS = 60_000;
const FIREFOX_DOWNLOAD_RESOURCE_TYPES = new Set(['main_frame', 'sub_frame', 'xmlhttprequest', 'object', 'other']);
const AMBIGUOUS_BINARY_DOWNLOAD_MIME_TYPES = new Set([
  'application/octet-stream',
]);
const STRUCTURED_API_MIME_TYPES = new Set([
  'application/json',
  'text/json',
  'application/x-protobuf',
  'application/protobuf',
]);
const GENERIC_APP_PAYLOAD_BASENAMES = new Set([
  'config',
  'data',
  'download',
  'file',
  'json',
  'manifest',
  'metadata',
  'ping',
  'player',
  'response',
  'session',
  'verify',
  'verify_session',
  'version',
]);
const GENERIC_APP_PAYLOAD_TEXT_FILENAMES = new Set([
  'data.txt',
  'download.txt',
  'file.txt',
  'json.txt',
  'response.txt',
]);
const DOWNLOAD_MIME_TYPES = new Set([
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
const STRONG_DOWNLOAD_EXTENSIONS = new Set([
  '7z',
  'apk',
  'bz2',
  'cab',
  'csv',
  'deb',
  'dmg',
  'doc',
  'docx',
  'exe',
  'gz',
  'iso',
  'jar',
  'msi',
  'pdf',
  'ppt',
  'pptx',
  'rar',
  'rpm',
  'tar',
  'tgz',
  'torrent',
  'txz',
  'xls',
  'xlsx',
  'xz',
  'zip',
  'zst',
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

  return shouldAllowBrowserDownloadBySettings(item, settings)
    && classifyBrowserDownloadIntent({ ...item, url }).action === 'capture';
}

export function shouldAllowBrowserDownloadBySettings(
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

export function classifyBrowserDownloadIntent(
  item: BrowserDownloadPolicyItem & { url: string; contentDisposition?: string },
): BrowserDownloadIntentDecision {
  if (isDownloadCreatedByExtension(item)) {
    return { action: 'ignore', reason: 'extension_initiated' };
  }

  const url = item.url;
  const contentDisposition = item.contentDisposition;
  const contentType = normalizeContentType(item.mime);
  const filename = item.filename ?? filenameFromContentDisposition(contentDisposition) ?? basenameFromUrl(url);
  const hasAttachmentDisposition = /\battachment\b/i.test(contentDisposition ?? '');
  const hasStrongDownloadFilename = !isInlineContentType(contentType)
    && (hasStrongDownloadExtension(filename) || hasStrongDownloadExtension(basenameFromUrl(url)));

  if (hasAttachmentDisposition) {
    return { action: 'capture', reason: 'attachment_disposition' };
  }

  if (hasStrongDownloadFilename) {
    return { action: 'capture', reason: 'strong_filename' };
  }

  if (isExplicitDownloadUrl(url)) {
    return { action: 'capture', reason: 'explicit_download_url' };
  }

  if (isLikelyAppTrafficPayload(item, url, filename, contentType)) {
    return { action: 'ignore', reason: 'app_traffic_payload' };
  }

  if (contentType && DOWNLOAD_MIME_TYPES.has(contentType)) {
    return { action: 'ignore', reason: 'download_mime_without_intent' };
  }

  return { action: 'ignore', reason: 'no_download_intent' };
}

export function browserDownloadTransferKind(item: BrowserDownloadPolicyItem): TransferKind | undefined {
  const url = browserDownloadUrl(item) ?? item.finalUrl ?? item.url;
  return isTorrentUrl(url) || isTorrentFilename(item.filename) ? 'torrent' : undefined;
}

export function firefoxWebRequestDownloadCandidate(
  details: FirefoxWebRequestDownloadDetails,
  settings: ExtensionIntegrationSettings,
): FirefoxWebRequestDownloadCandidate | null {
  if (details.type && !FIREFOX_DOWNLOAD_RESOURCE_TYPES.has(details.type)) {
    return null;
  }

  if (isRedirectResponse(details)) {
    return null;
  }

  const url = browserDownloadUrl({ url: details.url });
  if (!url) {
    return null;
  }

  const contentDisposition = headerValue(details.responseHeaders, 'content-disposition');
  const contentType = normalizeContentType(headerValue(details.responseHeaders, 'content-type'));
  const filename = filenameFromContentDisposition(contentDisposition) ?? basenameFromUrl(url);
  const responseTotalBytes = nonNegativeIntegerHeader(details.responseHeaders, 'content-length');
  const totalBytes = positiveFiniteNumber(responseTotalBytes);

  if (!shouldAllowBrowserDownloadBySettings({ url, filename }, settings)) {
    return null;
  }

  if (classifyBrowserDownloadIntent({
    url,
    filename,
    mime: contentType,
    totalBytes: responseTotalBytes,
    contentDisposition,
  }).action !== 'capture') {
    return null;
  }

  return {
    ...(details.requestId ? { requestId: details.requestId } : {}),
    url,
    filename,
    totalBytes,
    incognito: details.incognito ?? false,
    ...(isReplayableRequestMethod(details.method) ? {} : { browserFallback: 'unavailable' as const }),
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

export function classifyBrowserDownloadHandoffResolution(
  response: HostToExtensionResponse,
): BrowserDownloadHandoffResolution {
  if (isErrorResponse(response)) {
    return { action: 'record_error', response };
  }

  if (shouldRestoreBrowserDownloadAfterPromptSwap(response)) {
    return { action: 'restore' };
  }

  return { action: 'discard' };
}

export function createBrowserDownloadHandoffMetadata(
  item: { filename?: string; totalBytes?: number; browserFallback?: BrowserFallback },
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
    ...(item.browserFallback === 'unavailable' ? { browserFallback: item.browserFallback } : {}),
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

function isInlineContentType(contentType: string | undefined): boolean {
  return contentType === 'text/html' || contentType === 'application/json';
}

function isRedirectResponse(details: FirefoxWebRequestDownloadDetails): boolean {
  if (details.statusCode !== undefined) {
    return details.statusCode >= 300 && details.statusCode < 400;
  }

  return /^HTTP\/\S+\s+3\d\d\b/i.test(details.statusLine ?? '');
}

function isReplayableRequestMethod(method: string | undefined): boolean {
  return !method || method.toUpperCase() === 'GET';
}

function isDownloadCreatedByExtension(item: BrowserDownloadPolicyItem): boolean {
  return Boolean(item.byExtensionId || item.byExtensionName);
}

function isLikelyAppTrafficPayload(
  item: BrowserDownloadPolicyItem,
  url: string,
  filename: string | undefined,
  contentType: string | undefined,
): boolean {
  if (contentType && isStructuredApiMimeType(contentType)) {
    return true;
  }

  if (contentType && AMBIGUOUS_BINARY_DOWNLOAD_MIME_TYPES.has(contentType)) {
    return true;
  }

  return isLikelyApplicationTrafficUrl(url) || isTinyGenericPayload(item, url, filename);
}

function isStructuredApiMimeType(contentType: string): boolean {
  return STRUCTURED_API_MIME_TYPES.has(contentType) || contentType.endsWith('+json');
}

function isTinyGenericPayload(
  item: BrowserDownloadPolicyItem,
  url: string,
  filename: string | undefined,
): boolean {
  const byteLength = browserDownloadByteLength(item);
  return byteLength !== undefined
    && byteLength <= 1024
    && isGenericAppPayloadFilename(filename ?? basenameFromUrl(url));
}

function isGenericAppPayloadFilename(filename: string | undefined): boolean {
  const basename = basenameOnly(filename)?.toLowerCase();
  if (!basename || hasStrongDownloadExtension(basename)) {
    return false;
  }

  if (GENERIC_APP_PAYLOAD_TEXT_FILENAMES.has(basename)) {
    return true;
  }

  const stem = basename.replace(/\.[^.]+$/, '');
  return GENERIC_APP_PAYLOAD_BASENAMES.has(stem);
}

function isLikelyApplicationTrafficUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    const pathname = parsed.pathname.toLowerCase();
    const basename = basenameOnly(pathname) ?? '';
    return pathname.includes('/api/')
      || pathname.includes('/ajax/')
      || pathname.includes('/graphql')
      || pathname.includes('/rpc/')
      || pathname.endsWith('.json')
      || parsed.searchParams.has('prettyPrint')
      || /(?:^|[-_])(config|heartbeat|manifest|metadata|ping|player|session|verify|version)(?:[-_]|$)/.test(basename);
  } catch {
    return false;
  }
}

function isExplicitDownloadUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    const pathname = parsed.pathname.toLowerCase();
    return parsed.searchParams.get('download_frd') === '1'
      || (/\/files\/\d+\/download\/?$/.test(pathname) && parsed.searchParams.has('verifier'));
  } catch {
    return false;
  }
}

function hasStrongDownloadExtension(filename: string | undefined): boolean {
  const basename = basenameOnly(filename)?.toLowerCase();
  if (!basename) {
    return false;
  }

  const extension = basename.split('.').pop();
  return Boolean(extension && extension !== basename && STRONG_DOWNLOAD_EXTENSIONS.has(extension));
}

function nonNegativeIntegerHeader(headers: FirefoxWebRequestHeader[] | undefined, name: string): number | undefined {
  const value = headerValue(headers, name);
  if (!value) {
    return undefined;
  }

  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : undefined;
}

function browserDownloadByteLength(item: BrowserDownloadPolicyItem): number | undefined {
  return nonNegativeFiniteNumber(item.totalBytes) ?? nonNegativeFiniteNumber(item.fileSize);
}

function nonNegativeFiniteNumber(value: number | undefined): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? Math.floor(value) : undefined;
}

function positiveFiniteNumber(value: number | undefined): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.floor(value) : undefined;
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
