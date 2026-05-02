import type { TransferKind } from './types';

export function formatBytes(bytes: number | undefined): string {
  const value = Math.max(0, bytes ?? 0);
  if (value === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  const amount = value / 1024 ** index;
  return `${amount >= 10 || index === 0 ? amount.toFixed(0) : amount.toFixed(1)} ${units[index]}`;
}

export function formatTime(seconds: number | undefined): string {
  const value = Math.max(0, Math.floor(seconds ?? 0));
  if (value <= 0) return '--';
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  const remainingSeconds = value % 60;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${remainingSeconds}s`;
  return `${remainingSeconds}s`;
}

export function getHost(url: string): string {
  try {
    const parsed = new URL(url);
    if (parsed.protocol === 'magnet:') return 'Magnet link';
    return parsed.host || url;
  } catch {
    return url;
  }
}

export function joinDisplayPath(directory: string, filename: string): string {
  if (!directory) return filename;
  return `${directory.replace(/[\\/]+$/, '')}\\${filename.replace(/^[\\/]+/, '')}`;
}

export function fileExtension(filename: string): string {
  const basename = filename.split(/[\\/]/).pop() ?? filename;
  const dotIndex = basename.lastIndexOf('.');
  return dotIndex > 0 && dotIndex < basename.length - 1 ? basename.slice(dotIndex + 1).toUpperCase() : 'FILE';
}

export function transferLabel(transferKind?: TransferKind): string {
  return transferKind === 'torrent' ? 'TOR' : 'HTTP';
}
