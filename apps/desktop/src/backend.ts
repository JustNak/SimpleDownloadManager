import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { ConnectionState, DiagnosticsSnapshot, DownloadJob, DownloadPrompt, Settings, TorrentSessionCacheClearResult } from './types';
import type { ProgressBatchContext } from './batchProgress';
import { buildAddJobCommandArgs, buildAddJobsCommandArgs, type AddJobOptions, type AddJobsOptions } from './backendCommandArgs';
import type { AppUpdateMetadata, UpdateInstallProgressEvent } from './appUpdates';
import * as previewBackend from './backendPreview';
export { applyDownloadUpdateBatch } from './downloadUpdateBatch';

export interface DesktopSnapshot {
  connectionState: ConnectionState;
  jobs: DownloadJob[];
  settings: Settings;
}

export interface DownloadUpdateBatch {
  jobs: DownloadJob[];
  removedJobIds: string[];
}

export interface ProgressJobSnapshot {
  job: DownloadJob | null;
  settings: Settings;
}

export interface BatchProgressSnapshot {
  context: ProgressBatchContext | null;
  jobs: DownloadJob[];
  settings: Settings;
}

export interface SettingsSnapshot {
  settings: Settings;
}

export type AddJobStatus = 'queued' | 'duplicate_existing_job';

export interface AddJobResult {
  jobId: string;
  filename: string;
  status: AddJobStatus;
}

export interface FailedBatchItem {
  url: string;
  message: string;
}

export interface AddJobsResult {
  results: AddJobResult[];
  queuedCount: number;
  duplicateCount: number;
  failedItems: FailedBatchItem[];
}

export interface BulkMemberRetryResult {
  queuedCount: number;
  failedItems: FailedBatchItem[];
}

export interface ExternalUseResult {
  pausedTorrent: boolean;
  autoReseedRetrySeconds?: number;
}

const STATE_CHANGED_EVENT = 'app://state-changed';
const DOWNLOADS_UPDATE_BATCH_EVENT = 'app://downloads-update-batch';
const PROGRESS_JOB_SNAPSHOT_EVENT = 'app://progress-job-snapshot';
const BATCH_PROGRESS_SNAPSHOT_EVENT = 'app://batch-progress-snapshot';
const SETTINGS_SNAPSHOT_EVENT = 'app://settings-snapshot';
const DOWNLOAD_PROMPT_CHANGED_EVENT = 'app://download-prompt-changed';
const SELECT_JOB_EVENT = 'app://select-job';
const UPDATE_INSTALL_PROGRESS_EVENT = 'app://update-install-progress';

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, args);
}

export async function getAppSnapshot(): Promise<DesktopSnapshot> {
  if (!isTauriRuntime()) return previewBackend.getMockAppSnapshot();
  return invokeCommand<DesktopSnapshot>('get_app_snapshot');
}

export async function getProgressJobSnapshot(id: string): Promise<ProgressJobSnapshot> {
  if (!isTauriRuntime()) return previewBackend.getMockProgressJobSnapshot(id);
  return invokeCommand<ProgressJobSnapshot>('get_progress_job_snapshot', { id });
}

export async function getBatchProgressSnapshot(batchId: string): Promise<BatchProgressSnapshot> {
  if (!isTauriRuntime()) return previewBackend.getMockBatchProgressSnapshot(batchId);
  return invokeCommand<BatchProgressSnapshot>('get_batch_progress_snapshot', { batchId });
}

export async function getSettingsSnapshot(): Promise<SettingsSnapshot> {
  if (!isTauriRuntime()) return previewBackend.getMockSettingsSnapshot();
  return invokeCommand<SettingsSnapshot>('get_settings_snapshot');
}

export async function getDiagnostics(): Promise<DiagnosticsSnapshot> {
  if (!isTauriRuntime()) return previewBackend.getMockDiagnostics();
  return invokeCommand<DiagnosticsSnapshot>('get_diagnostics');
}

export async function pauseJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.pauseMockJob(id);
    return;
  }
  await invokeCommand('pause_job', { id });
}

export async function pauseJobs(ids: string[]): Promise<void> {
  const uniqueIds = [...new Set(ids)].filter(Boolean);
  if (uniqueIds.length === 0) return;

  if (!isTauriRuntime()) {
    previewBackend.pauseMockJobs(uniqueIds);
    return;
  }

  await invokeCommand('pause_jobs', { ids: uniqueIds });
}

export async function resumeJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.resumeMockJob(id);
    return;
  }
  await invokeCommand('resume_job', { id });
}

export async function resumeJobs(ids: string[]): Promise<void> {
  const uniqueIds = [...new Set(ids)].filter(Boolean);
  if (uniqueIds.length === 0) return;

  if (!isTauriRuntime()) {
    previewBackend.resumeMockJobs(uniqueIds);
    return;
  }

  await invokeCommand('resume_jobs', { ids: uniqueIds });
}

