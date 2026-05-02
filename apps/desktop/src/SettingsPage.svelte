<script module lang="ts">
  export const SETTINGS_SECTIONS = [
    { id: 'downloads', label: 'Downloads', iconName: 'download' },
    { id: 'torrents', label: 'Torrents', iconName: 'torrent' },
    { id: 'extension', label: 'Extension', iconName: 'extension' },
    { id: 'appearance', label: 'Appearance', iconName: 'appearance' },
    { id: 'startup', label: 'Startup', iconName: 'startup' },
  ] as const;

  export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number]['id'];
</script>

<script lang="ts">
  import type { Component } from 'svelte';
  import { Download, FolderOpen, Palette, PlugZap, Save, Shield, Wrench } from '@lucide/svelte';
  import type { QueueRowSize, Settings } from './types';
  import { DEFAULT_ACCENT_COLOR, normalizeAccentColor } from './appearance';
  import ToggleControl from './ToggleControl.svelte';

  type IconComponent = Component<{ size?: number; class?: string }>;

  interface Props {
    settings: Settings;
    activeSectionId: SettingsSectionId;
    isSaving: boolean;
    onActiveSectionChange: (id: SettingsSectionId) => void;
    onSave: (settings: Settings) => void;
    onDirtyChange: (dirty: boolean, draft: Settings | null) => void;
    onBrowseDirectory: () => Promise<string | null>;
    onClearTorrentSessionCache: () => void;
  }

  let {
    settings,
    activeSectionId,
    isSaving,
    onActiveSectionChange,
    onSave,
    onDirtyChange,
    onBrowseDirectory,
    onClearTorrentSessionCache,
  }: Props = $props();

  let formData = $state<Settings>(initialSettings());
  let excludedHostInput = $state('');
  let accentColorInput = $state(initialAccentColor());

  const isDirty = $derived(JSON.stringify(formData) !== JSON.stringify(settings));

  $effect(() => {
    formData = cloneSettings(settings);
    accentColorInput = normalizeAccentColor(settings.accentColor);
  });

  $effect(() => {
    onDirtyChange(isDirty, isDirty ? cloneSettings(formData) : null);
  });

  function cloneSettings(value: Settings): Settings {
    return structuredClone($state.snapshot(value));
  }

  function initialSettings(): Settings {
    return cloneSettings(settings);
  }

  function initialAccentColor(): string {
    return normalizeAccentColor(settings.accentColor);
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    onSave({
      ...cloneSettings(formData),
      accentColor: normalizeAccentColor(accentColorInput),
    });
  }

  async function browseDownloadDirectory() {
    const selected = await onBrowseDirectory();
    if (selected) formData.downloadDirectory = selected;
  }

  function addExcludedHost() {
    const host = excludedHostInput.trim().toLowerCase();
    if (!host || formData.extensionIntegration.excludedHosts.includes(host)) return;
    formData.extensionIntegration.excludedHosts = [...formData.extensionIntegration.excludedHosts, host];
    excludedHostInput = '';
  }

  function removeExcludedHost(host: string) {
    formData.extensionIntegration.excludedHosts = formData.extensionIntegration.excludedHosts.filter((item) => item !== host);
  }

  function sectionIcon(iconName: string): IconComponent {
    if (iconName === 'download') return Download;
    if (iconName === 'torrent') return Shield;
    if (iconName === 'extension') return PlugZap;
    if (iconName === 'appearance') return Palette;
    return Wrench;
  }

  const queueRowSizes: Array<{ value: QueueRowSize; label: string }> = [
    { value: 'compact', label: 'Compact' },
    { value: 'small', label: 'Small' },
    { value: 'medium', label: 'Medium' },
    { value: 'large', label: 'Large' },
    { value: 'damn', label: 'DAMN' },
  ];
</script>

