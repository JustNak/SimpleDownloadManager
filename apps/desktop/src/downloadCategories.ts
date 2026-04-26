export type DownloadCategory =
  | 'document'
  | 'program'
  | 'picture'
  | 'video'
  | 'compressed'
  | 'music'
  | 'other';

export interface DownloadCategoryDefinition {
  id: DownloadCategory;
  label: string;
  folderName: string;
  extensions: readonly string[];
}

export const DOWNLOAD_CATEGORIES: readonly DownloadCategoryDefinition[] = [
  {
    id: 'document',
    label: 'Document',
    folderName: 'Document',
    extensions: ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'txt', 'rtf', 'csv', 'md', 'epub'],
  },
  {
    id: 'program',
    label: 'Program',
    folderName: 'Program',
    extensions: ['exe', 'msi', 'apk', 'dmg', 'pkg', 'deb', 'rpm', 'appimage'],
  },
  {
    id: 'picture',
    label: 'Picture',
    folderName: 'Picture',
    extensions: ['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'svg', 'tif', 'tiff', 'heic'],
  },
  {
    id: 'video',
    label: 'Video',
    folderName: 'Video',
    extensions: ['mp4', 'mkv', 'avi', 'mov', 'webm', 'm4v', 'wmv', 'flv'],
  },
  {
    id: 'compressed',
    label: 'Compressed',
    folderName: 'Compressed',
    extensions: ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'tgz'],
  },
  {
    id: 'music',
    label: 'Music',
    folderName: 'Music',
    extensions: ['mp3', 'wav', 'flac', 'ogg', 'm4a', 'aac', 'opus', 'wma'],
  },
  {
    id: 'other',
    label: 'Other',
    folderName: 'Other',
    extensions: [],
  },
] as const;

const CATEGORY_BY_EXTENSION = new Map<string, DownloadCategory>(
  DOWNLOAD_CATEGORIES.flatMap((category) => (
    category.extensions.map((extension) => [extension, category.id] as const)
  )),
);

export function categoryForFilename(filename: string): DownloadCategory {
  const extension = extensionFromFilename(filename);
  if (!extension) return 'other';
  return CATEGORY_BY_EXTENSION.get(extension) ?? 'other';
}

export function categoryFolderForFilename(filename: string): string {
  const category = categoryForFilename(filename);
  return DOWNLOAD_CATEGORIES.find((definition) => definition.id === category)?.folderName ?? 'Other';
}

export function countJobsByCategory<T extends { filename: string }>(jobs: readonly T[]): Record<DownloadCategory, number> {
  const counts = Object.fromEntries(
    DOWNLOAD_CATEGORIES.map((category) => [category.id, 0]),
  ) as Record<DownloadCategory, number>;

  for (const job of jobs) {
    counts[categoryForFilename(job.filename)] += 1;
  }

  return counts;
}

export function filterJobsByCategory<T extends { filename: string }>(
  jobs: readonly T[],
  category: DownloadCategory,
): T[] {
  return jobs.filter((job) => categoryForFilename(job.filename) === category);
}

function extensionFromFilename(filename: string): string {
  const basename = filename.trim().split(/[\\/]/).pop() ?? '';
  const dotIndex = basename.lastIndexOf('.');
  if (dotIndex <= 0 || dotIndex === basename.length - 1) return '';
  return basename.slice(dotIndex + 1).toLowerCase();
}
