export interface BrowserDownloadLike {
  url?: string;
  filename?: string;
}

export function shouldHandOffTorrentBrowserDownload(item: BrowserDownloadLike): boolean {
  return isTorrentUrl(item.url) || isTorrentFilename(item.filename);
}

export function isTorrentUrl(url: string | undefined): boolean {
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

export function isTorrentFilename(filename: string | undefined): boolean {
  if (!filename) return false;
  return filename.replaceAll('\\', '/').split('/').pop()?.toLowerCase().endsWith('.torrent') ?? false;
}
