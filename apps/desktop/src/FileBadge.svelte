<script lang="ts">
  import {
    Box,
    Check,
    FileAudio,
    FileArchive,
    FileCode,
    FileImage,
    FileText,
    FileVideo,
    LoaderCircle,
    Magnet,
  } from '@lucide/svelte';
  import type { QueueRowSize, TransferKind } from './types';
  import { fileExtension } from './popupShared';
  import type { FileBadgeActivityState } from './queueRowPresentation';

  interface Props {
    filename: string;
    transferKind?: TransferKind;
    large?: boolean;
    rowSize?: QueueRowSize;
    size?: 'sm' | 'md' | 'lg';
    selected?: boolean;
    selectionTitle?: string;
    onSelectionChange?: (checked: boolean) => void;
    onSelectionPointerDown?: (event: PointerEvent) => void;
    muted?: boolean;
    blurred?: boolean;
    activityState?: FileBadgeActivityState;
  }

  let {
    filename,
    transferKind = 'http',
    large = false,
    rowSize = 'medium',
    size,
    selected = false,
    selectionTitle,
    onSelectionChange,
    onSelectionPointerDown,
    muted = false,
    blurred = false,
    activityState = 'none',
  }: Props = $props();

  const density = $derived(fileBadgeDensity(rowSize, size));
  const iconSize = $derived(large ? 28 : density.iconSize);
  const extension = $derived(fileExtension(filename).toLowerCase());
  const label = $derived(transferKind === 'torrent' ? 'P2P' : extension ? extension.slice(0, 4).toUpperCase() : 'FILE');
  const Icon = $derived(iconForFilename(filename, transferKind));
  const selectable = $derived(!large && Boolean(onSelectionChange));

  function iconForFilename(name: string, kind: TransferKind) {
    if (kind === 'torrent') return Magnet;
    const ext = fileExtension(name).toLowerCase();
    if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg'].includes(ext)) return FileImage;
    if (['mp4', 'mkv', 'avi', 'mov', 'webm'].includes(ext)) return FileVideo;
    if (['mp3', 'wav', 'flac', 'ogg', 'm4a'].includes(ext)) return FileAudio;
    if (['zip', 'rar', '7z', 'tar', 'gz'].includes(ext)) return FileArchive;
    if (['exe', 'msi', 'apk', 'dmg', 'pkg', 'deb'].includes(ext)) return Box;
    if (['js', 'ts', 'json', 'html', 'css'].includes(ext)) return FileCode;
    return FileText;
  }

  function fileBadgeDensity(rowSize: QueueRowSize, popupSize: Props['size']) {
    if (popupSize === 'sm') return { className: 'h-5 w-5', iconSize: 13 };
    if (popupSize === 'lg') return { className: 'h-12 w-12', iconSize: 28 };

    switch (rowSize) {
      case 'compact':
        return { className: 'h-5 w-5', iconSize: 13 };
      case 'small':
        return { className: 'h-6 w-6', iconSize: 15 };
      case 'large':
        return { className: 'h-10 w-10', iconSize: 23 };
      case 'damn':
        return { className: 'h-12 w-12', iconSize: 28 };
      case 'medium':
      default:
        return { className: 'h-7 w-7', iconSize: 18 };
    }
  }
</script>

<div class={`file-badge relative flex shrink-0 items-center justify-center rounded-sm border border-border bg-background ${large ? 'h-[76px] w-14' : density.className}`}>
  <div class={`absolute right-0 top-0 h-2 w-2 border-b border-l border-border bg-surface ${selectable ? 'opacity-0' : ''}`}></div>
  {#if selectable}
    <input
      type="checkbox"
      checked={selected}
      title={selectionTitle ?? 'Select download'}
      aria-label={selectionTitle ?? 'Select download'}
      class="absolute -right-1 -top-1 z-20 h-3.5 w-3.5 shrink-0 cursor-pointer rounded-[2px] accent-primary"
      onclick={(event) => event.stopPropagation()}
      ondblclick={(event) => event.stopPropagation()}
      onpointerdown={(event) => {
        if (onSelectionPointerDown) {
          onSelectionPointerDown(event);
          return;
        }
        event.stopPropagation();
      }}
      oninput={(event) => {
        event.stopPropagation();
        onSelectionChange?.(event.currentTarget.checked);
      }}
    />
  {/if}
  <div class={`${muted ? 'text-muted-foreground' : 'text-primary'} ${blurred ? 'opacity-70 blur-[0.7px]' : ''}`}>
    <Icon size={iconSize} />
  </div>
  {#if activityState === 'buffering'}
    <div class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-sm bg-background/45 text-primary" aria-hidden="true">
      <LoaderCircle size={large ? 20 : 14} strokeWidth={2.4} class="animate-spin" />
    </div>
  {:else if activityState === 'completed'}
    <div class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-sm bg-success/15 text-success animate-[queue-complete-check_1.2s_ease-out_forwards]" aria-hidden="true">
      <Check size={large ? 20 : 14} strokeWidth={2.6} />
    </div>
  {/if}
  {#if large}
    <div class="absolute bottom-1.5 text-[10px] font-semibold text-muted-foreground">{label}</div>
  {/if}
</div>