<form class="settings-surface flex min-h-0 flex-1 overflow-hidden bg-background" onsubmit={submit}>
  <aside class="w-56 shrink-0 border-r border-border bg-sidebar p-3">
    <div class="mb-3 text-xs font-semibold uppercase text-muted-foreground">Settings</div>
    <nav class="space-y-1">
      {#each SETTINGS_SECTIONS as section (section.id)}
        {@const Icon = sectionIcon(section.iconName)}
        <button
          type="button"
          class={`flex w-full items-center gap-2 rounded px-2.5 py-2 text-left text-sm ${activeSectionId === section.id ? 'bg-selected text-foreground' : 'text-muted-foreground hover:bg-muted hover:text-foreground'}`}
          onclick={() => onActiveSectionChange(section.id)}
        >
          <Icon size={16} /> {section.label}
        </button>
      {/each}
    </nav>
  </aside>

  <main class="min-h-0 flex-1 overflow-y-auto p-6">
    <div class="mx-auto max-w-4xl">
      <div class="mb-5 flex items-center justify-between">
        <div>
          <h1 class="text-xl font-semibold">Settings</h1>
          <p class="mt-1 text-sm text-muted-foreground">Manage downloads, torrents, browser handoff, and appearance.</p>
        </div>
        <button class="inline-flex items-center gap-2 rounded bg-primary px-3 py-2 text-xs font-semibold text-primary-foreground disabled:opacity-50" disabled={!isDirty || isSaving}>
          <Save size={15} /> {isSaving ? 'Saving...' : 'Save changes'}
        </button>
      </div>

      {#if activeSectionId === 'downloads'}
        <section class="rounded border border-border bg-card p-5 text-card-foreground shadow-sm">
          <div class="mb-4 flex items-center gap-2">
            <Download size={18} class="text-primary" />
            <h2 class="text-base font-semibold">Download behavior</h2>
          </div>
          <div class="space-y-4">
            <label class="block">
              <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Download directory</span>
              <div class="flex gap-2">
                <input class="min-w-0 flex-1 rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.downloadDirectory} />
                <button type="button" class="inline-flex items-center gap-2 rounded border border-border px-3 text-xs font-semibold hover:bg-muted" onclick={() => void browseDownloadDirectory()}><FolderOpen size={15} /> Browse</button>
              </div>
            </label>
            <div class="grid grid-cols-2 gap-4">
              <label class="block">
                <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Concurrent downloads</span>
                <input type="number" min="1" max="16" class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.maxConcurrentDownloads} />
              </label>
              <label class="block">
                <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Auto retry attempts</span>
                <input type="number" min="0" max="20" class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.autoRetryAttempts} />
              </label>
              <label class="block">
                <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Speed limit (KiB/s)</span>
                <input type="number" min="0" class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.speedLimitKibPerSecond} />
              </label>
              <label class="block">
                <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Queue row size</span>
                <select class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.queueRowSize}>
                  {#each queueRowSizes as option (option.value)}<option value={option.value}>{option.label}</option>{/each}
                </select>
              </label>
            </div>
            <ToggleControl label="Show details on click" bind:checked={formData.showDetailsOnClick} />
            <ToggleControl label="Enable notifications" bind:checked={formData.notificationsEnabled} />
          </div>
        </section>
      {:else if activeSectionId === 'torrents'}
        <section class="rounded border border-border bg-card p-5 text-card-foreground shadow-sm">
          <div class="mb-4 flex items-center gap-2">
            <Shield size={18} class="text-primary" />
            <h2 class="text-base font-semibold">Torrent engine</h2>
          </div>
          <div class="space-y-4">
            <ToggleControl label="Enable torrent downloads" bind:checked={formData.torrent.enabled} />
            <label class="block">
              <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Torrent directory</span>
              <input class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.torrent.downloadDirectory} />
            </label>
            <div class="grid grid-cols-2 gap-4">
              <label class="block">
                <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Seed mode</span>
                <select class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.torrent.seedMode}>
                  <option value="forever">Forever</option>
                  <option value="ratio">Ratio</option>
                  <option value="time">Time</option>
                  <option value="ratio_or_time">Ratio or time</option>
                </select>
              </label>
              <label class="block">
                <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Seed ratio</span>
                <input type="number" step="0.1" min="0" class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.torrent.seedRatioLimit} />
              </label>
              <label class="block">
                <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Seed time minutes</span>
                <input type="number" min="0" class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.torrent.seedTimeLimitMinutes} />
              </label>
              <label class="block">
                <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Upload limit KiB/s</span>
                <input type="number" min="0" class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.torrent.uploadLimitKibPerSecond} />
              </label>
            </div>
            <ToggleControl label="Enable port forwarding" bind:checked={formData.torrent.portForwardingEnabled} />
            <button type="button" class="mt-3 rounded border border-border px-3 py-2 text-xs font-semibold hover:bg-muted" onclick={onClearTorrentSessionCache}>Clear torrent session cache</button>
          </div>
        </section>
      {:else if activeSectionId === 'extension'}
        <section class="rounded border border-border bg-card p-5 text-card-foreground shadow-sm">
          <div class="mb-4 flex items-center gap-2">
            <PlugZap size={18} class="text-primary" />
            <h2 class="text-base font-semibold">Browser integration</h2>
          </div>
          <div class="space-y-4">
            <ToggleControl label="Enable extension integration" bind:checked={formData.extensionIntegration.enabled} />
            <div class="grid grid-cols-2 gap-4">
              <label class="block">
                <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Handoff mode</span>
                <select class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.extensionIntegration.downloadHandoffMode}>
                  <option value="ask">Ask</option>
                  <option value="auto">Auto</option>
                  <option value="off">Off</option>
                </select>
              </label>
              <label class="block">
                <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Listen port</span>
                <input type="number" min="1" max="65535" class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.extensionIntegration.listenPort} />
              </label>
            </div>
            <ToggleControl label="Show context menu" bind:checked={formData.extensionIntegration.contextMenuEnabled} />
            <ToggleControl label="Show progress after handoff" bind:checked={formData.extensionIntegration.showProgressAfterHandoff} />
            <ToggleControl label="Show badge status" bind:checked={formData.extensionIntegration.showBadgeStatus} />
            <ToggleControl label="Authenticated handoff" bind:checked={formData.extensionIntegration.authenticatedHandoffEnabled} />
            <label class="block">
              <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Excluded hosts</span>
              <div class="flex gap-2">
                <input class="min-w-0 flex-1 rounded border border-input bg-background px-3 py-2 text-sm" bind:value={excludedHostInput} placeholder="example.com" />
                <button type="button" class="rounded border border-border px-3 text-xs font-semibold hover:bg-muted" onclick={addExcludedHost}>Add</button>
              </div>
              <div class="mt-2 flex flex-wrap gap-2">
                {#each formData.extensionIntegration.excludedHosts as host}
                  <button type="button" class="rounded border border-border bg-background px-2 py-1 text-xs hover:bg-muted" onclick={() => removeExcludedHost(host)}>{host} x</button>
                {/each}
              </div>
            </label>
          </div>
        </section>
      {:else if activeSectionId === 'appearance'}
        <section class="rounded border border-border bg-card p-5 text-card-foreground shadow-sm">
          <div class="mb-4 flex items-center gap-2">
            <Palette size={18} class="text-primary" />
            <h2 class="text-base font-semibold">Appearance</h2>
          </div>
          <div class="grid grid-cols-2 gap-4">
            <label class="block">
              <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Theme</span>
              <select class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.theme}>
                <option value="system">System</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
                <option value="oled_dark">OLED dark</option>
              </select>
            </label>
            <label class="block">
              <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Accent color</span>
              <div class="flex gap-2">
                <input type="color" class="h-10 w-12 rounded border border-input bg-background p-1" bind:value={accentColorInput} />
                <input class="min-w-0 flex-1 rounded border border-input bg-background px-3 py-2 text-sm font-mono" bind:value={accentColorInput} placeholder={DEFAULT_ACCENT_COLOR} />
              </div>
            </label>
          </div>
        </section>
      {:else}
        <section class="rounded border border-border bg-card p-5 text-card-foreground shadow-sm">
          <div class="mb-4 flex items-center gap-2">
            <Wrench size={18} class="text-primary" />
            <h2 class="text-base font-semibold">Startup</h2>
          </div>
          <div class="space-y-4">
            <ToggleControl label="Start on system startup" bind:checked={formData.startOnStartup} />
            <label class="block">
              <span class="mb-1.5 block text-xs font-semibold text-muted-foreground">Launch mode</span>
              <select class="w-full rounded border border-input bg-background px-3 py-2 text-sm" bind:value={formData.startupLaunchMode}>
                <option value="open">Open main window</option>
                <option value="tray">Start in tray</option>
              </select>
            </label>
          </div>
        </section>
      {/if}
    </div>
  </main>
</form>
