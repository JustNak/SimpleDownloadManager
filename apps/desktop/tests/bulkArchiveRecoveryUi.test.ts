import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const appSource = readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8');
const backendSource = readFileSync(new URL('../src/backend.ts', import.meta.url), 'utf8');
const queueSource = readFileSync(new URL('../src/QueueView.svelte', import.meta.url), 'utf8');
const commandSource = readFileSync(new URL('../src-tauri/src/commands/mod.rs', import.meta.url), 'utf8');
const mainSource = readFileSync(new URL('../src-tauri/src/main.rs', import.meta.url), 'utf8');

assert.match(backendSource, /export async function retryBulkArchive\(archiveId: string\): Promise<void>/, 'frontend backend should expose retryBulkArchive');
assert.match(backendSource, /invokeCommand\('retry_bulk_archive', \{ archiveId \}\)/, 'retryBulkArchive should call the Tauri retry_bulk_archive command');
assert.match(backendSource, /export async function retryBulkMembers\(archiveId: string\): Promise<BulkMemberRetryResult>/, 'frontend backend should expose retryBulkMembers');
assert.match(backendSource, /invokeCommand<BulkMemberRetryResult>\('retry_bulk_members', \{ archiveId \}\)/, 'retryBulkMembers should call the Tauri retry_bulk_members command');
assert.match(commandSource, /pub async fn retry_bulk_archive\(/, 'Rust commands should expose retry_bulk_archive');
assert.match(commandSource, /pub async fn retry_bulk_members\(/, 'Rust commands should expose retry_bulk_members');
assert.match(mainSource, /commands::retry_bulk_archive/, 'retry_bulk_archive should be registered with Tauri');
assert.match(mainSource, /commands::retry_bulk_members/, 'retry_bulk_members should be registered with Tauri');
assert.match(appSource, /retryBulkArchive\(row\.bulkArchiveId\)/, 'bulk aggregate retry should rerun archive creation by archive id');
assert.match(appSource, /retryBulkMembers\(row\.bulkArchiveId\)/, 'bulk aggregate member retry should retry failed members by archive id');
assert.match(queueSource, /function isFailedBulkAggregate[\s\S]*archiveStatus === 'failed'/, 'QueueView should identify failed bulk aggregate rows');
assert.match(queueSource, /bulkRetryableMemberCount > 0/, 'QueueView should enable bulk member retry only when failed members exist');
assert.match(queueSource, /Retry'[\s\S]*onRetryBulkMembers[\s\S]*bulkRetryableMemberCount <= 0/, 'bulk aggregate menus should expose a disabled-capable Retry action for member downloads');
assert.match(queueSource, /isFailedBulkAggregate\(job\)[\s\S]*Show Popup[\s\S]*Retry archive[\s\S]*Delete[\s\S]*Delete from disk/, 'failed bulk aggregate menus should expose popup, retry, delete, and disk-delete actions');
