export type DownloadMode = 'single' | 'torrent' | 'multi' | 'bulk';

export const batchUrlTextAreaWrap = 'off';
export const batchUrlTextAreaClassName =
  'w-full resize-none overflow-x-auto whitespace-pre rounded-md border border-input bg-background px-3 py-2.5 font-mono text-[13px] leading-5 text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20';

export function parseDownloadUrlLines(value: string) {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

export function ensureTrailingEditableLine(value: string) {
  const normalized = value.replace(/\r\n/g, '\n');
  if (!normalized.trim()) return '';
  return normalized.endsWith('\n') ? normalized : `${normalized}\n`;
}

export function downloadSubmitLabel(mode: DownloadMode, linkCount: number, combineBulk: boolean) {
  if (mode === 'single') return 'Start Download';
  if (mode === 'torrent') return 'Add Torrent';

  if (mode === 'bulk' && combineBulk) {
    return linkCount > 0
      ? `Queue ${downloadCountLabel(linkCount)} and Combine`
      : 'Queue and Combine';
  }

  return linkCount > 0 ? `Queue ${downloadCountLabel(linkCount)}` : 'Queue Downloads';
}

function downloadCountLabel(count: number) {
  return `${count} ${count === 1 ? 'Download' : 'Downloads'}`;
}
