import type { DownloadJob } from './types';

export type UpdateCheckMode = 'startup' | 'manual';
export type AppUpdateStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'not_available'
  | 'downloading'
  | 'installing'
  | 'error';

export interface AppUpdateMetadata {
  version: string;
  currentVersion: string;
  date?: string;
  body?: string;
}

export type UpdateInstallProgressEvent =
  | { event: 'started'; data: { contentLength?: number | null } }
  | { event: 'progress'; data: { chunkLength: number } }
  | { event: 'finished' };

export interface AppUpdateState {
  status: AppUpdateStatus;
  availableUpdate: AppUpdateMetadata | null;
  lastCheckMode: UpdateCheckMode | null;
  errorMessage: string | null;
  downloadedBytes: number;
  totalBytes: number | null;
}

export type AppUpdateVersionTone = 'available' | 'current' | 'pending' | 'error';

export interface AppUpdateVersionIndicator {
  currentVersion: string;
  newVersion: string;
  newVersionTone: AppUpdateVersionTone;
}

export type BulkUpdateBlockerKind = 'bulk_download' | 'bulk_archive';

export interface BulkUpdateBlocker {
  kind: BulkUpdateBlockerKind;
  archiveId: string;
  message: string;
}

export const initialAppUpdateState: AppUpdateState = {
  status: 'idle',
  availableUpdate: null,
  lastCheckMode: null,
  errorMessage: null,
  downloadedBytes: 0,
  totalBytes: null,
};

const activeBulkMemberStates = new Set<string>(['queued', 'starting', 'downloading']);
const finalizingBulkArchiveStatuses = new Set<string>([
  'extracting',
  'combining',
  'creating_folder',
  'compressing',
]);

export function bulkUpdateBlockerForJobs(jobs: readonly DownloadJob[]): BulkUpdateBlocker | null {
  for (const job of jobs) {
    const archive = job.transferKind === 'http' ? job.bulkArchive : undefined;
    if (!archive?.id) continue;
    if (activeBulkMemberStates.has(job.state)) {
      return {
        kind: 'bulk_download',
        archiveId: archive.id,
        message: 'Finish or pause active bulk downloads before installing the update.',
      };
    }
  }

  for (const job of jobs) {
    const archive = job.transferKind === 'http' ? job.bulkArchive : undefined;
    if (!archive?.id || !archive.archiveStatus) continue;
    if (finalizingBulkArchiveStatuses.has(archive.archiveStatus)) {
      return {
        kind: 'bulk_archive',
        archiveId: archive.id,
        message: 'Wait for bulk archive finalization to finish before installing the update.',
      };
    }
  }

  return null;
}

export function shouldRunStartupUpdateCheck(
  hasChecked: boolean,
  blocker: BulkUpdateBlocker | null = null,
): boolean {
  return !hasChecked && !blocker;
}

export function shouldOpenUpdatePrompt(blocker: BulkUpdateBlocker | null): blocker is null {
  return !blocker;
}

export function shouldNotifyUpdateCheckFailure(mode: UpdateCheckMode): boolean {
  return mode === 'manual';
}

export function updateVersionIndicator(
  state: AppUpdateState,
  installedVersion: string,
): AppUpdateVersionIndicator {
  if (state.availableUpdate) {
    return {
      currentVersion: state.availableUpdate.currentVersion || installedVersion,
      newVersion: state.availableUpdate.version,
      newVersionTone: 'available',
    };
  }

  if (state.status === 'not_available') {
    return {
      currentVersion: installedVersion,
      newVersion: installedVersion,
      newVersionTone: 'current',
    };
  }

  if (state.status === 'error') {
    return {
      currentVersion: installedVersion,
      newVersion: 'Unavailable',
      newVersionTone: 'error',
    };
  }

  return {
    currentVersion: installedVersion,
    newVersion: state.status === 'checking' ? 'Checking...' : 'Check pending',
    newVersionTone: 'pending',
  };
}

export function startUpdateCheck(
  state: AppUpdateState,
  mode: UpdateCheckMode,
): AppUpdateState {
  return {
    ...state,
    status: 'checking',
    lastCheckMode: mode,
    errorMessage: null,
  };
}

export function finishUpdateCheck(
  state: AppUpdateState,
  update: AppUpdateMetadata | null,
): AppUpdateState {
  return {
    ...state,
    status: update ? 'available' : 'not_available',
    availableUpdate: update,
    errorMessage: null,
    downloadedBytes: 0,
    totalBytes: null,
  };
}

export function failUpdateCheck(
  state: AppUpdateState,
  message: string,
): AppUpdateState {
  return {
    ...state,
    status: 'error',
    errorMessage: message,
  };
}

export function beginUpdateInstall(state: AppUpdateState): AppUpdateState {
  return {
    ...state,
    status: 'downloading',
    errorMessage: null,
    downloadedBytes: 0,
    totalBytes: null,
  };
}

export function failUpdateInstall(
  state: AppUpdateState,
  message: string,
): AppUpdateState {
  return {
    ...state,
    status: 'error',
    errorMessage: message,
  };
}

export function applyInstallProgressEvent(
  state: AppUpdateState,
  event: UpdateInstallProgressEvent,
): AppUpdateState {
  if (event.event === 'started') {
    return {
      ...state,
      status: 'downloading',
      downloadedBytes: 0,
      totalBytes: event.data.contentLength ?? null,
    };
  }

  if (event.event === 'progress') {
    return {
      ...state,
      status: 'downloading',
      downloadedBytes: state.downloadedBytes + event.data.chunkLength,
    };
  }

  return {
    ...state,
    status: 'installing',
  };
}
