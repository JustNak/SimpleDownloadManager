import {
  isUrlHostExcludedByPatterns,
  DEFAULT_CAPTURED_FILE_EXTENSIONS,
  type ExtensionIntegrationSettings,
} from '@myapp/protocol';

export type BrowserDownloadFilenameSuggestion = {
  filename?: string;
  conflictAction?: 'uniquify' | 'overwrite' | 'prompt';
};
export type BrowserDownloadFilenameSuggest = (suggestion?: BrowserDownloadFilenameSuggestion) => void;
export type BrowserDownloadState = 'in_progress' | 'interrupted' | 'complete' | string;
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
export type BrowserDownloadPolicyItem = {
  url?: string;
  finalUrl?: string;
  referrer?: string;
  originUrl?: string;
  documentUrl?: string;
  initiator?: string;
  filename?: string;
  mime?: string;
  totalBytes?: number;
  fileSize?: number;
  resourceType?: string;
  byExtensionId?: string;
  byExtensionName?: string;
};
export type BrowserDownloadIntentReason =
  | 'extension_initiated'
  | 'explicit_download_url'
  | 'app_traffic_probe'
  | 'attachment_disposition'
  | 'strong_filename'
  | 'download_mime'
  | 'app_traffic_payload'
  | 'download_mime_without_intent'
  | 'no_download_intent';
export type BrowserDownloadIntentDecision = {
  action: 'capture' | 'ignore';
  reason: BrowserDownloadIntentReason;
};
export type BrowserDownloadDelta = {
  id: number;
  state?: { current?: string };
};
export type CompletedBrowserDownloadItem = {
  state?: string;
  exists?: boolean;
  filename?: string;
  totalBytes?: number;
  fileSize?: number;
  mime?: string;
};
export type AdoptCompletedBrowserDownloadPayload<TSource> = {
  url: string;
  source: TSource;
  localPath: string;
  suggestedFilename?: string;
  totalBytes?: number;
  mimeType?: string;
};
export type CompletedBrowserDownloadAdoptionDependencies<TAdoption, TSource, TResponse> = {
  searchDownloads: (query: { id: number }) => Promise<CompletedBrowserDownloadItem[]>;
  browserDownloadSource: (adoption: TAdoption & { filename: string }) => TSource;
  adoptBrowserDownload: (payload: AdoptCompletedBrowserDownloadPayload<TSource>) => Promise<TResponse>;
  isErrorResponse: (response: TResponse) => boolean;
  onError: (response: TResponse) => Promise<void> | void;
  onSuccess: (response: TResponse) => Promise<void> | void;
};
export type FirefoxWebRequestHeader = {
  name: string;
  value?: string;
};
export type FirefoxWebRequestDownloadDetails = {
  requestId?: string;
  url: string;
  originUrl?: string;
  documentUrl?: string;
  initiator?: string;
  method?: string;
  type?: string;
  statusCode?: number;
  statusLine?: string;
  responseHeaders?: FirefoxWebRequestHeader[];
  incognito?: boolean;
  cookieStoreId?: string;
};
export type FirefoxWebRequestDownloadCandidate = {
  requestId?: string;
  url: string;
  pageUrl?: string;
  referrer?: string;
  filename?: string;
  totalBytes?: number;
  reason: BrowserDownloadIntentDecision['reason'];
  incognito: boolean;
  cookieStoreId?: string;
};

const FIREFOX_DOWNLOAD_RESOURCE_TYPES = new Set(['main_frame', 'sub_frame', 'xmlhttprequest', 'object', 'other']);
const PAGE_INTERNAL_RESOURCE_TYPES = new Set([
  'csp_report',
  'font',
  'image',
  'imageset',
  'media',
  'ping',
  'script',
  'stylesheet',
  'websocket',
  'xmlhttprequest',
]);
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
    && classifyBrowserDownloadIntent({ ...item, url }, settings.capturedFileExtensions).action === 'capture';
}

export function shouldAllowBrowserDownloadBySettings(
  item: BrowserDownloadPolicyItem,
  settings: ExtensionIntegrationSettings,
): boolean {
  const url = browserDownloadUrl(item);
  if (!url) {
    return false;
  }

  return settings.enabled
    && settings.downloadHandoffMode !== 'off'
    && !isAnyHostExcluded(browserDownloadExclusionUrls(item, url), settings.excludedHosts)
    && !isBrowserExtensionPackage(url, item.filename);
}

