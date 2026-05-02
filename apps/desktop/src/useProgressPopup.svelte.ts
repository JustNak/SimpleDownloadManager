import { getCurrentWindow } from '@tauri-apps/api/window';
import { applyAppearance } from './appearance';
import {
  cancelJob,
  getProgressJobSnapshot,
  subscribeToProgressJobSnapshot,
} from './backend';
import {
  calculateDownloadProgressMetrics,
  recordProgressSample,
  type DownloadProgressMetrics,
  type ProgressSample,
} from './downloadProgressMetrics';
import { runPopupAction } from './popupActions';
import type { DownloadJob, Settings } from './types';

export type PopupActionRunner = (
  action: () => Promise<void>,
  options?: { closeOnSuccess?: boolean },
) => Promise<void>;

export interface ProgressPopupState {
  readonly job: DownloadJob | null;
  readonly progress: number;
  readonly progressMetrics: DownloadProgressMetrics;
  readonly isBusy: boolean;
  readonly isConfirmingCancel: boolean;
  readonly errorMessage: string;
  runAction: PopupActionRunner;
  onCancelClick: () => void;
  onClose: () => void;
}

export function useProgressPopup(): ProgressPopupState {
  let job = $state<DownloadJob | null>(null);
  let isBusy = $state(false);
  let isConfirmingCancel = $state(false);
  let errorMessage = $state('');
  let progressSamples: ProgressSample[] = [];
  const currentWindow = isTauriRuntime() ? getCurrentWindow() : null;
  const jobId = new URLSearchParams(window.location.search).get('jobId') || '';

  $effect(() => {
    let dispose: (() => void | Promise<void>) | undefined;
    let latestSettings: Settings | null = null;
    let disposed = false;

    const applySnapshotAppearance = (snapshot: Awaited<ReturnType<typeof getProgressJobSnapshot>>) => {
      latestSettings = snapshot.settings;
      applyAppearance(snapshot.settings);
    };
    const applySnapshotJob = (snapshot: Awaited<ReturnType<typeof getProgressJobSnapshot>>) => {
      const nextJob = snapshot.job;
      if (nextJob) {
        progressSamples = recordProgressSample(progressSamples, nextJob);
      }
      job = nextJob;
    };

    const media = typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-color-scheme: dark)') : null;
    const handleSystemThemeChange = () => {
      if (latestSettings) applyAppearance(latestSettings);
    };
    media?.addEventListener('change', handleSystemThemeChange);

    async function initialize() {
      const snapshot = await getProgressJobSnapshot(jobId);
      if (disposed) return;
      applySnapshotAppearance(snapshot);
      applySnapshotJob(snapshot);
      dispose = await subscribeToProgressJobSnapshot((nextSnapshot) => {
        applySnapshotAppearance(nextSnapshot);
        applySnapshotJob(nextSnapshot);
      });
    }

    void initialize();
    return () => {
      disposed = true;
      media?.removeEventListener('change', handleSystemThemeChange);
      void dispose?.();
    };
  });

  $effect(() => {
    job?.id;
    isConfirmingCancel = false;
  });

  async function runAction(
    action: () => Promise<void>,
    { closeOnSuccess = false }: { closeOnSuccess?: boolean } = {},
  ) {
    isBusy = true;
    isConfirmingCancel = false;
    errorMessage = '';
    const result = await runPopupAction({
      action,
      close: closeOnSuccess && currentWindow ? () => currentWindow.close() : undefined,
    });
    if (!result.ok) {
      errorMessage = result.message;
    }
    isBusy = false;
  }

  return {
    get job() { return job; },
    get progress() { return clampProgress(job?.progress ?? 0); },
    get progressMetrics() {
      return job
        ? calculateDownloadProgressMetrics(job, progressSamples)
        : { averageSpeed: 0, timeRemaining: 0 };
    },
    get isBusy() { return isBusy; },
    get isConfirmingCancel() { return isConfirmingCancel; },
    get errorMessage() { return errorMessage; },
    runAction,
    onCancelClick: () => {
      const activeJobId = job?.id ?? jobId;
      if (!activeJobId) return;
      if (!isConfirmingCancel) {
        isConfirmingCancel = true;
        return;
      }
      void runAction(() => cancelJob(activeJobId));
    },
    onClose: () => {
      void currentWindow?.close();
    },
  };
}

export function clampProgress(progress: number) {
  if (!Number.isFinite(progress)) return 0;
  return Math.max(0, Math.min(100, progress));
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}
