import { useEffect, useMemo, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { applyAppearance } from './appearance';
import {
  cancelJob,
  getAppSnapshot,
  subscribeToStateChanged,
} from './backend';
import {
  calculateDownloadProgressMetrics,
  recordProgressSample,
  type DownloadProgressMetrics,
  type ProgressSample,
} from './downloadProgressMetrics';
import { runPopupAction } from './popupActions';
import type { DownloadJob } from './types';

export type PopupActionRunner = (
  action: () => Promise<void>,
  options?: { closeOnSuccess?: boolean },
) => Promise<void>;

export interface ProgressPopupState {
  job: DownloadJob | null;
  progress: number;
  progressMetrics: DownloadProgressMetrics;
  isBusy: boolean;
  isConfirmingCancel: boolean;
  errorMessage: string;
  runAction: PopupActionRunner;
  onCancelClick: () => void;
  onClose: () => void;
}

export function useProgressPopup(): ProgressPopupState {
  const [job, setJob] = useState<DownloadJob | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [isConfirmingCancel, setIsConfirmingCancel] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');
  const progressSamplesRef = useRef<ProgressSample[]>([]);
  const currentWindow = useMemo(() => (isTauriRuntime() ? getCurrentWindow() : null), []);
  const jobId = useMemo(() => new URLSearchParams(window.location.search).get('jobId') || '', []);

  useEffect(() => {
    let dispose: (() => void | Promise<void>) | undefined;
    let latestSettings: Awaited<ReturnType<typeof getAppSnapshot>>['settings'] | null = null;

    const applySnapshotAppearance = (snapshot: Awaited<ReturnType<typeof getAppSnapshot>>) => {
      latestSettings = snapshot.settings;
      applyAppearance(snapshot.settings);
    };
    const applySnapshotJob = (snapshot: Awaited<ReturnType<typeof getAppSnapshot>>) => {
      const nextJob = snapshot.jobs.find((candidate) => candidate.id === jobId) ?? null;
      if (nextJob) {
        progressSamplesRef.current = recordProgressSample(progressSamplesRef.current, nextJob);
      }
      setJob(nextJob);
    };

    const media = typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-color-scheme: dark)') : null;
    const handleSystemThemeChange = () => {
      if (latestSettings) applyAppearance(latestSettings);
    };
    media?.addEventListener('change', handleSystemThemeChange);

    async function initialize() {
      const snapshot = await getAppSnapshot();
      applySnapshotAppearance(snapshot);
      applySnapshotJob(snapshot);
      dispose = await subscribeToStateChanged((nextSnapshot) => {
        applySnapshotAppearance(nextSnapshot);
        applySnapshotJob(nextSnapshot);
      });
    }

    void initialize();
    return () => {
      media?.removeEventListener('change', handleSystemThemeChange);
      void dispose?.();
    };
  }, [jobId]);

  useEffect(() => {
    setIsConfirmingCancel(false);
  }, [job?.id]);

  async function runAction(
    action: () => Promise<void>,
    { closeOnSuccess = false }: { closeOnSuccess?: boolean } = {},
  ) {
    setIsBusy(true);
    setIsConfirmingCancel(false);
    setErrorMessage('');
    const result = await runPopupAction({
      action,
      close: closeOnSuccess && currentWindow ? () => currentWindow.close() : undefined,
    });
    if (!result.ok) {
      setErrorMessage(result.message);
    }
    setIsBusy(false);
  }

  const progress = clampProgress(job?.progress ?? 0);
  const progressMetrics = job
    ? calculateDownloadProgressMetrics(job, progressSamplesRef.current)
    : { averageSpeed: 0, timeRemaining: 0 };

  return {
    job,
    progress,
    progressMetrics,
    isBusy,
    isConfirmingCancel,
    errorMessage,
    runAction,
    onCancelClick: () => {
      const activeJobId = job?.id ?? jobId;
      if (!activeJobId) return;

      if (!isConfirmingCancel) {
        setIsConfirmingCancel(true);
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
