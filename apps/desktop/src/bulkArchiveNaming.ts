const unsafeArchiveNameChars = /[<>:"/\\|?*\u0000-\u001F]/g;

export type BulkOutputKind = 'folder';

export type MultipartArchiveSuffix = 'partRar' | 'numbered' | 'legacyRar';

export interface MultipartArchivePart {
  key: string;
  displayPrefix: string;
  suffix: MultipartArchiveSuffix;
  partNumber: number;
  numberWidth: number;
}

export function normalizeFolderName(value: string) {
  const sanitized = value.replace(unsafeArchiveNameChars, '').trim().replace(/^\.+|\.+$/g, '');
  return sanitized || 'bulk-download';
}

export function normalizeBulkOutputName(value: string, outputKind: BulkOutputKind = 'folder') {
  void outputKind;
  return normalizeFolderName(value);
}

export function defaultBulkArchiveNameForUrls(urls: string[], fallback = 'bulk-download') {
  return defaultBulkOutputNameForUrls(urls, 'folder', fallback);
}

export function defaultBulkOutputNameForUrls(
  urls: string[],
  outputKind: BulkOutputKind = 'folder',
  fallback = 'bulk-download',
) {
  void outputKind;
  for (const url of urls) {
    const filename = filenameCandidateFromUrl(url);
    const archiveStem = filename ? multipartArchiveStem(filename) : null;
    if (archiveStem) return normalizeBulkOutputName(archiveStem);
  }

  return fallback;
}

function multipartArchiveStem(filename: string): string | null {
  return detectMultipartArchivePart(filename)?.displayPrefix ?? null;
}

export function detectMultipartArchivePart(name: string): MultipartArchivePart | null {
  const normalized = name.replace(/\\/g, '/');
  const fileName = normalized.split('/').filter(Boolean).pop()?.trim() ?? '';
  if (!fileName || fileName === '.' || fileName === '..') return null;

  const lower = fileName.toLowerCase();
  if (lower.endsWith('.rar')) {
    const withoutRar = fileName.slice(0, -4);
    const lowerWithoutRar = lower.slice(0, -4);
    const partIndex = lowerWithoutRar.lastIndexOf('.part');
    if (partIndex >= 0) {
      const numberText = lowerWithoutRar.slice(partIndex + 5);
      if (/^\d+$/.test(numberText)) {
        const partNumber = Number.parseInt(numberText, 10);
        if (!Number.isSafeInteger(partNumber)) return null;
        const displayPrefix = fileName.slice(0, partIndex);
        return {
          key: `part-rar:${displayPrefix.toLowerCase()}`,
          displayPrefix,
          suffix: 'partRar',
          partNumber,
          numberWidth: numberText.length,
        };
      }
    }

    return {
      key: `legacy-rar:${withoutRar.toLowerCase()}`,
      displayPrefix: withoutRar,
      suffix: 'legacyRar',
      partNumber: 1,
      numberWidth: 1,
    };
  }

  const dotIndex = fileName.lastIndexOf('.');
  if (dotIndex < 0) return null;
  const extension = fileName.slice(dotIndex + 1);
  const lowerExtension = extension.toLowerCase();
  if (/^r\d{2}$/.test(lowerExtension)) {
    const partIndex = Number.parseInt(lowerExtension.slice(1), 10);
    if (!Number.isSafeInteger(partIndex)) return null;
    const displayPrefix = fileName.slice(0, dotIndex);
    return {
      key: `legacy-rar:${displayPrefix.toLowerCase()}`,
      displayPrefix,
      suffix: 'legacyRar',
      partNumber: partIndex + 2,
      numberWidth: 2,
    };
  }

  if (/^\d{3}$/.test(extension)) {
    const partNumber = Number.parseInt(extension, 10);
    if (!Number.isSafeInteger(partNumber) || partNumber === 0) return null;
    const displayPrefix = fileName.slice(0, dotIndex);
    return {
      key: `numbered:${displayPrefix.toLowerCase()}`,
      displayPrefix,
      suffix: 'numbered',
      partNumber,
      numberWidth: extension.length,
    };
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
