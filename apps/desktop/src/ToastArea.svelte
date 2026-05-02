<script lang="ts">
  import { AlertCircle, AlertTriangle, CheckCircle, Info, X } from '@lucide/svelte';
  import type { ToastMessage } from './types';

  interface Props {
    toasts: ToastMessage[];
    onRemove: (id: string) => void;
  }

  let { toasts, onRemove }: Props = $props();

  $effect(() => {
    const timers = toasts
      .filter((toast) => toast.autoClose !== false)
      .map((toast) => window.setTimeout(() => onRemove(toast.id), 4200));
    return () => timers.forEach((timer) => window.clearTimeout(timer));
  });

  function iconFor(type: ToastMessage['type']) {
    if (type === 'success') return CheckCircle;
    if (type === 'warning') return AlertTriangle;
    if (type === 'error') return AlertCircle;
    return Info;
  }

  function tone(type: ToastMessage['type']) {
    if (type === 'success') return 'border-success/45 text-success';
    if (type === 'warning') return 'border-warning/45 text-warning';
    if (type === 'error') return 'border-destructive/45 text-destructive';
    return 'border-primary/45 text-primary';
  }
</script>

<div class="pointer-events-none fixed right-4 top-14 z-50 flex w-[360px] max-w-[calc(100vw-2rem)] flex-col gap-2">
  {#each toasts as toast (toast.id)}
    {@const Icon = iconFor(toast.type)}
    <article class={`pointer-events-auto flex gap-2 rounded border bg-popover px-3 py-2 text-popover-foreground shadow-lg ${tone(toast.type)}`}>
      <Icon class="mt-0.5 shrink-0" size={16} />
      <div class="min-w-0 flex-1">
        <div class="truncate text-xs font-semibold">{toast.title}</div>
        <div class="mt-0.5 text-[11px] leading-4 text-muted-foreground">{toast.message}</div>
      </div>
      <button class="shrink-0 rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground" title="Dismiss" onclick={() => onRemove(toast.id)}>
        <X size={13} />
      </button>
    </article>
  {/each}
</div>
