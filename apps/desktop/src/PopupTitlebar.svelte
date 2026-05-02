<script lang="ts">
  import { Minus, X } from '@lucide/svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { shouldStartWindowDrag } from './windowDrag';

  interface Props {
    title: string;
    onClose?: () => void;
  }

  let { title, onClose }: Props = $props();
  const appWindow = isTauriRuntime() ? getCurrentWindow() : null;

  function startDrag(event: PointerEvent) {
    if (!appWindow || !shouldStartWindowDrag(event)) return;
    event.preventDefault();
    void appWindow.startDragging().catch(() => undefined);
  }

  function closeWindow() {
    if (onClose) {
      onClose();
      return;
    }
    void appWindow?.close();
  }

  function isTauriRuntime(): boolean {
    return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
  }
</script>

<header class="flex h-11 shrink-0 select-none items-center justify-between border-b border-border bg-background text-foreground">
  <div class="flex h-full min-w-0 flex-1 cursor-grab items-center gap-2.5 px-3 active:cursor-grabbing" data-tauri-drag-region role="presentation" onpointerdown={startDrag}>
    <svg aria-hidden="true" viewBox="0 0 24 24" class="pointer-events-none h-5 w-5 shrink-0 text-primary" fill="none">
      <path d="M12 3V15M12 15L7.5 10.5M12 15L16.5 10.5" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" />
      <path d="M5 20C5 20 8 20 12 20C16 20 19 20 19 20" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" opacity="0.8" />
    </svg>
    <span class="pointer-events-none min-w-0 truncate text-sm font-semibold text-foreground">{title}</span>
  </div>
  <div class="flex h-full shrink-0 items-center">
    <button class="flex h-full w-10 items-center justify-center text-muted-foreground transition hover:bg-muted hover:text-foreground" title="Minimize" aria-label="Minimize" onclick={() => void appWindow?.minimize()}><Minus size={16} /></button>
    <button class="flex h-full w-10 items-center justify-center text-muted-foreground transition hover:bg-destructive hover:text-destructive-foreground" title="Close" aria-label="Close" onclick={closeWindow}><X size={16} /></button>
  </div>
</header>
