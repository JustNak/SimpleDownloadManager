<script lang="ts">
  import { Archive, Check, File, FileAudio, FileImage, FileText, FileVideo, Package, Magnet } from '@lucide/svelte';
  import type { TransferKind } from './types';
  import { fileExtension } from './popupShared';

  interface Props {
    filename: string;
    transferKind?: TransferKind;
    size?: 'sm' | 'md' | 'lg';
    activityState?: 'none' | 'buffering' | 'completed';
  }

  let { filename, transferKind = 'http', size = 'md', activityState = 'none' }: Props = $props();

  const density = $derived(size === 'sm'
    ? { box: 'h-7 w-7', icon: 14, label: 'text-[8px]' }
    : size === 'lg'
      ? { box: 'h-12 w-12', icon: 22, label: 'text-[10px]' }
      : { box: 'h-9 w-9', icon: 18, label: 'text-[9px]' });

  const extension = $derived(fileExtension(filename));
  const Icon = $derived(iconForFilename(filename, transferKind));

  function iconForFilename(name: string, kind: TransferKind) {
    if (kind === 'torrent') return Magnet;
    const ext = fileExtension(name).toLowerCase();
    if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg'].includes(ext)) return FileImage;
    if (['mp4', 'mkv', 'avi', 'mov', 'webm'].includes(ext)) return FileVideo;
    if (['mp3', 'wav', 'flac', 'ogg', 'm4a'].includes(ext)) return FileAudio;
    if (['zip', 'rar', '7z', 'tar', 'gz'].includes(ext)) return Archive;
    if (['exe', 'msi', 'apk', 'dmg'].includes(ext)) return Package;
    if (['pdf', 'doc', 'docx', 'txt', 'md'].includes(ext)) return FileText;
    return File;
  }
</script>

<div class={`file-badge relative flex ${density.box} shrink-0 flex-col items-center justify-center rounded-sm border border-border bg-card text-primary`}>
  <Icon size={density.icon} strokeWidth={2.1} />
  <span class={`mt-0.5 max-w-full truncate px-0.5 font-semibold leading-none text-muted-foreground ${density.label}`}>{extension}</span>
  {#if activityState === 'buffering'}
    <span class="absolute -right-1 -top-1 h-3 w-3 animate-pulse rounded-full border border-background bg-primary"></span>
  {:else if activityState === 'completed'}
    <span class="absolute -right-1 -top-1 flex h-3.5 w-3.5 items-center justify-center rounded-full border border-background bg-success text-white">
      <Check size={9} strokeWidth={3} />
    </span>
  {/if}
</div>
