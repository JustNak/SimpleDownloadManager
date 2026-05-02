<script lang="ts">
  import { tick } from 'svelte';
  import { AlertTriangle, ChevronDown, Download, FolderOpen, Globe, HardDrive, MousePointerClick } from '@lucide/svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import type { DownloadPrompt, Settings } from './types';
  import {
    browseDirectory,
    cancelDownloadPrompt,
    confirmDownloadPrompt,
    getCurrentDownloadPrompt,
    getSettingsSnapshot,
    swapDownloadPrompt,
    subscribeToDownloadPromptChanged,
    subscribeToSettingsSnapshot,
  } from './backend';
  import PopupTitlebar from './PopupTitlebar.svelte';
  import FileBadge from './FileBadge.svelte';
  import { formatBytes, getHost, joinDisplayPath } from './popupShared';
  import { getErrorMessage } from './errors';
  import { applyAppearance } from './appearance';
  import { categoryFolderForFilename } from './downloadCategories';

  let prompt = $state<DownloadPrompt | null>(null);
  let directoryOverride = $state<string | null>(null);
  let duplicateMenuOpen = $state(false);
  let isRenamingDuplicate = $state(false);
  let renamedFilename = $state('');
  let isBusy = $state(false);
  let errorMessage = $state('');
  let renameInput: HTMLInputElement | null = $state(null);
  const currentWindow = isTauriRuntime() ? getCurrentWindow() : null;

  const destination = $derived.by(() => {
    if (!prompt) return '';
    return directoryOverride
      ? joinDisplayPath(joinDisplayPath(directoryOverride, categoryFolderForFilename(prompt.filename)), prompt.filename)
      : prompt.targetPath;
  });
  const isDuplicate = $derived(Boolean(prompt?.duplicateJob || prompt?.duplicatePath));
  const duplicateLabel = $derived(prompt?.duplicateJob?.filename ?? prompt?.duplicateFilename ?? prompt?.duplicatePath ?? '');
  const duplicateMessage = $derived(prompt?.duplicateJob ? 'Already in queue: ' : 'Destination exists: ');
  const overwriteLabel = $derived(prompt?.duplicateJob ? 'replace queue' : 'replace file');
  const canSwapToBrowser = $derived(prompt?.source?.entryPoint === 'browser_download' && !isDuplicate);
  const sourceLabel = $derived(prompt?.source
    ? `${prompt.source.browser} ${prompt.source.entryPoint.replaceAll('_', ' ')}`
    : 'Browser download');
  const trimmedRenamedFilename = $derived(renamedFilename.trim());

  $effect(() => {
    let promptDispose: (() => void | Promise<void>) | undefined;
    let stateDispose: (() => void | Promise<void>) | undefined;
    let latestSettings: Settings | null = null;

    const applySnapshotAppearance = (snapshot: Awaited<ReturnType<typeof getSettingsSnapshot>>) => {
      latestSettings = snapshot.settings;
      applyAppearance(snapshot.settings);
    };

    const media = typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-color-scheme: dark)') : null;
    const handleSystemThemeChange = () => {
      if (latestSettings) applyAppearance(latestSettings);
    };
    media?.addEventListener('change', handleSystemThemeChange);

    async function initialize() {
      applySnapshotAppearance(await getSettingsSnapshot());
      prompt = await getCurrentDownloadPrompt();
      promptDispose = await subscribeToDownloadPromptChanged((nextPrompt) => {
        directoryOverride = null;
        duplicateMenuOpen = false;
        isRenamingDuplicate = false;
        renamedFilename = '';
        errorMessage = '';
        isBusy = false;
        prompt = nextPrompt;
      });
      stateDispose = await subscribeToSettingsSnapshot((nextSnapshot) => {
        applySnapshotAppearance(nextSnapshot);
      });
    }

    void initialize();
    return () => {
      media?.removeEventListener('change', handleSystemThemeChange);
      void promptDispose?.();
      void stateDispose?.();
    };
  });

  $effect(() => {
    if (!isRenamingDuplicate) return;
    void tick().then(() => renameInput?.focus());
  });

  async function runAction(action: () => Promise<void>) {
    isBusy = true;
    errorMessage = '';
    try {
      await action();
    } catch (error) {
      errorMessage = getErrorMessage(error, 'Action failed.');
      isBusy = false;
    }
  }

  async function handleChangeDirectory() {
    const selected = await browseDirectory();
    if (selected) directoryOverride = selected;
  }

  async function handleClose() {
    if (prompt) {
      await cancelDownloadPrompt(prompt.id).catch(() => undefined);
      return;
    }
    await currentWindow?.close();
  }

  function startDuplicateRename() {
    if (!prompt) return;
    renamedFilename = prompt.filename;
    duplicateMenuOpen = false;
    isRenamingDuplicate = true;
  }

  async function confirmDuplicateAction(duplicateAction: 'download_anyway' | 'overwrite') {
    const activePrompt = prompt;
    if (!activePrompt) return;
    duplicateMenuOpen = false;
    await runAction(() => confirmDownloadPrompt(activePrompt.id, directoryOverride, { duplicateAction }));
  }

  async function confirmDuplicateRename() {
    const activePrompt = prompt;
    if (!activePrompt || !trimmedRenamedFilename) return;
    await runAction(() => confirmDownloadPrompt(activePrompt.id, directoryOverride, {
      duplicateAction: 'rename',
      renamedFilename: trimmedRenamedFilename,
    }));
  }

  function isTauriRuntime(): boolean {
    return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
  }
