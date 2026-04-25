import type { DownloadJob } from './types';

export function shouldRevealJobDirectoryOnDoubleClick(job: DownloadJob, button: number): boolean {
  return button === 0 && Boolean(job.targetPath?.trim());
}
