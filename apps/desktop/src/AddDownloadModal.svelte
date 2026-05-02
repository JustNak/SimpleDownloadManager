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
  import { Archive, Link2, ListPlus, Magnet, PackagePlus, X } from '@lucide/svelte';
  import { addJob, addJobs, browseTorrentFile, type AddJobResult, type AddJobsResult } from './backend';
  import { getErrorMessage } from './errors';
  import {
    progressPopupIntentForSubmission,
    type DownloadMode,
  } from './batchProgress';
  import { downloadSubmitLabel, ensureTrailingEditableLine, parseDownloadUrlLines } from './downloadInput';
  import { validateOptionalSha256 } from './downloadIntegrity';

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

  const activeUrls = $derived(urlsForMode(mode));
  const canSubmit = $derived(activeUrls.length > 0 && !isSubmitting && !(mode === 'bulk' && combineBulk && !archiveName.trim()));
  const submitLabel = $derived(downloadSubmitLabel(mode, activeUrls.length, combineBulk));

  $effect(() => {
    mode;
    errorMessage = '';
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
      if (selected) torrentUrl = selected;
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : 'Could not import torrent file.';
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

  const modes: Array<{ id: DownloadMode; label: string; hint: string; icon: typeof Link2 }> = [
    { id: 'single', label: 'Single', hint: 'One URL with optional SHA-256 verification.', icon: Link2 },
    { id: 'torrent', label: 'Torrent', hint: 'Magnet link, .torrent URL, or local .torrent file.', icon: Magnet },
    { id: 'multi', label: 'Multi', hint: 'One URL per line, kept as separate downloads.', icon: ListPlus },
    { id: 'bulk', label: 'Bulk', hint: 'Queue many URLs and optionally combine them into one archive.', icon: Archive },
  ];
</script>

<div class="fixed inset-0 z-40 flex items-center justify-center bg-black/35 px-4" role="presentation" onmousedown={(event) => event.target === event.currentTarget && onClose()}>
  <div class="flex w-[640px] max-w-full flex-col overflow-hidden rounded-lg border border-border bg-popover text-popover-foreground shadow-2xl" role="dialog" aria-modal="true" aria-labelledby="add-download-title">
    <header class="flex items-center justify-between border-b border-border bg-header px-4 py-3">
      <div>
        <h2 id="add-download-title" class="text-sm font-semibold">New download</h2>
        <p class="mt-0.5 text-xs text-muted-foreground">Add regular downloads, torrents, or batch jobs.</p>
      </div>
      <button class="rounded p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground" title="Close" onclick={onClose}><X size={17} /></button>
    </header>

    <form class="flex min-h-0 flex-col" onsubmit={submitForm}>
      <div class="grid grid-cols-4 gap-2 border-b border-border bg-surface p-3">
        {#each modes as item (item.id)}
          {@const Icon = item.icon}
          <button
            type="button"
            class={`min-w-0 rounded border px-3 py-2 text-left transition ${mode === item.id ? 'border-primary bg-primary-soft text-accent-foreground' : 'border-border bg-background hover:bg-muted'}`}
            onclick={() => mode = item.id}
          >
            <div class="flex items-center gap-2 text-xs font-semibold"><Icon size={15} /> {item.label}</div>
            <div class="mt-1 line-clamp-2 text-[10px] leading-3 text-muted-foreground">{item.hint}</div>
          </button>
        {/each}
      </div>

      <div class="min-h-[280px] p-4">
        {#if mode === 'single'}
          <label class="block text-xs font-semibold text-foreground" for="single-download-url">Download URL</label>
          <input id="single-download-url" type="url" required class="mt-1 w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={singleUrl} placeholder="https://example.com/file.zip" />
          <label class="mt-4 block text-xs font-semibold text-foreground" for="single-download-sha256">Expected SHA-256</label>
          <input id="single-download-sha256" class="mt-1 w-full rounded border border-input bg-background px-3 py-2 text-sm font-mono" bind:value={singleSha256} placeholder="Optional integrity hash" />
        {:else if mode === 'torrent'}
          <label class="block text-xs font-semibold text-foreground" for="torrent-download-source">Torrent source</label>
          <div class="mt-1 flex gap-2">
            <input id="torrent-download-source" required class="min-w-0 flex-1 rounded border border-input bg-background px-3 py-2 text-sm" bind:value={torrentUrl} placeholder="magnet:?xt=... or https://example.com/file.torrent" />
            <button type="button" class="inline-flex items-center gap-2 rounded border border-border px-3 text-xs font-semibold hover:bg-muted" onclick={() => void importTorrentFile()} disabled={isImportingTorrent}>
              <PackagePlus size={15} /> Import
            </button>
          </div>
        {:else if mode === 'multi'}
          <label class="block text-xs font-semibold text-foreground" for="multi-download-urls">Download URLs</label>
          <textarea id="multi-download-urls" class="mt-1 h-44 w-full resize-none rounded border border-input bg-background px-3 py-2 text-sm" value={multiUrls} oninput={(event) => multiUrls = ensureTrailingEditableLine(event.currentTarget.value)} placeholder="https://example.com/file-1.zip&#10;https://example.com/file-2.zip"></textarea>
        {:else}
          <label class="block text-xs font-semibold text-foreground" for="bulk-download-urls">Bulk URLs</label>
          <textarea id="bulk-download-urls" class="mt-1 h-36 w-full resize-none rounded border border-input bg-background px-3 py-2 text-sm" value={bulkUrls} oninput={(event) => bulkUrls = ensureTrailingEditableLine(event.currentTarget.value)} placeholder="https://example.com/asset-1.png&#10;https://example.com/asset-2.png"></textarea>
          <label class="mt-3 flex items-center gap-2 text-xs font-semibold">
            <input type="checkbox" bind:checked={combineBulk} />
            Combine completed files into archive
          </label>
          {#if combineBulk}
            <input class="mt-2 w-full rounded border border-input bg-background px-3 py-2 text-sm" value={archiveName} oninput={(event) => archiveName = normalizeArchiveName(event.currentTarget.value)} placeholder="bulk-download.zip" />
          {/if}
        {/if}

        {#if errorMessage}
          <div class="mt-3 rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">{errorMessage}</div>
        {/if}
      </div>

      <footer class="flex items-center justify-between border-t border-border bg-surface px-4 py-3">
        <div class="text-xs text-muted-foreground">{activeUrls.length} item{activeUrls.length === 1 ? '' : 's'} ready</div>
        <div class="flex gap-2">
          <button type="button" class="rounded border border-border px-3 py-1.5 text-xs font-semibold hover:bg-muted" onclick={onClose}>Cancel</button>
          <button type="submit" class="rounded border border-primary/60 bg-primary px-3 py-1.5 text-xs font-semibold text-primary-foreground hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-45" disabled={!canSubmit}>
            {isSubmitting ? 'Adding...' : submitLabel}
          </button>
        </div>
      </footer>
    </form>
  </div>
</div>
