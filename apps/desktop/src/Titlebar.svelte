<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Copy, Minus, Square, X } from '@lucide/svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { shouldStartWindowDrag } from './windowDrag';
  import appIconUrl from '../src-tauri/icons/icon.svg';

  interface Props {
    title?: string;
    children?: Snippet;
  }

  let { title = 'SDM', children }: Props = $props();
  let isMaximized = $state(false);
  const appWindow = isTauriRuntime() ? getCurrentWindow() : null;

  async function refreshMaximized() {
    if (!appWindow) return;
    isMaximized = await appWindow.isMaximized().catch(() => false);
  }

  async function startDrag(event: PointerEvent) {
    if (!appWindow || !shouldStartWindowDrag(event)) return;
    await appWindow.startDragging().catch(() => undefined);
  }

  async function toggleMaximize(event?: MouseEvent) {
    if (event && !shouldStartWindowDrag(event)) return;
    if (!appWindow) return;
    await appWindow.toggleMaximize().catch(() => undefined);
    await refreshMaximized();
  }

  $effect(() => {
    void refreshMaximized();
    if (!appWindow) return;

    let unlisten: (() => void) | undefined;
    void appWindow.onResized(async () => {
      isMaximized = await appWindow.isMaximized().catch(() => false);
    }).then((dispose) => {
      unlisten = dispose;
    });

    return () => {
      unlisten?.();
    };
  });

  function isTauriRuntime(): boolean {
    return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
  }
</script>

<header class="titlebar z-50 flex h-11 w-full shrink-0 select-none items-center justify-between border-b border-border bg-background text-foreground">
  <div
    class="flex h-full w-[220px] shrink-0 cursor-grab items-center gap-2.5 border-r border-border px-3 active:cursor-grabbing"
    data-tauri-drag-region
    role="presentation"
    onpointerdown={startDrag}
    ondblclick={toggleMaximize}
  >
    <img src={appIconUrl} alt="" aria-hidden="true" class="pointer-events-none h-5 w-5 rounded-[5px]" />
    <span class="pointer-events-none text-sm font-semibold text-foreground">{title}</span>
  </div>

  <div class="flex h-full min-w-0 flex-1 items-center px-4" data-tauri-drag-region role="presentation" onpointerdown={startDrag} ondblclick={toggleMaximize}>
    {#if children}
      <div class="min-w-0 flex-1 cursor-grab active:cursor-grabbing">{@render children()}</div>
    {:else}
      <div class="h-full flex-1 cursor-grab active:cursor-grabbing"></div>
    {/if}
  </div>

  <div class="flex h-full shrink-0 items-center">
    <button class="flex h-full w-10 items-center justify-center text-foreground transition-colors hover:bg-muted" title="Minimize" aria-label="Minimize" onclick={() => void appWindow?.minimize()}><Minus size={16} /></button>
    <button class="flex h-full w-10 items-center justify-center text-foreground transition-colors hover:bg-muted" title={isMaximized ? 'Restore Down' : 'Maximize'} aria-label={isMaximized ? 'Restore Down' : 'Maximize'} onclick={() => void toggleMaximize()}>
      {#if isMaximized}<Copy size={14} class="rotate-180" />{:else}<Square size={14} />{/if}
    </button>
    <button class="flex h-full w-10 items-center justify-center text-foreground transition-colors hover:bg-destructive hover:text-destructive-foreground" title="Close" aria-label="Close" onclick={() => void appWindow?.close()}><X size={16} /></button>
  </div>
</header>
