<script module lang="ts">
  import type { AddJobResult as ModalAddJobResult, AddJobsResult as ModalAddJobsResult } from './backend';
  import type { DownloadMode as ModalDownloadMode, ProgressPopupIntent } from './batchProgress';

  export interface AddDownloadOutcome {
    mode: ModalDownloadMode;
    result: ModalAddJobResult | ModalAddJobsResult;
    primaryResult?: ModalAddJobResult;
    intent: ProgressPopupIntent | null;
    archiveName?: string;
  }
</script>

<script lang="ts">
  import type { Component } from 'svelte';
  import { Archive, Link2, ListPlus, Magnet, PackagePlus, X } from '@lucide/svelte';
  import { addJob, addJobs, browseTorrentFile, type AddJobResult, type AddJobsResult } from './backend';
  import { getErrorMessage } from './errors';
  import {
    progressPopupIntentForSubmission,
    type DownloadMode,
  } from './batchProgress';
  import {
    batchUrlTextAreaClassName,
    batchUrlTextAreaWrap,
    downloadSubmitLabel,
    ensureTrailingEditableLine,
    parseDownloadUrlLines,
  } from './downloadInput';
  import { validateOptionalSha256 } from './downloadIntegrity';

  type IconComponent = Component<{ size?: number; class?: string; strokeWidth?: number }>;

  interface Props {
    onClose: () => void;
    onAdded: (outcome: AddDownloadOutcome) => void;
  }

  let { onClose, onAdded }: Props = $props();

  let mode = $state<DownloadMode>('single');
  let singleUrl = $state('');
  let torrentUrl = $state('');
  let singleSha256 = $state('');
  let multiUrls = $state('');
  let bulkUrls = $state('');
  let archiveName = $state('bulk-download.zip');
  let combineBulk = $state(true);
  let isSubmitting = $state(false);
  let isImportingTorrent = $state(false);
  let errorMessage = $state('');
  let inputElement = $state<HTMLInputElement | HTMLTextAreaElement | null>(null);

  const activeUrls = $derived(urlsForMode(mode));
  const canSubmit = $derived(activeUrls.length > 0 && !isSubmitting && !(mode === 'bulk' && combineBulk && !archiveName.trim()));
  const submitLabel = $derived(downloadSubmitLabel(mode, activeUrls.length, combineBulk));
  const readyLabel = $derived(mode === 'torrent'
    ? `${activeUrls.length} ${activeUrls.length === 1 ? 'torrent' : 'torrents'} ready`
    : `${activeUrls.length} ${activeUrls.length === 1 ? 'link' : 'links'} ready`);

  $effect(() => {
    mode;
    errorMessage = '';
    requestAnimationFrame(() => inputElement?.focus());
  });

  $effect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', closeOnEscape);
    return () => document.removeEventListener('keydown', closeOnEscape);
  });

  function emitAdded(activeMode: DownloadMode, result: AddJobResult | AddJobsResult, activeArchiveName?: string) {
    const primaryResult = activeMode === 'single' || activeMode === 'torrent'
      ? result as AddJobResult
      : (result as AddJobsResult).results.find((item) => item.status === 'queued');
    onAdded({
      mode: activeMode,
      result,
      primaryResult,
      intent: progressPopupIntentForSubmission(activeMode, result, activeArchiveName),
      archiveName: activeArchiveName,
    });
  }

  function urlsForMode(activeMode: DownloadMode): string[] {
    if (activeMode === 'single') return singleUrl.trim() ? [singleUrl.trim()] : [];
    if (activeMode === 'torrent') return torrentUrl.trim() ? [torrentUrl.trim()] : [];
    if (activeMode === 'multi') return parseDownloadUrlLines(multiUrls);
    return parseDownloadUrlLines(bulkUrls);
  }

  function isHttpUrl(value: string): boolean {
    try {
      const parsed = new URL(value);
      return parsed.protocol === 'http:' || parsed.protocol === 'https:';
    } catch {
      return false;
    }
  }

  function isTorrentSource(value: string): boolean {
    if (value.startsWith('magnet:?')) return true;
    return isHttpUrl(value);
  }

  function validationErrorFor(activeMode: DownloadMode, urls: string[]): string | null {
    const invalid = urls.find((url) => activeMode === 'torrent' ? !isTorrentSource(url) : !isHttpUrl(url));
    if (!invalid) return null;
    return activeMode === 'torrent' ? 'Enter a valid magnet link or torrent URL.' : 'Enter a valid URL.';
  }

  function normalizeArchiveName(value: string) {
    const sanitized = value.replace(/[<>:"/\\|?*\u0000-\u001F]/g, '').trimStart();
    if (!sanitized) return '';
    return sanitized.toLowerCase().endsWith('.zip') ? sanitized : `${sanitized}.zip`;
  }

  async function importTorrentFile() {
    isImportingTorrent = true;
    errorMessage = '';
    try {
      const selected = await browseTorrentFile();
      if (selected) {
        mode = 'torrent';
        torrentUrl = selected;
      }
    } catch (error) {
      errorMessage = getErrorMessage(error, 'Failed to import torrent file.');
    } finally {
      isImportingTorrent = false;
    }
  }

  async function submitForm(event: SubmitEvent) {
    event.preventDefault();
    if (!canSubmit) return;
    const urls = activeUrls;
    const validationError = validationErrorFor(mode, urls);
    if (validationError) {
      errorMessage = validationError;
      return;
    }
    isSubmitting = true;
    errorMessage = '';

    try {
      if (mode === 'single') {
        const result = await addJob(urls[0], { expectedSha256: validateOptionalSha256(singleSha256), transferKind: 'http' });
        emitAdded(mode, result);
      } else if (mode === 'torrent') {
        const result = await addJob(urls[0], { transferKind: 'torrent' });
        emitAdded(mode, result);
      } else if (mode === 'multi') {
        const result = await addJobs(urls);
        emitAdded(mode, result);
      } else {
        const trimmedArchiveName = combineBulk ? normalizeArchiveName(archiveName) : undefined;
        const result = await addJobs(urls, trimmedArchiveName);
        emitAdded(mode, result, trimmedArchiveName);
      }
      onClose();
    } catch (error) {
      errorMessage = getErrorMessage(error, 'Failed to add downloads.');
    } finally {
      isSubmitting = false;
    }
  }

  const modes: Array<{ id: DownloadMode; label: string; icon: IconComponent }> = [
    { id: 'single', label: 'File', icon: Link2 },
    { id: 'torrent', label: 'Torrent', icon: Magnet },
    { id: 'multi', label: 'Multi', icon: ListPlus },
    { id: 'bulk', label: 'Bulk', icon: PackagePlus },
  ];
</script>

<div class="fixed inset-0 z-50 flex items-center justify-center bg-background/60 p-4 backdrop-blur-[1px]" role="presentation" onmousedown={(event) => event.target === event.currentTarget && onClose()}>
  <div class="w-full max-w-xl overflow-hidden rounded-md border border-border bg-card shadow-2xl animate-in fade-in zoom-in-95 duration-200" role="dialog" aria-modal="true" aria-labelledby="add-download-title">
    <header class="flex items-center justify-between border-b border-border bg-header px-5 py-3">
      <div>
        <h2 id="add-download-title" class="text-base font-semibold text-foreground">New Download</h2>
        <p class="mt-0.5 text-xs text-muted-foreground">Add a file, torrent, link list, or bulk archive.</p>
      </div>
      <button class="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground" aria-label="Close new download" title="Close" onclick={onClose}><X size={18} /></button>
    </header>

    <form onsubmit={submitForm}>
      <div class="border-b border-border px-5 py-3">
        <div class="grid grid-cols-4 rounded-md border border-border bg-background p-1">
          {#each modes as item (item.id)}
            {@const Icon = item.icon}
            <button
              type="button"
              class={`flex h-8 items-center justify-center gap-1.5 rounded-[4px] text-xs font-semibold transition ${mode === item.id ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:bg-muted hover:text-foreground'}`}
              onclick={() => mode = item.id}
            >
              <Icon size={15} />
              <span class="truncate">{item.label}</span>
            </button>
          {/each}
        </div>
      </div>

      <div class="space-y-3 px-5 py-4">
        {#if mode === 'single'}
          <div>
            <div class="mb-2 flex items-end justify-between gap-3">
              <label class="text-xs font-semibold text-foreground" for="single-download-url">File URL</label>
              <span class="text-xs text-muted-foreground">HTTP(S) direct download.</span>
            </div>
            <input bind:this={inputElement} id="single-download-url" type="url" required class="h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" bind:value={singleUrl} placeholder="https://example.com/file.zip" />
          </div>
          <div>
            <div class="mb-2 flex items-end justify-between gap-3">
              <label class="text-xs font-semibold text-foreground" for="single-download-sha256">SHA-256 Checksum</label>
              <span class="text-xs text-muted-foreground">Optional integrity check after download.</span>
            </div>
            <input id="single-download-sha256" class="h-9 w-full rounded-md border border-input bg-background px-3 font-mono text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" bind:value={singleSha256} placeholder="64-character hex digest" spellcheck="false" />
          </div>
        {:else if mode === 'torrent'}
          <section class="space-y-3">
            <div class="flex items-center gap-2 text-sm font-semibold text-foreground">
              <Magnet size={16} class="text-primary" />
              <span>Add Torrent</span>
            </div>
            <div>
              <div class="mb-2 flex items-end justify-between gap-3">
                <label class="text-xs font-semibold text-foreground" for="torrent-download-source">Torrent URL</label>
                <span class="text-xs text-muted-foreground">Magnet or HTTP(S) .torrent link.</span>
              </div>
              <input bind:this={inputElement} id="torrent-download-source" required class="h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" bind:value={torrentUrl} placeholder="magnet:?xt=urn:btih:... or https://example.com/file.torrent" />
            </div>
          </section>
        {:else if mode === 'multi'}
          <div>
            <div class="mb-2 flex items-end justify-between gap-3">
              <label class="text-xs font-semibold text-foreground" for="multi-download-urls">Download URLs</label>
              <span class="text-xs text-muted-foreground">Paste one HTTP(S) file link per line.</span>
            </div>
            <textarea bind:this={inputElement} id="multi-download-urls" rows="7" wrap={batchUrlTextAreaWrap} class={batchUrlTextAreaClassName} value={multiUrls} oninput={(event) => multiUrls = ensureTrailingEditableLine(event.currentTarget.value)} placeholder="https://example.com/file-01.zip&#10;https://example.com/file-02.zip"></textarea>
          </div>
        {:else}
          <div>
            <div class="mb-2 flex items-end justify-between gap-3">
              <label class="text-xs font-semibold text-foreground" for="bulk-download-urls">Bulk Links</label>
              <span class="text-xs text-muted-foreground">Paste one HTTP(S) file link per line.</span>
            </div>
            <textarea bind:this={inputElement} id="bulk-download-urls" rows="7" wrap={batchUrlTextAreaWrap} class={batchUrlTextAreaClassName} value={bulkUrls} oninput={(event) => bulkUrls = ensureTrailingEditableLine(event.currentTarget.value)} placeholder="https://example.com/assets/model.fbx&#10;https://example.com/assets/textures.zip&#10;https://example.com/assets/readme.pdf"></textarea>
          </div>
          <div class="grid gap-3 rounded-md border border-border bg-background p-3 md:grid-cols-[1fr_220px]">
            <label class="flex items-start gap-3 text-sm">
              <input type="checkbox" bind:checked={combineBulk} class="mt-1 h-4 w-4 accent-primary" />
              <span>
                <span class="flex items-center gap-2 font-medium text-foreground">
                  <Archive size={16} />
                  Combine into one archive
                </span>
                <span class="mt-1 block text-xs leading-5 text-muted-foreground">Links are queued together with an archive name so the batch can be collected as one compressed output.</span>
              </span>
            </label>
            <input class="h-9 rounded-md border border-input bg-card px-3 text-sm text-foreground outline-none transition focus:border-primary disabled:cursor-not-allowed disabled:opacity-50" value={archiveName} oninput={(event) => archiveName = normalizeArchiveName(event.currentTarget.value)} disabled={!combineBulk} aria-label="Archive file name" />
          </div>
        {/if}

        <div class="flex items-center justify-between rounded-md border border-border bg-background px-3 py-2 text-xs text-muted-foreground">
          <span>{readyLabel}</span>
          <span>{mode === 'torrent' ? 'Torrent' : mode === 'bulk' && combineBulk ? archiveName : 'Queue only'}</span>
        </div>

        {#if errorMessage}
          <p class="rounded-md border border-destructive/35 bg-destructive/10 px-3 py-2 text-sm text-destructive">{errorMessage}</p>
        {/if}
      </div>

      <footer class="flex items-center justify-between gap-3 border-t border-border px-5 py-3">
        <div>
          {#if mode === 'torrent'}
            <button type="button" class="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-semibold text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50" title="Import magnet or torrent file" onclick={() => void importTorrentFile()} disabled={isImportingTorrent}>
              {@render TorrentFileIcon()}
              <span>{isImportingTorrent ? 'Importing...' : 'Import'}</span>
            </button>
          {/if}
        </div>
        <div class="flex justify-end gap-3">
          <button type="button" class="h-9 rounded-md px-4 text-sm font-semibold text-foreground transition-colors hover:bg-muted" onclick={onClose}>Cancel</button>
          <button type="submit" class="h-9 rounded-md bg-primary px-4 text-sm font-semibold text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50" disabled={!canSubmit}>
            {isSubmitting ? 'Adding...' : submitLabel}
          </button>
        </div>
      </footer>
    </form>
  </div>
</div>

{#snippet TorrentFileIcon()}
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true" class="shrink-0">
    <path d="M4 1.75h5.25L12 4.5v9.75H4V1.75Z" stroke="currentColor" stroke-width="1.35" stroke-linejoin="round" />
    <path d="M9.25 1.75V4.5H12" stroke="currentColor" stroke-width="1.35" stroke-linejoin="round" />
    <path d="M6.15 7.1v2.05a1.85 1.85 0 0 0 3.7 0V7.1" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" />
    <path d="M6.15 7.1h1.2M8.65 7.1h1.2" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" />
  </svg>
{/snippet}