export async function pauseAllJobs(): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.pauseAllMockJobs();
    return;
  }
  await invokeCommand('pause_all_jobs');
}

export async function resumeAllJobs(): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.resumeAllMockJobs();
    return;
  }
  await invokeCommand('resume_all_jobs');
}

export async function cancelJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.cancelMockJob(id);
    return;
  }
  await invokeCommand('cancel_job', { id });
}

export async function cancelJobs(ids: string[]): Promise<void> {
  const uniqueIds = [...new Set(ids)].filter(Boolean);
  if (uniqueIds.length === 0) return;

  if (!isTauriRuntime()) {
    previewBackend.cancelMockJobs(uniqueIds);
    return;
  }

  await invokeCommand('cancel_jobs', { ids: uniqueIds });
}

export async function retryJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.retryMockJob(id);
    return;
  }
  await invokeCommand('retry_job', { id });
}

export async function restartJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.restartMockJob(id);
    return;
  }
  await invokeCommand('restart_job', { id });
}

export async function retryFailedJobs(): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.retryFailedMockJobs();
    return;
  }
  await invokeCommand('retry_failed_jobs');
}

export async function swapFailedDownloadToBrowser(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.swapFailedMockDownloadToBrowser(id);
    return;
  }
  await invokeCommand('swap_failed_download_to_browser', { id });
}

export async function removeJob(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.removeMockJob(id);
    return;
  }
  await invokeCommand('remove_job', { id });
}

export async function deleteJob(id: string, deleteFromDisk: boolean): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.deleteMockJob(id);
    return;
  }
  await invokeCommand('delete_job', { id, deleteFromDisk });
}

export async function deleteJobs(ids: string[], deleteFromDisk: boolean): Promise<void> {
  const uniqueIds = [...new Set(ids)].filter(Boolean);
  if (uniqueIds.length === 0) return;

  if (!isTauriRuntime()) {
    previewBackend.deleteMockJobs(uniqueIds);
    return;
  }

  await invokeCommand('delete_jobs', { ids: uniqueIds, deleteFromDisk });
}

export async function renameJob(id: string, filename: string): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.renameMockJob(id, filename);
    return;
  }
  await invokeCommand('rename_job', { id, filename });
}

export async function clearCompletedJobs(): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.clearCompletedMockJobs();
    return;
  }
  await invokeCommand('clear_completed_jobs');
}

export async function addJob(url: string, options?: AddJobOptions): Promise<AddJobResult> {
  const args = buildAddJobCommandArgs(url, options);
  if (!isTauriRuntime()) {
    return previewBackend.addMockJob(args);
  }
  return invokeCommand<AddJobResult>('add_job', args);
}

export async function addJobs(urls: string[], bulkArchiveName?: string, options: AddJobsOptions = {}): Promise<AddJobsResult> {
  const args = buildAddJobsCommandArgs(urls, bulkArchiveName, options);
  const normalizedUrls = args.urls;
  if (normalizedUrls.length === 0) {
    throw new Error('Add at least one download URL.');
  }

  if (!isTauriRuntime()) {
    return previewBackend.addMockJobs(args);
  }

  return invokeCommand<AddJobsResult>('add_jobs', args);
}

export async function saveSettings(settings: Settings): Promise<Settings> {
  if (!isTauriRuntime()) {
    return previewBackend.saveMockSettings(settings);
  }
  return invokeCommand<Settings>('save_settings', { settings });
}

export async function browseDirectory(): Promise<string | null> {
  if (!isTauriRuntime()) return previewBackend.browseMockDirectory();
  return invokeCommand<string | null>('browse_directory');
}

export async function clearTorrentSessionCache(): Promise<TorrentSessionCacheClearResult> {
  if (!isTauriRuntime()) {
    return previewBackend.clearMockTorrentSessionCache();
  }
  return invokeCommand<TorrentSessionCacheClearResult>('clear_torrent_session_cache');
}

export async function browseTorrentFile(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return previewBackend.browseMockTorrentFile();
  }
  return invokeCommand<string | null>('browse_torrent_file');
}

export async function getCurrentDownloadPrompt(): Promise<DownloadPrompt | null> {
  if (!isTauriRuntime()) {
    return previewBackend.getMockCurrentDownloadPrompt();
  }
  return invokeCommand<DownloadPrompt | null>('get_current_download_prompt');
}

export type PromptDuplicateAction = 'return_existing' | 'download_anyway' | 'overwrite' | 'rename';

export interface ConfirmDownloadPromptOptions {
  duplicateAction?: PromptDuplicateAction;
  renamedFilename?: string | null;
}

export async function confirmDownloadPrompt(
  id: string,
  directoryOverride: string | null,
  options: ConfirmDownloadPromptOptions = {},
): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('confirm_download_prompt', {
    id,
    directoryOverride,
    duplicateAction: options.duplicateAction ?? 'return_existing',
    renamedFilename: options.renamedFilename ?? null,
  });
}

export async function showExistingDownloadPrompt(id: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('show_existing_download_prompt', { id });
}

