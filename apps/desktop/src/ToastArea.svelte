<script lang="ts">
  import { AlertCircle, AlertTriangle, CheckCircle, Info, X } from '@lucide/svelte';
  import type { ToastMessage } from './types';

  interface Props {
    toasts: ToastMessage[];
    onRemove: (id: string) => void;
  }

  let { toasts, onRemove }: Props = $props();

  const TOAST_AUTO_CLOSE_MS = 3000;

  $effect(() => {
    const timers = toasts
      .filter((toast) => toast.autoClose !== false)
      .map((toast) => window.setTimeout(() => onRemove(toast.id), TOAST_AUTO_CLOSE_MS));
    return () => timers.forEach((timer) => window.clearTimeout(timer));
  });

  function configFor(type: ToastMessage['type']) {
    if (type === 'success') return { icon: CheckCircle, iconClass: 'text-green-500', border: 'border-green-500/20' };
    if (type === 'warning') return { icon: AlertTriangle, iconClass: 'text-yellow-500', border: 'border-yellow-500/20' };
    if (type === 'error') return { icon: AlertCircle, iconClass: 'text-red-500', border: 'border-red-500/20' };
    return { icon: Info, iconClass: 'text-primary', border: 'border-primary/20' };
  }
</script>

{#if toasts.length > 0}
<div class="fixed bottom-20 right-6 z-50 flex w-full max-w-sm flex-col gap-3 pointer-events-none">
  {#each toasts as toast (toast.id)}
    {@const config = configFor(toast.type)}
    {@const Icon = config.icon}
    <article class={`pointer-events-auto flex items-start gap-4 rounded-md border bg-card p-4 shadow-lg animate-in slide-in-from-bottom-5 fade-in duration-300 ${config.border}`}>
      <div class="mt-0.5 shrink-0">
        <Icon size={20} class={config.iconClass} />
      </div>
      <div class="min-w-0 flex-1">
        <h4 class="mb-1 text-sm font-semibold leading-none text-foreground">{toast.title}</h4>
        <p class="break-words text-sm leading-snug text-muted-foreground">{toast.message}</p>
      </div>
      <button class="-mr-1 -mt-1 shrink-0 rounded-md p-1 text-muted-foreground transition-colors hover:bg-muted focus:outline-none focus:ring-2 focus:ring-primary" title="Dismiss" aria-label="Dismiss" onclick={() => onRemove(toast.id)}>
        <X size={16} />
      </button>
    </article>
  {/each}
</div>
{/if}
