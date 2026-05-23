import {
  createBrowserBlobBeginRequest as createProtocolBrowserBlobBeginRequest,
  createBrowserBlobCancelRequest as createProtocolBrowserBlobCancelRequest,
  createBrowserBlobChunkRequest as createProtocolBrowserBlobChunkRequest,
  createBrowserBlobFinishRequest as createProtocolBrowserBlobFinishRequest,
  isUrlHostExcludedByPatterns,
  type BrowserBlobDownloadBeginPayload,
  type ExtensionIntegrationSettings,
} from '@myapp/protocol';

export type BrowserBlobDownloadCandidate = {
  blobUrl: string;
  pageUrl?: string;
  filename?: string;
  mimeType?: string;
};

export const BROWSER_BLOB_DOWNLOAD_PORT = 'browser_blob_download';
export const BLOB_DOWNLOAD_INTERCEPT_EVENT = 'simple-download-manager:blob-download';
export const BLOB_DOWNLOAD_PAGE_MESSAGE_SOURCE = 'simple-download-manager-blob-download';
export const BLOB_DOWNLOAD_BYPASS_ATTRIBUTE = 'data-simple-download-manager-blob-bypass';
export const BROWSER_BLOB_CHUNK_SIZE_BYTES = 256 * 1024;

const MIME_EXTENSION_MAP = new Map<string, string>([
  ['application/gzip', 'gz'],
  ['application/java-archive', 'jar'],
  ['application/json', 'json'],
  ['application/octet-stream', 'bin'],
  ['application/pdf', 'pdf'],
  ['application/vnd.android.package-archive', 'apk'],
  ['application/vnd.ms-cab-compressed', 'cab'],
  ['application/vnd.microsoft.portable-executable', 'exe'],
  ['application/vnd.rar', 'rar'],
  ['application/x-7z-compressed', '7z'],
  ['application/x-apple-diskimage', 'dmg'],
  ['application/x-bzip2', 'bz2'],
  ['application/x-debian-package', 'deb'],
  ['application/x-msdownload', 'exe'],
  ['application/x-msi', 'msi'],
  ['application/x-rar-compressed', 'rar'],
  ['application/x-tar', 'tar'],
  ['application/x-xz', 'xz'],
  ['application/zip', 'zip'],
  ['audio/aac', 'aac'],
  ['audio/flac', 'flac'],
  ['audio/mpeg', 'mp3'],
  ['audio/ogg', 'ogg'],
  ['audio/wav', 'wav'],
  ['image/gif', 'gif'],
  ['image/jpeg', 'jpg'],
  ['image/png', 'png'],
  ['image/webp', 'webp'],
  ['text/csv', 'csv'],
  ['text/plain', 'txt'],
  ['video/mp4', 'mp4'],
  ['video/mpeg', 'mpeg'],
  ['video/quicktime', 'mov'],
  ['video/webm', 'webm'],
  ['video/x-matroska', 'mkv'],
]);

export function isBlobDownloadHref(href: string | undefined): href is string {
  if (!href) return false;
  try {
    return new URL(href).protocol === 'blob:';
  } catch {
    return href.trim().toLowerCase().startsWith('blob:');
  }
}

export function shouldHandleBlobDownload(
  candidate: BrowserBlobDownloadCandidate,
  settings: ExtensionIntegrationSettings,
): boolean {
  if (!settings.enabled || settings.downloadHandoffMode === 'off') {
    return false;
  }

  if (!isBlobDownloadHref(candidate.blobUrl)) {
    return false;
  }

  const pageUrl = candidate.pageUrl ?? pageUrlFromBlobUrl(candidate.blobUrl);
  if (pageUrl && isUrlHostExcludedByPatterns(pageUrl, settings.excludedHosts)) {
    return false;
  }

  return !isFileExtensionIgnored(
    candidate.filename ?? blobDownloadFilename(candidate.filename, candidate.mimeType),
    settings.ignoredFileExtensions,
  );
}

export function blobDownloadFilename(filename?: string, mimeType?: string): string {
  const basename = basenameOnly(filename);
  if (basename) {
    return basename;
  }

  const extension = extensionForMimeType(mimeType) ?? 'bin';
  return `download.${extension}`;
}

export function createBrowserBlobBeginRequest(
  payload: BrowserBlobDownloadBeginPayload & { filename?: string },
) {
  const suggestedFilename = payload.suggestedFilename ?? payload.filename;
  return createProtocolBrowserBlobBeginRequest({
    ...payload,
    suggestedFilename: blobDownloadFilename(suggestedFilename, payload.mimeType),
  });
}

export function createBrowserBlobChunkRequest(
  streamId: string,
  offset: number,
  chunk: Uint8Array,
) {
  return createProtocolBrowserBlobChunkRequest(streamId, offset, bytesToBase64(chunk));
}

export function createBrowserBlobFinishRequest(streamId: string) {
  return createProtocolBrowserBlobFinishRequest(streamId);
}

export function createBrowserBlobCancelRequest(streamId: string, reason?: string) {
  return createProtocolBrowserBlobCancelRequest(streamId, reason);
}

export function createBrowserBlobStreamId(): string {
  const cryptoApi = globalThis.crypto;
  if (cryptoApi?.randomUUID) {
    return `blob:${cryptoApi.randomUUID()}`;
  }

  return `blob:${Date.now().toString(36)}:${Math.random().toString(36).slice(2)}`;
}

function pageUrlFromBlobUrl(blobUrl: string): string | undefined {
  if (!blobUrl.toLowerCase().startsWith('blob:')) {
    return undefined;
  }

  const inner = blobUrl.slice(5);
  try {
    const parsed = new URL(inner);
    return parsed.origin === 'null' ? undefined : parsed.toString();
  } catch {
    return undefined;
  }
}

function extensionForMimeType(mimeType: string | undefined): string | undefined {
  const normalized = mimeType?.split(';', 1)[0]?.trim().toLowerCase();
  if (!normalized) {
    return undefined;
  }

  if (MIME_EXTENSION_MAP.has(normalized)) {
    return MIME_EXTENSION_MAP.get(normalized);
  }

  if (normalized.startsWith('text/')) {
    return 'txt';
  }

  return undefined;
}

function isFileExtensionIgnored(filename: string, ignoredExtensions: string[] = []): boolean {
  const normalizedFilename = filename.toLowerCase();
  return ignoredExtensions
    .map((extension) => extension.trim().replace(/^\.+/, '').toLowerCase())
    .filter(Boolean)
    .some((extension) => normalizedFilename.endsWith(`.${extension}`));
}

function basenameOnly(path: string | undefined): string | undefined {
  if (!path) return undefined;
  const normalized = path.replaceAll('\\', '/').trim();
  const basename = normalized.split('/').filter(Boolean).pop()?.trim();
  return basename && basename !== '.' && basename !== '..' ? basename : undefined;
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let index = 0; index < bytes.length; index += 1) {
    binary += String.fromCharCode(bytes[index] ?? 0);
  }

  return btoa(binary);
}