export async function swapDownloadPrompt(id: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('swap_download_prompt', { id });
}

export async function cancelDownloadPrompt(id: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('cancel_download_prompt', { id });
}

export async function openProgressWindow(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    previewBackend.openMockProgressWindow(id);
    return;
  }
  await invokeCommand('open_progress_window', { id });
}

export async function openBatchProgressWindow(context: ProgressBatchContext): Promise<string> {
  if (!isTauriRuntime()) {
    return previewBackend.openMockBatchProgressWindow(context);
  }

  const storedContext = previewBackend.createStoredProgressBatchContext(context);
  await invokeCommand('open_batch_progress_window', { context: storedContext });
  return storedContext.batchId;
}

export async function getProgressBatchContext(batchId: string): Promise<ProgressBatchContext | null> {
  if (!isTauriRuntime()) return previewBackend.getMockProgressBatchContext(batchId);
  return invokeCommand<ProgressBatchContext | null>('get_progress_batch_context', { batchId });
}

export async function openJobFile(id: string): Promise<ExternalUseResult> {
  if (!isTauriRuntime()) return previewBackend.prepareMockExternalUse(id);
  return invokeCommand<ExternalUseResult>('open_job_file', { id });
}

export async function revealJobInFolder(id: string): Promise<ExternalUseResult> {
  if (!isTauriRuntime()) return previewBackend.prepareMockExternalUse(id);
  return invokeCommand<ExternalUseResult>('reveal_job_in_folder', { id });
}

export async function openBulkArchive(archiveId: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('open_bulk_archive', { archiveId });
}

export async function revealBulkArchive(archiveId: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('reveal_bulk_archive', { archiveId });
}

export async function retryBulkArchive(archiveId: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('retry_bulk_archive', { archiveId });
}

export async function retryBulkMembers(archiveId: string): Promise<BulkMemberRetryResult> {
  if (!isTauriRuntime()) return { queuedCount: 0, failedItems: [] };
  return invokeCommand<BulkMemberRetryResult>('retry_bulk_members', { archiveId });
}

export async function openInstallDocs(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('open_install_docs');
}

export async function runHostRegistrationFix(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('run_host_registration_fix');
}

export async function testExtensionHandoff(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('test_extension_handoff');
}

export async function exportDiagnosticsReport(): Promise<string | null> {
  if (!isTauriRuntime()) return previewBackend.exportMockDiagnosticsReport();
  return invokeCommand<string | null>('export_diagnostics_report');
}

export async function checkForUpdate(): Promise<AppUpdateMetadata | null> {
  if (!isTauriRuntime()) return null;
  return invokeCommand<AppUpdateMetadata | null>('check_for_update');
}

export async function getInstalledVersion(): Promise<string> {
  if (!isTauriRuntime()) return 'Preview build';
  return invokeCommand<string>('plugin:app|version');
}

export async function installUpdate(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invokeCommand('install_update');
}

export async function subscribeToStateChanged(
  listener: (snapshot: DesktopSnapshot) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return previewBackend.subscribeMockStateChanged(listener);
  }
  return listen<DesktopSnapshot>(STATE_CHANGED_EVENT, (event) => listener(event.payload));
}

export async function subscribeToDownloadUpdateBatch(
  listener: (batch: DownloadUpdateBatch) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return async () => undefined;
  return listen<DownloadUpdateBatch>(DOWNLOADS_UPDATE_BATCH_EVENT, (event) => listener(event.payload));
}

export async function subscribeToProgressJobSnapshot(
  listener: (snapshot: ProgressJobSnapshot) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return async () => undefined;
  return listen<ProgressJobSnapshot>(PROGRESS_JOB_SNAPSHOT_EVENT, (event) => listener(event.payload));
}

export async function subscribeToBatchProgressSnapshot(
  listener: (snapshot: BatchProgressSnapshot) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return async () => undefined;
  return listen<BatchProgressSnapshot>(BATCH_PROGRESS_SNAPSHOT_EVENT, (event) => listener(event.payload));
}

export async function subscribeToSettingsSnapshot(
  listener: (snapshot: SettingsSnapshot) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return async () => undefined;
  return listen<SettingsSnapshot>(SETTINGS_SNAPSHOT_EVENT, (event) => listener(event.payload));
}

export async function subscribeToDownloadPromptChanged(
  listener: (prompt: DownloadPrompt) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return async () => undefined;
  return listen<DownloadPrompt>(DOWNLOAD_PROMPT_CHANGED_EVENT, (event) => listener(event.payload));
}

export async function subscribeToSelectedJobRequested(
  listener: (jobId: string) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return async () => undefined;
  return listen<string>(SELECT_JOB_EVENT, (event) => listener(event.payload));
}

export async function subscribeToUpdateInstallProgress(
  listener: (event: UpdateInstallProgressEvent) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return async () => undefined;
  return listen<UpdateInstallProgressEvent>(UPDATE_INSTALL_PROGRESS_EVENT, (event) => listener(event.payload));
}
