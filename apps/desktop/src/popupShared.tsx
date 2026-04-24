import React from 'react';
import {
  Box,
  FileArchive,
  FileAudio,
  FileCode,
  FileImage,
  FileText,
  FileVideo,
} from 'lucide-react';

export function FileBadge({ filename, large = false }: { filename: string; large?: boolean }) {
  const ext = filename.split('.').pop()?.toLowerCase() || '';
  const iconSize = large ? 32 : 22;
  const label = ext ? ext.slice(0, 4).toUpperCase() : 'FILE';

  return (
    <div className={`file-badge relative flex shrink-0 items-center justify-center rounded-sm border border-border bg-background ${large ? 'h-[92px] w-[70px]' : 'h-12 w-10'}`}>
      <div className="absolute right-0 top-0 h-3 w-3 border-b border-l border-border bg-surface" />
      <div className="text-primary">{fileIcon(ext, iconSize)}</div>
      {large ? <div className="absolute bottom-2 text-[10px] font-semibold text-muted-foreground">{label}</div> : null}
    </div>
  );
}

export function formatBytes(bytes: number | undefined, decimals = 1) {
  if (typeof bytes !== 'number' || !Number.isFinite(bytes)) return 'Unknown';
  if (bytes <= 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(decimals))} ${sizes[i]}`;
}

export function formatTime(seconds: number) {
  if (!Number.isFinite(seconds) || seconds <= 0) return '--';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.round(seconds % 60);
  if (minutes < 60) return `${minutes}m ${remainingSeconds}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

export function getHost(rawUrl: string) {
  try {
    return new URL(rawUrl).host;
  } catch {
    return rawUrl;
  }
}

export function joinDisplayPath(directory: string, filename: string) {
  if (!directory) return filename;
  const separator = directory.endsWith('\\') || directory.endsWith('/') ? '' : '\\';
  return `${directory}${separator}${filename}`;
}

function fileIcon(ext: string, size: number) {
  if (['mp4', 'mkv', 'avi', 'mov', 'webm'].includes(ext)) return <FileVideo size={size} />;
  if (['mp3', 'wav', 'flac', 'ogg', 'm4a'].includes(ext)) return <FileAudio size={size} />;
  if (['jpg', 'jpeg', 'png', 'gif', 'webp'].includes(ext)) return <FileImage size={size} />;
  if (['zip', 'rar', '7z', 'tar', 'gz'].includes(ext)) return <FileArchive size={size} />;
  if (['exe', 'msi', 'apk', 'dmg', 'pkg', 'deb'].includes(ext)) return <Box size={size} />;
  if (['js', 'ts', 'json', 'html', 'css'].includes(ext)) return <FileCode size={size} />;
  return <FileText size={size} />;
}
