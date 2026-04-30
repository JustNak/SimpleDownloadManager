import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { ConnectionState, DiagnosticsSnapshot, DownloadJob, DownloadPrompt, Settings } from './types';
import type { ProgressBatchContext } from './batchProgress';
import { buildAddJobCommandArgs, type AddJobOptions } from './backendCommandArgs';
import type { AppUpdateMetadata, UpdateInstallProgressEvent } from './appUpdates';

export interface DesktopSnapshot {
  connectionState: ConnectionState;
  jobs: DownloadJob[];
  settings: Settings;
}

export type AddJobStatus = 'queued' | 'duplicate_existing_job';

export interface AddJobResult {
  jobId: string;
  filename: string;
  status: AddJobStatus;
}

export interface AddJobsResult {
  results: AddJobResult[];
  queuedCount: number;
  duplicateCount: number;
}

export interface ExternalUseResult {
  pausedTorrent: boolean;
  autoReseedRetrySeconds?: number;
}

const STATE_CHANGED_EVENT = 'app://state-changed';
const DOWNLOAD_PROMPT_CHANGED_EVENT = 'app://download-prompt-changed';
const SELECT_JOB_EVENT = 'app://select-job';
const UPDATE_INSTALL_PROGRESS_EVENT = 'app://update-install-progress';
type BackendMockModule = typeof import('./backendMock');
let backendMockPromise: Promise<BackendMockModule> | null = null;

function loadBackendMock(): Promise<BackendMockModule> {
  backendMockPromise ??= import('./backendMock');
  return backendMockPromise;
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, args);
}

function createBatchId() {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `batch_${crypto.randomUUID()}`;
  }
  return `batch_${Date.now()}_${Math.random().toString(36).slice(2)}`;
}

export async function getAppSnapshot(): Promise<DesktopSnapshot> {
  if (!isTauriRuntime()) return (await loadBackendMock()).getAppSnapshot();
  return invokeCommand<DesktopSnapshot>('get_app_snapshot');
}

export async function getDiagnostics(): Promise<DiagnosticsSnapshot> {
  if (!isTauriRuntime()) return (await loadBackendMock()).getDiagnostics();

  return invokeCommand<DiagnosticsSnapshot>('get_diagnostics');
}

export async function pauseJob(id: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).pauseJob(id);
  await invokeCommand('pause_job', { id });
}

export async function resumeJob(id: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).resumeJob(id);
  await invokeCommand('resume_job', { id });
}

export async function pauseAllJobs(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).pauseAllJobs();
  await invokeCommand('pause_all_jobs');
}

export async function resumeAllJobs(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).resumeAllJobs();
  await invokeCommand('resume_all_jobs');
}

export async function cancelJob(id: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).cancelJob(id);
  await invokeCommand('cancel_job', { id });
}

export async function retryJob(id: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).retryJob(id);
  await invokeCommand('retry_job', { id });
}

export async function restartJob(id: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).restartJob(id);
  await invokeCommand('restart_job', { id });
}

export async function retryFailedJobs(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).retryFailedJobs();
  await invokeCommand('retry_failed_jobs');
}

export async function swapFailedDownloadToBrowser(id: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).swapFailedDownloadToBrowser(id);
  await invokeCommand('swap_failed_download_to_browser', { id });
}

export async function removeJob(id: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).removeJob(id);
  await invokeCommand('remove_job', { id });
}

export async function deleteJob(id: string, deleteFromDisk: boolean): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).deleteJob(id, deleteFromDisk);
  await invokeCommand('delete_job', { id, deleteFromDisk });
}

export async function deleteJobs(ids: string[], deleteFromDisk: boolean): Promise<void> {
  const uniqueIds = [...new Set(ids)].filter(Boolean);
  if (uniqueIds.length === 0) return;

  if (!isTauriRuntime()) return (await loadBackendMock()).deleteJobs(uniqueIds, deleteFromDisk);

  for (const id of uniqueIds) {
    await invokeCommand('delete_job', { id, deleteFromDisk });
  }
}

export async function renameJob(id: string, filename: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).renameJob(id, filename);
  await invokeCommand('rename_job', { id, filename });
}

export async function clearCompletedJobs(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).clearCompletedJobs();
  await invokeCommand('clear_completed_jobs');
}

export async function addJob(url: string, options?: AddJobOptions): Promise<AddJobResult> {
  const args = buildAddJobCommandArgs(url, options);
  if (!isTauriRuntime()) return (await loadBackendMock()).addJob(url, options);
  return invokeCommand<AddJobResult>('add_job', args);
}

export async function addJobs(urls: string[], bulkArchiveName?: string): Promise<AddJobsResult> {
  const normalizedUrls = urls.map((url) => url.trim()).filter(Boolean);
  if (normalizedUrls.length === 0) {
    throw new Error('Add at least one download URL.');
  }

  if (!isTauriRuntime()) return (await loadBackendMock()).addJobs(normalizedUrls, bulkArchiveName);

  return invokeCommand<AddJobsResult>('add_jobs', {
    urls: normalizedUrls,
    bulkArchiveName: bulkArchiveName?.trim() || undefined,
  });
}

