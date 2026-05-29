import { DEFAULT_CAPTURED_FILE_EXTENSIONS } from './defaultSettings.ts';

export function normalizeCapturedExtensionInput(value: string): string {
  const extension = normalizeExtensionAlias(value.trim().replace(/^\.+/, '').toLowerCase());
  if (
    !extension
    || extension.includes('/')
    || extension.includes('\\')
    || /\s/.test(extension)
    || /^\.+$/.test(extension)
  ) {
    return '';
  }

  return extension;
}

export function parseCapturedExtensionInput(value: string): string[] {
  return Array.from(
    new Set(
      value
        .split(/[,\s]+/)
        .map(normalizeCapturedExtensionInput)
        .filter(Boolean),
    ),
  );
}

export function addCapturedExtensions(
  existingExtensions: string[],
  candidates: string[],
): { extensions: string[]; addedExtensions: string[]; duplicateExtensions: string[] } {
  const extensions = [...existingExtensions];
  const addedExtensions: string[] = [];
  const duplicateExtensions: string[] = [];

  for (const candidate of candidates.map(normalizeCapturedExtensionInput).filter(Boolean)) {
    if (extensions.includes(candidate)) {
      duplicateExtensions.push(candidate);
      continue;
    }

    extensions.push(candidate);
    addedExtensions.push(candidate);
  }

  return { extensions, addedExtensions, duplicateExtensions };
}

export function removeCapturedExtension(existingExtensions: string[], extension: string): string[] {
  const normalized = normalizeCapturedExtensionInput(extension);
  return existingExtensions.filter((candidate) => candidate !== normalized);
}

export function filterCapturedExtensions(extensions: string[], query: string): string[] {
  const normalizedQuery = query.trim().replace(/^\.+/, '').toLowerCase();
  if (!normalizedQuery) return extensions;
  return extensions.filter((extension) => extension.includes(normalizedQuery));
}

export function formatCapturedExtensionsSummary(extensions: string[]): string {
  if (extensions.length === 0) return 'No captured extensions';
  if (extensions.length === DEFAULT_CAPTURED_FILE_EXTENSIONS.length && isDefaultCapturedExtensionSet(extensions)) {
    return `${extensions.length} default extensions`;
  }
  return extensions.length === 1 ? '1 captured extension' : `${extensions.length} captured extensions`;
}

function isDefaultCapturedExtensionSet(extensions: string[]): boolean {
  const extensionSet = new Set(extensions);
  return DEFAULT_CAPTURED_FILE_EXTENSIONS.every((extension) => extensionSet.has(extension));
}

function normalizeExtensionAlias(value: string): string {
  return value === '7zip' ? '7z' : value;
}
