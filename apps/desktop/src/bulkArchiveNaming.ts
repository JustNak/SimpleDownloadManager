const unsafeArchiveNameChars = /[<>:"/\\|?*\u0000-\u001F]/g;

export type BulkOutputKind = 'archive' | 'folder';

const multipartSuffixes = [
  /\.part\d+\.rar$/i,
  /\.rar$/i,
  /\.0*1$/i,
];

export function normalizeArchiveName(value: string) {
  const sanitized = value.replace(unsafeArchiveNameChars, '').trimStart();
  if (!sanitized) return '';
  return sanitized.toLowerCase().endsWith('.zip') ? sanitized : `${sanitized}.zip`;
}

export function normalizeFolderName(value: string) {
  const sanitized = value.replace(unsafeArchiveNameChars, '').trim().replace(/^\.+|\.+$/g, '');
  return sanitized || 'bulk-download';
}

export function normalizeBulkOutputName(value: string, outputKind: BulkOutputKind) {
  return outputKind === 'folder' ? normalizeFolderName(value) : normalizeArchiveName(value);
}

export function stripZipExtension(value: string) {
  return value.toLowerCase().endsWith('.zip') ? value.slice(0, -4) : value;
}

export function defaultBulkArchiveNameForUrls(urls: string[], fallback = 'bulk-download.zip') {
  return defaultBulkOutputNameForUrls(urls, 'archive', fallback);
}

export function defaultBulkOutputNameForUrls(
  urls: string[],
  outputKind: BulkOutputKind,
  fallback = outputKind === 'archive' ? 'bulk-download.zip' : 'bulk-download',
) {
  for (const url of urls) {
    const filename = filenameCandidateFromUrl(url);
    const archiveStem = filename ? multipartArchiveStem(filename) : null;
    if (archiveStem) return normalizeBulkOutputName(archiveStem, outputKind);
  }

  return fallback;
}

function multipartArchiveStem(filename: string): string | null {
  const cleanName = filename.trim();
  if (!cleanName || cleanName === '.' || cleanName === '..') return null;

  for (const suffix of multipartSuffixes) {
    if (!suffix.test(cleanName)) continue;
    const stem = cleanName.replace(suffix, '').trim();
    return stem && stem !== '.' && stem !== '..' ? stem : null;
  }

  return null;
}

function filenameCandidateFromUrl(value: string): string | null {
  try {
    const parsed = new URL(value);
    const fragment = decodeUrlComponent(parsed.hash.startsWith('#') ? parsed.hash.slice(1) : parsed.hash);
    if (fragment) return basename(fragment);

    const pathSegment = parsed.pathname.split('/').filter(Boolean).pop();
    return pathSegment ? basename(decodeUrlComponent(pathSegment)) : null;
  } catch {
    const segment = value.split(/[\\/]/).filter(Boolean).pop();
    return segment ? decodeUrlComponent(segment) : null;
  }
}

function basename(value: string) {
  return value.split(/[\\/]/).filter(Boolean).pop() ?? value;
}

function decodeUrlComponent(value: string) {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}