export async function saveSettings(settings: Settings): Promise<Settings> {
  if (!isTauriRuntime()) return (await loadBackendMock()).saveSettings(settings);
  return invokeCommand<Settings>('save_settings', { settings });
}

export async function browseDirectory(): Promise<string | null> {
  if (!isTauriRuntime()) return (await loadBackendMock()).browseDirectory();
  return invokeCommand<string | null>('browse_directory');
}

export async function browseTorrentFile(): Promise<string | null> {
  if (!isTauriRuntime()) return (await loadBackendMock()).browseTorrentFile();
  return invokeCommand<string | null>('browse_torrent_file');
}

export async function getCurrentDownloadPrompt(): Promise<DownloadPrompt | null> {
  if (!isTauriRuntime()) return (await loadBackendMock()).getCurrentDownloadPrompt();
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
  if (!isTauriRuntime()) return (await loadBackendMock()).confirmDownloadPrompt(id, directoryOverride, options);
  await invokeCommand('confirm_download_prompt', {
    id,
    directoryOverride,
    duplicateAction: options.duplicateAction ?? 'return_existing',
    renamedFilename: options.renamedFilename ?? null,
  });
}

export async function showExistingDownloadPrompt(id: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).showExistingDownloadPrompt(id);
  await invokeCommand('show_existing_download_prompt', { id });
}

export async function swapDownloadPrompt(id: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).swapDownloadPrompt(id);
  await invokeCommand('swap_download_prompt', { id });
}

export async function cancelDownloadPrompt(id: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).cancelDownloadPrompt(id);
  await invokeCommand('cancel_download_prompt', { id });
}

export async function openProgressWindow(id: string): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).openProgressWindow(id);
  await invokeCommand('open_progress_window', { id });
}

export async function openBatchProgressWindow(context: ProgressBatchContext): Promise<string> {
  const batchId = context.batchId ?? createBatchId();
  const storedContext = { ...context, batchId };
  if (!isTauriRuntime()) return (await loadBackendMock()).openBatchProgressWindow(storedContext);

  await invokeCommand('open_batch_progress_window', { context: storedContext });
  return batchId;
}

export async function getProgressBatchContext(batchId: string): Promise<ProgressBatchContext | null> {
  if (!isTauriRuntime()) return (await loadBackendMock()).getProgressBatchContext(batchId);
  return invokeCommand<ProgressBatchContext | null>('get_progress_batch_context', { batchId });
}

export async function openJobFile(id: string): Promise<ExternalUseResult> {
  if (!isTauriRuntime()) return (await loadBackendMock()).openJobFile(id);
  return invokeCommand<ExternalUseResult>('open_job_file', { id });
}

export async function revealJobInFolder(id: string): Promise<ExternalUseResult> {
  if (!isTauriRuntime()) return (await loadBackendMock()).revealJobInFolder(id);
  return invokeCommand<ExternalUseResult>('reveal_job_in_folder', { id });
}

export async function openInstallDocs(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).openInstallDocs();
  await invokeCommand('open_install_docs');
}

export async function runHostRegistrationFix(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).runHostRegistrationFix();
  await invokeCommand('run_host_registration_fix');
}

export async function testExtensionHandoff(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).testExtensionHandoff();
  await invokeCommand('test_extension_handoff');
}

export async function exportDiagnosticsReport(): Promise<string | null> {
  if (!isTauriRuntime()) return (await loadBackendMock()).exportDiagnosticsReport();
  return invokeCommand<string | null>('export_diagnostics_report');
}

export async function checkForUpdate(): Promise<AppUpdateMetadata | null> {
  if (!isTauriRuntime()) return (await loadBackendMock()).checkForUpdate();
  return invokeCommand<AppUpdateMetadata | null>('check_for_update');
}

export async function installUpdate(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBackendMock()).installUpdate();
  await invokeCommand('install_update');
}

export async function subscribeToStateChanged(
  listener: (snapshot: DesktopSnapshot) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return (await loadBackendMock()).subscribeToStateChanged(listener);
  return listen<DesktopSnapshot>(STATE_CHANGED_EVENT, (event) => listener(event.payload));
}

export async function subscribeToDownloadPromptChanged(
  listener: (prompt: DownloadPrompt) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return (await loadBackendMock()).subscribeToDownloadPromptChanged(listener);
  return listen<DownloadPrompt>(DOWNLOAD_PROMPT_CHANGED_EVENT, (event) => listener(event.payload));
}

export async function subscribeToSelectedJobRequested(
  listener: (jobId: string) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return (await loadBackendMock()).subscribeToSelectedJobRequested(listener);
  return listen<string>(SELECT_JOB_EVENT, (event) => listener(event.payload));
}

export async function subscribeToUpdateInstallProgress(
  listener: (event: UpdateInstallProgressEvent) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return (await loadBackendMock()).subscribeToUpdateInstallProgress(listener);
  return listen<UpdateInstallProgressEvent>(UPDATE_INSTALL_PROGRESS_EVENT, (event) => listener(event.payload));
}