</script>

{#if !prompt}
  <div class="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
    <PopupTitlebar title="Download prompt" onClose={() => void currentWindow?.close()} />
    <div class="flex flex-1 items-center justify-center text-sm text-muted-foreground">Waiting for a download...</div>
  </div>
{:else}
  {@const activePrompt = prompt}
  <div class="app-window flex h-screen flex-col overflow-hidden border border-border bg-background text-foreground shadow-2xl">
    <PopupTitlebar title={isDuplicate ? 'Duplicate download detected' : 'New download detected'} onClose={() => void handleClose()} />

    <main class="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface px-3 py-2">
      {#if isDuplicate}
        <div class="mb-1.5 flex shrink-0 items-center gap-1.5 overflow-hidden rounded border border-warning/45 bg-warning/10 px-2 py-1.5 text-[11px] leading-4 text-warning">
          <AlertTriangle size={14} class="shrink-0" />
          <div class="min-w-0 flex-1 truncate" title={duplicateLabel}>
            <span class="font-semibold text-foreground">{duplicateMessage}</span>
            <span class="text-warning/90">{duplicateLabel}</span>
          </div>
        </div>
      {/if}

      <section class="flex min-h-0 min-w-0 shrink-0 gap-2">
        <FileBadge filename={activePrompt.filename} />
        <div class="min-w-0 flex-1 overflow-hidden">
          <h1 class="truncate text-sm font-semibold leading-5 text-foreground" title={activePrompt.filename}>{activePrompt.filename}</h1>
          <div class="truncate text-[11px] leading-4 text-muted-foreground" title={activePrompt.url}>{getHost(activePrompt.url)}</div>
          <div class="mt-1.5 grid min-w-0 grid-cols-[86px_minmax(0,1fr)] gap-x-2 gap-y-0.5 text-[10px] leading-[14px]">
            {@render MetaLabel(Globe, 'Source')}
            {@render MetaValue(activePrompt.url, true)}
            {@render MetaLabel(FolderOpen, 'Destination')}
            {@render MetaValue(destination || 'Choose a destination before downloading.')}
            {@render MetaLabel(HardDrive, 'File size')}
            {@render MetaValue(formatBytes(activePrompt.totalBytes))}
            {@render MetaLabel(MousePointerClick, 'Detected by')}
            {@render MetaValue(sourceLabel)}
          </div>
        </div>
      </section>

      {#if errorMessage}
        <div class="mt-1.5 shrink-0 truncate rounded border border-destructive/40 bg-destructive/10 px-2 py-1 text-[11px] leading-4 text-destructive" title={errorMessage}>
          {errorMessage}
        </div>
      {/if}

      <div class="mt-auto flex min-h-[38px] shrink-0 items-center justify-between gap-2 border-t border-border pt-2">
        <button
          onclick={() => void handleChangeDirectory()}
          disabled={isBusy}
          class="flex h-8 shrink-0 items-center gap-2 rounded border border-input px-3 text-xs font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
        >
          <FolderOpen size={16} />
          Change
        </button>

        {#if isDuplicate && isRenamingDuplicate}
          <div class="flex min-w-0 flex-1 items-center justify-end gap-1.5">
            <input
              bind:this={renameInput}
              value={renamedFilename}
              oninput={(event) => renamedFilename = event.currentTarget.value}
              onkeydown={(event) => {
                if (event.key === 'Enter') void confirmDuplicateRename();
                if (event.key === 'Escape') isRenamingDuplicate = false;
              }}
              class="h-8 min-w-0 flex-1 rounded border border-input bg-background px-2.5 text-xs text-foreground outline-none transition focus:border-primary"
              aria-label="Renamed filename"
              title={renamedFilename}
            />
            <button
              onclick={() => {
                isRenamingDuplicate = false;
                renamedFilename = '';
              }}
              disabled={isBusy}
              class="h-8 rounded bg-destructive px-3 text-xs font-semibold text-destructive-foreground transition hover:bg-destructive/90 disabled:cursor-not-allowed disabled:opacity-50"
            >
              Cancel
            </button>
            <button
              onclick={() => void confirmDuplicateRename()}
              disabled={isBusy || !trimmedRenamedFilename}
              class="h-8 rounded bg-primary px-4 text-xs font-semibold text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
            >
              Rename
            </button>
          </div>
        {:else}
          <div class="flex min-w-0 items-center justify-end gap-1.5">
            <button
              onclick={() => void runAction(() => cancelDownloadPrompt(activePrompt.id))}
              disabled={isBusy}
              class="h-8 rounded bg-destructive px-3 text-xs font-semibold text-destructive-foreground transition hover:bg-destructive/90 disabled:cursor-not-allowed disabled:opacity-50"
            >
              Cancel
            </button>
            {#if canSwapToBrowser}
              <button
                onclick={() => void runAction(() => swapDownloadPrompt(activePrompt.id))}
                disabled={isBusy}
                class="flex h-8 items-center gap-2 rounded bg-foreground px-3 text-xs font-semibold text-background transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {@render BrowserWindowIcon()}
                Swap
              </button>
            {/if}
            {#if isDuplicate}
              <div class="relative">
                <button
                  onclick={() => duplicateMenuOpen = !duplicateMenuOpen}
                  disabled={isBusy}
                  class="flex h-8 min-w-[118px] items-center justify-center gap-1.5 rounded bg-primary px-3 text-xs font-semibold text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <Download size={14} />
                  Choose Action
                  <ChevronDown size={13} />
                </button>
                {#if duplicateMenuOpen}
                  <div class="absolute bottom-full right-0 z-10 mb-1 w-52 overflow-hidden rounded border border-border bg-popover text-xs text-popover-foreground shadow-xl">
                    <button onclick={() => void confirmDuplicateAction('overwrite')} class="flex w-full items-center justify-between gap-3 px-3 py-2 text-left font-semibold text-warning hover:bg-muted">
                      <span>Overwrite</span>
                      <span class="text-[10px] font-medium text-muted-foreground">{overwriteLabel}</span>
                    </button>
                    <button onclick={startDuplicateRename} class="flex w-full items-center justify-between gap-3 px-3 py-2 text-left font-semibold text-foreground hover:bg-muted">
                      <span>Rename</span>
                      <span class="text-[10px] font-medium text-muted-foreground">edit name</span>
                    </button>
                    <button onclick={() => void confirmDuplicateAction('download_anyway')} class="flex w-full items-center justify-between gap-3 px-3 py-2 text-left font-semibold text-foreground hover:bg-muted">
                      <span>Download Anyway</span>
                      <span class="text-[10px] font-medium text-muted-foreground">copy</span>
                    </button>
                  </div>
                {/if}
              </div>
            {:else}
              <button
                onclick={() => void runAction(() => confirmDownloadPrompt(activePrompt.id, directoryOverride))}
                disabled={isBusy}
                class="flex h-8 min-w-[92px] items-center justify-center gap-2 rounded bg-primary px-4 text-xs font-semibold text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
              >
                <Download size={15} />
                Download
              </button>
            {/if}
          </div>
        {/if}
      </div>
    </main>
  </div>
{/if}

{#snippet BrowserWindowIcon()}
  <svg aria-hidden="true" viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M3 9h18" />
    <path d="M7 6.5h.01" />
    <path d="M10 6.5h.01" />
    <path d="M13 6.5h.01" />
    <path d="M9 14h6" />
    <path d="m13 12 2 2-2 2" />
  </svg>
{/snippet}

{#snippet MetaLabel(icon: typeof Globe, label: string)}
  {@const Icon = icon}
  <div class="flex min-w-0 items-center gap-1.5 text-muted-foreground">
    <span class="shrink-0"><Icon size={13} /></span>
    <span class="truncate">{label}</span>
  </div>
{/snippet}

{#snippet MetaValue(value: string, accent = false)}
  <div class={`min-w-0 truncate ${accent ? 'text-primary' : 'text-foreground'}`} title={value}>{value}</div>
{/snippet}