export function classifyBrowserDownloadIntent(
  item: BrowserDownloadPolicyItem & { url: string; contentDisposition?: string },
  capturedFileExtensions: string[] = [...DEFAULT_CAPTURED_FILE_EXTENSIONS],
): BrowserDownloadIntentDecision {
  if (isDownloadCreatedByExtension(item)) {
    return { action: 'ignore', reason: 'extension_initiated' };
  }

  const url = item.url;
  const contentDisposition = item.contentDisposition;
  const contentType = normalizeContentType(item.mime);
  const filename = item.filename ?? filenameFromContentDisposition(contentDisposition) ?? basenameFromUrl(url);
  const capturedExtensionSet = normalizedCapturedFileExtensions(capturedFileExtensions);
  const hasAttachmentDisposition = /\battachment\b/i.test(contentDisposition ?? '');
  const hasStrongDownloadFilename = !isInlineContentType(contentType)
    && (
      hasStrongDownloadExtension(filename, capturedExtensionSet)
      || hasStrongDownloadExtension(basenameFromUrl(url), capturedExtensionSet)
    );

  if (isExplicitDownloadUrl(url)) {
    return { action: 'capture', reason: 'explicit_download_url' };
  }

  if (isPageInternalResourceType(item.resourceType)) {
    return { action: 'ignore', reason: 'app_traffic_payload' };
  }

  if (isHighConfidenceAppTrafficProbe(item, url, filename, contentType)) {
    return { action: 'ignore', reason: 'app_traffic_probe' };
  }

  if (hasAttachmentDisposition && hasStrongDownloadFilename) {
    return { action: 'capture', reason: 'attachment_disposition' };
  }

  if (hasStrongDownloadFilename) {
    return { action: 'capture', reason: 'strong_filename' };
  }

  if (isLikelyAppTrafficPayload(item, url, filename, contentType)) {
    return { action: 'ignore', reason: 'app_traffic_payload' };
  }

  if (contentType && DOWNLOAD_MIME_TYPES.has(contentType) && isDownloadMimeCaptureCandidate(item)) {
    return { action: 'capture', reason: 'download_mime' };
  }

  if (contentType && DOWNLOAD_MIME_TYPES.has(contentType)) {
    return { action: 'ignore', reason: 'download_mime_without_intent' };
  }

  return { action: 'ignore', reason: 'no_download_intent' };
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

  if (!shouldAllowBrowserDownloadBySettings({
    url,
    filename,
    originUrl: details.originUrl,
    documentUrl: details.documentUrl,
    initiator: details.initiator,
  }, settings)) {
    return null;
  }

  const decision = classifyBrowserDownloadIntent({
    url,
    filename,
    mime: contentType,
    totalBytes: responseTotalBytes,
    contentDisposition,
    resourceType: details.type,
  }, settings.capturedFileExtensions);
  if (decision.action !== 'capture') {
    return null;
  }

  return {
    ...(details.requestId ? { requestId: details.requestId } : {}),
    url,
    ...(details.documentUrl ? { pageUrl: details.documentUrl } : {}),
    ...(details.originUrl ? { referrer: details.originUrl } : {}),
    filename,
    totalBytes,
    reason: decision.reason,
    incognito: details.incognito ?? false,
    ...(details.cookieStoreId ? { cookieStoreId: details.cookieStoreId } : {}),
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

export function browserDownloadFilenameSuggestion(
  item: Pick<BrowserDownloadPolicyItem, 'filename' | 'url'>,
): BrowserDownloadFilenameSuggestion | undefined {
  const filename = basenameOnly(item.filename) ?? basenameFromUrl(item.url ?? '');
  return filename ? { filename, conflictAction: 'uniquify' } : undefined;
}

export async function completeBrowserDownloadAdoption<TAdoption extends { url: string; suggestedFilename?: string; totalBytes?: number }, TSource, TResponse>(
  delta: BrowserDownloadDelta,
  adoptions: Map<number, TAdoption>,
  dependencies: CompletedBrowserDownloadAdoptionDependencies<TAdoption, TSource, TResponse>,
): Promise<boolean> {
  if (delta.state?.current !== 'complete') {
    return false;
  }

  const adoption = adoptions.get(delta.id);
  if (!adoption) {
    return false;
  }

  const items = await dependencies.searchDownloads({ id: delta.id }).catch(() => []);
  const item = items[0];
  if (!item || item.state !== 'complete' || item.exists === false || !item.filename) {
    adoptions.delete(delta.id);
    return true;
  }

  const response = await dependencies.adoptBrowserDownload({
    url: adoption.url,
    source: dependencies.browserDownloadSource({ ...adoption, filename: item.filename }),
    localPath: item.filename,
    suggestedFilename: basenameOnly(item.filename) ?? adoption.suggestedFilename,
    totalBytes: positiveFiniteNumber(item.totalBytes ?? item.fileSize ?? adoption.totalBytes),
    mimeType: item.mime,
  });
  adoptions.delete(delta.id);

  if (dependencies.isErrorResponse(response)) {
    await dependencies.onError(response);
  } else {
    await dependencies.onSuccess(response);
  }

  return true;
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

function isHighConfidenceAppTrafficProbe(
  item: BrowserDownloadPolicyItem,
  url: string,
  filename: string | undefined,
  contentType: string | undefined,
): boolean {
  const payloadFilename = filename ?? basenameFromUrl(url);
  if (!isGenericAppPayloadFilename(payloadFilename)) {
    return false;
  }

  if (isLikelyApplicationTrafficUrl(url)) {
    return true;
  }

  const byteLength = browserDownloadByteLength(item);
  return byteLength !== undefined
    && byteLength <= 1024
    && (
      !contentType
      || isStructuredApiMimeType(contentType)
      || AMBIGUOUS_BINARY_DOWNLOAD_MIME_TYPES.has(contentType)
      || contentType.startsWith('text/')
    );
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

function isPageInternalResourceType(resourceType: string | undefined): boolean {
  return Boolean(resourceType && PAGE_INTERNAL_RESOURCE_TYPES.has(resourceType.toLowerCase()));
}

function isDownloadMimeCaptureCandidate(item: BrowserDownloadPolicyItem): boolean {
  if (isPageInternalResourceType(item.resourceType)) {
    return false;
  }

  const byteLength = browserDownloadByteLength(item);
  return byteLength === undefined || byteLength > 1024;
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

function hasStrongDownloadExtension(filename: string | undefined, capturedFileExtensions: Set<string> = new Set()): boolean {
  const basename = basenameOnly(filename)?.toLowerCase();
  if (!basename) {
    return false;
  }

  const extension = basename.split('.').pop();
  return Boolean(
    extension
    && extension !== basename
    && capturedFileExtensions.has(extension),
  );
}

function normalizedCapturedFileExtensions(values: string[]): Set<string> {
  return new Set(values.map(normalizeFileExtension).filter(Boolean));
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

function isAnyHostExcluded(urls: string[], excludedHosts: string[]): boolean {
  return urls.some((url) => isHostExcluded(url, excludedHosts));
}

function browserDownloadExclusionUrls(item: BrowserDownloadPolicyItem, url: string): string[] {
  return uniqueHttpUrls([
    url,
    item.finalUrl,
    item.url,
    item.referrer,
    item.originUrl,
    item.documentUrl,
    item.initiator,
  ]);
}

function uniqueHttpUrls(values: Array<string | undefined>): string[] {
  const urls: string[] = [];
  const seen = new Set<string>();

  for (const value of values) {
    if (!isHttpUrl(value) || seen.has(value)) {
      continue;
    }

    seen.add(value);
    urls.push(value);
  }

  return urls;
}

function normalizeFileExtension(value: string): string {
  const extension = value.trim().replace(/^\.+/, '').toLowerCase();
  return extension === '7zip' ? '7z' : extension;
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

function isBrowserExtensionPackage(url: string, filename: string | undefined): boolean {
  return hasPackageExtension(url, filename, 'xpi');
}

function hasPackageExtension(url: string, filename: string | undefined, extension: string): boolean {
  const candidates = [basenameOnly(filename), basenameFromUrl(url)]
    .filter((candidate): candidate is string => Boolean(candidate))
    .map((candidate) => candidate.toLowerCase());

  return candidates.some((candidate) => candidate.endsWith(`.${extension}`));
}
