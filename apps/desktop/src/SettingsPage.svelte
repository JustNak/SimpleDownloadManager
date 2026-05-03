<script lang="ts">
  import { untrack, type Component, type Snippet } from 'svelte';
  import {
    Ban,
    Bell,
    CheckCircle2,
    Clock3,
    Download,
    ExternalLink,
    FolderOpen,
    Gauge,
    Globe,
    Palette,
    Plus,
    PlugZap,
    RotateCw,
    Save,
    Search,
    Settings2,
    ShieldAlert,
    ShieldCheck,
    ShieldX,
    Trash2,
    Upload,
    Wrench,
    X,
  } from '@lucide/svelte';
  import type { DiagnosticsSnapshot, QueueRowSize, Settings } from './types';
  import type { AppUpdateState, AppUpdateVersionTone } from './appUpdates';
  import { DEFAULT_ACCENT_COLOR, normalizeAccentColor } from './appearance';
  import { defaultTorrentDownloadDirectory, normalizeTorrentSettings } from './torrentSettings';
  import { settingsEqual, shouldAdoptIncomingSettingsDraft } from './settingsDraftSync';
  import {
    addExcludedHosts,
    filterExcludedHosts,
    formatExcludedSitesSummary,
    normalizeHostInput,
    parseExcludedHostInput,
    removeExcludedHost,
  } from './settingsExcludedSites';

  type IconComponent = Component<{ size?: number; class?: string; strokeWidth?: number }>;

  interface Props {
    settings: Settings;
    diagnostics: DiagnosticsSnapshot | null;
    hasActiveTorrentJobs: boolean;
    isSaving: boolean;
    onSave: (settings: Settings) => void | Promise<void | boolean>;
    onCancel: () => void;
    onDirtyChange: (dirty: boolean, draft: Settings | null) => void;
    onBrowseDirectory: () => Promise<string | null | void>;
    onClearTorrentSessionCache: () => void | Promise<void>;
    onRefreshDiagnostics: () => void;
    onOpenInstallDocs: () => void;
    onRunHostRegistrationFix: () => void;
    onTestExtensionHandoff: () => void;
    onCopyDiagnostics: () => void;
    onExportDiagnostics: () => void;
    updateState: AppUpdateState;
    onCheckForUpdates: () => void;
    onInstallUpdate: () => void;
  }

  let {
    settings,
    diagnostics,
    hasActiveTorrentJobs,
    isSaving,
    onSave,
    onCancel,
    onDirtyChange,
    onBrowseDirectory,
    onClearTorrentSessionCache,
    onRefreshDiagnostics,
    onOpenInstallDocs,
    onRunHostRegistrationFix,
    onTestExtensionHandoff,
    onCopyDiagnostics,
    onExportDiagnostics,
    updateState,
    onCheckForUpdates,
    onInstallUpdate,
  }: Props = $props();

  let formData = $state<Settings>(initialSettings());
  let previousSettings = initialSettings();
  let lastReportedDirty = false;
  let lastReportedDraftKey = '';
  let excludedHostInput = $state('');
  let excludedBulkInput = $state('');
  let excludedSearchQuery = $state('');
  let isExcludedSitesDialogOpen = $state(false);
  let accentColorInput = $state(initialAccentColor());
  let isClearingTorrentSessionCache = $state(false);

  const isDirty = $derived(!settingsEqual(completeSettingsDraft(), settings));
  const excludedHosts = $derived(formData.extensionIntegration.excludedHosts);
  const filteredExcludedHosts = $derived(filterExcludedHosts(excludedHosts, excludedSearchQuery));
  const updateIsBusy = $derived(updateState.status === 'checking' || updateState.status === 'downloading' || updateState.status === 'installing');

  $effect(() => {
    const nextSettings = settings;
    untrack(() => {
      const previous = previousSettings;
      previousSettings = cloneSettings(nextSettings);

      if (!shouldAdoptIncomingSettingsDraft(formData, previous, nextSettings)) {
        return;
      }

      formData = cloneSettings(nextSettings);
      excludedHostInput = '';
      excludedBulkInput = '';
      excludedSearchQuery = '';
      isExcludedSitesDialogOpen = false;
      accentColorInput = normalizeAccentColor(nextSettings.accentColor);
    });
  });

  $effect(() => {
    const dirty = isDirty;
    const draft = dirty ? completeSettingsDraft() : null;
    const draftKey = draft ? JSON.stringify(draft) : '';
    if (dirty === lastReportedDirty && draftKey === lastReportedDraftKey) {
      return;
    }

    lastReportedDirty = dirty;
    lastReportedDraftKey = draftKey;
    untrack(() => onDirtyChange(dirty, draft));
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

  function completeSettingsDraft(): Settings {
    return {
      ...cloneSettings(formData),
      accentColor: normalizeAccentColor(accentColorInput),
    };
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    const draft = completeSettingsDraft();
    onSave({
      ...draft,
      torrent: normalizeTorrentSettings(draft.torrent, draft.downloadDirectory),
      extensionIntegration: {
        ...draft.extensionIntegration,
        listenPort: normalizeListenPort(String(draft.extensionIntegration.listenPort)),
      },
    });
  }

  async function browseDownloadDirectory() {
    const selected = await onBrowseDirectory();
    if (!selected) return;

    const previousTorrentDirectory = defaultTorrentDownloadDirectory(formData.downloadDirectory);
    const shouldUpdateTorrentDirectory = !formData.torrent.downloadDirectory
      || formData.torrent.downloadDirectory === previousTorrentDirectory;

    formData.downloadDirectory = selected;
    if (shouldUpdateTorrentDirectory) {
      updateTorrentSettings({ downloadDirectory: defaultTorrentDownloadDirectory(selected) });
    }
  }

  async function browseTorrentDirectory() {
    const selected = await onBrowseDirectory();
    if (selected) updateTorrentSettings({ downloadDirectory: selected });
  }

  async function clearTorrentSessionCache() {
    isClearingTorrentSessionCache = true;
    try {
      await onClearTorrentSessionCache();
    } finally {
      isClearingTorrentSessionCache = false;
    }
  }

  function updateTorrentSettings(update: Partial<Settings['torrent']>) {
    formData.torrent = normalizeTorrentSettings({ ...formData.torrent, ...update }, formData.downloadDirectory);
  }

  function updateExtensionIntegration(update: Partial<Settings['extensionIntegration']>) {
    formData.extensionIntegration = {
      ...formData.extensionIntegration,
      ...update,
    };
  }

  function addExcludedHost() {
    const normalized = normalizeHostInput(excludedHostInput);
    if (!normalized) return;
    const result = addExcludedHosts(excludedHosts, [normalized]);
    updateExtensionIntegration({ excludedHosts: result.hosts });
    excludedHostInput = '';
  }

  function addExcludedBulkHosts() {
    const candidates = parseExcludedHostInput(excludedBulkInput);
    if (candidates.length === 0) return;
    const result = addExcludedHosts(excludedHosts, candidates);
    updateExtensionIntegration({ excludedHosts: result.hosts });
    excludedBulkInput = '';
  }

  function removeExcludedSite(host: string) {
    updateExtensionIntegration({ excludedHosts: removeExcludedHost(excludedHosts, host) });
  }

  function normalizeListenPort(value: string): number {
    const port = Number.parseInt(value, 10);
    return Number.isFinite(port) && port >= 1 && port <= 65535 ? port : 1420;
  }

  function normalizeTorrentPort(value: string): number {
    const port = Number.parseInt(value, 10);
    return Number.isFinite(port) && port >= 1024 && port <= 65534 ? port : 42000;
  }

  function normalizeInteger(value: string, fallback: number): number {
    const parsed = Number.parseInt(value, 10);
    return Number.isFinite(parsed) ? parsed : fallback;
  }

  function normalizeNumber(value: string, fallback: number): number {
    const parsed = Number.parseFloat(value);
    return Number.isFinite(parsed) ? parsed : fallback;
  }

  function usesTorrentRatioLimit(mode: Settings['torrent']['seedMode']) {
    return mode === 'ratio' || mode === 'ratio_or_time';
  }

  function usesTorrentTimeLimit(mode: Settings['torrent']['seedMode']) {
    return mode === 'time' || mode === 'ratio_or_time';
  }

  function renderUpdateStatus(state: AppUpdateState): string {
    if (state.status === 'checking') return 'Checking GitHub Releases for a newer beta build.';
    if (state.status === 'available' && state.availableUpdate) return `Version ${state.availableUpdate.version} is available.`;
    if (state.status === 'not_available') return 'You are running the latest beta build.';
    if (state.status === 'downloading') return 'Downloading the signed update package.';
    if (state.status === 'installing') return 'Installing the update. The app may close automatically.';
    if (state.status === 'error') return 'The last update action failed.';
    return 'Checks the signed beta feed hosted on GitHub Releases.';
  }

  function versionIndicatorToneClass(tone: AppUpdateVersionTone): string {
    switch (tone) {
      case 'available':
        return 'text-primary';
      case 'error':
        return 'text-destructive';
      case 'pending':
        return 'text-muted-foreground';
      default:
        return 'text-foreground';
    }
  }

  function updateProgressPercent(state: AppUpdateState): number {
    if (!state.totalBytes || state.totalBytes <= 0) return 0;
    return Math.max(0, Math.min(100, (state.downloadedBytes / state.totalBytes) * 100));
  }

  function formatUpdateProgress(state: AppUpdateState): string {
    if (!state.totalBytes) return `${formatCompactBytes(state.downloadedBytes)} downloaded`;
    return `${formatCompactBytes(state.downloadedBytes)} / ${formatCompactBytes(state.totalBytes)}`;
  }

  function formatCompactBytes(value: number): string {
    if (!Number.isFinite(value) || value <= 0) return '0 B';
    const units = ['B', 'KiB', 'MiB', 'GiB'];
    let unitIndex = 0;
    let nextValue = value;
    while (nextValue >= 1024 && unitIndex < units.length - 1) {
      nextValue /= 1024;
      unitIndex += 1;
    }
    return `${nextValue >= 10 || unitIndex === 0 ? nextValue.toFixed(0) : nextValue.toFixed(1)} ${units[unitIndex]}`;
  }

  function renderRegistrationIcon(status?: DiagnosticsSnapshot['hostRegistration']['status']): IconComponent {
    switch (status) {
      case 'configured':
        return ShieldCheck;
      case 'broken':
        return ShieldAlert;
      default:
        return ShieldX;
    }
  }

  function renderRegistrationMessage(status?: DiagnosticsSnapshot['hostRegistration']['status']) {
    switch (status) {
      case 'configured':
        return 'At least one browser has a valid native host registration and host binary path.';
      case 'broken':
        return 'A browser registration exists, but the manifest or native host binary path is broken.';
      case 'missing':
        return 'No browser registration was detected for the native messaging host.';
      default:
        return 'Diagnostics are still loading.';
    }
  }

  function registrationStatusLabel(status?: DiagnosticsSnapshot['hostRegistration']['status']) {
    switch (status) {
      case 'configured':
        return 'Ready';
      case 'broken':
        return 'Repair';
      case 'missing':
        return 'Missing';
      default:
        return 'Checking';
    }
  }

  function registrationBadgeClass(status?: DiagnosticsSnapshot['hostRegistration']['status']) {
    switch (status) {
      case 'configured':
        return 'bg-success/10 text-success';
      case 'broken':
        return 'bg-warning/10 text-warning';
      case 'missing':
        return 'bg-destructive/10 text-destructive';
      default:
        return 'bg-muted text-muted-foreground';
    }
  }

  function diagnosticLevelConsoleClass(level: DiagnosticsSnapshot['recentEvents'][number]['level']) {
    switch (level) {
      case 'error':
        return 'text-red-300';
      case 'warning':
        return 'text-amber-300';
      default:
        return 'text-emerald-300';
    }
  }

  function formatDiagnosticEventTime(timestamp: number) {
    if (!Number.isFinite(timestamp) || timestamp <= 0) return 'Unknown time';
    return new Intl.DateTimeFormat(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(timestamp));
  }

  const queueRowSizes: Array<{ value: QueueRowSize; label: string }> = [
    { value: 'compact', label: 'Compact' },
    { value: 'small', label: 'Small' },
    { value: 'medium', label: 'Medium' },
    { value: 'large', label: 'Large' },
    { value: 'damn', label: 'DAMN' },
  ];

  const accentPresets = [
    DEFAULT_ACCENT_COLOR,
    '#2563eb',
    '#06b6d4',
    '#14b8a6',
    '#22c55e',
    '#84cc16',
    '#eab308',
    '#f97316',
    '#e11d48',
    '#ec4899',
    '#a855f7',
    '#6366f1',
  ];

  const accentGradientPresets = [
    {
      name: 'Aurora',
      value: '#22c55e',
      gradient: 'linear-gradient(135deg, #06b6d4 0%, #22c55e 52%, #a3e635 100%)',
    },
    {
      name: 'Ember',
      value: '#f97316',
      gradient: 'linear-gradient(135deg, #f43f5e 0%, #f97316 48%, #facc15 100%)',
    },
    {
      name: 'Violet',
      value: '#a855f7',
      gradient: 'linear-gradient(135deg, #6366f1 0%, #a855f7 52%, #ec4899 100%)',
    },
    {
      name: 'Lagoon',
      value: '#06b6d4',
      gradient: 'linear-gradient(135deg, #2563eb 0%, #06b6d4 46%, #14b8a6 100%)',
    },
  ];
</script>

{#snippet generalContent()}
  <div>
    {@render FieldRow('Download Directory', 'Default save path.', directoryControl)}
    {@render FieldRow('Max Concurrent Downloads', 'Active job limit.', maxConcurrentControl)}
    {@render FieldRow('Auto Retry Attempts', 'Failure retries.', autoRetryControl)}
    {@render FieldRow('Per-Download Speed Limit', 'Transfer cap.', speedLimitControl)}
    {@render FieldRow('Download Performance', 'Connection strategy.', performanceControl)}
  </div>
{/snippet}

{#snippet updateContent()}
  <div class="space-y-4">
    <div class="flex items-start justify-between gap-3">
      <div class="min-w-0">
        <div class="font-semibold text-foreground">Beta channel updates</div>
        <div class="mt-1 text-sm leading-6 text-muted-foreground">{renderUpdateStatus(updateState)}</div>
      </div>
      <button type="button" onclick={onCheckForUpdates} disabled={updateIsBusy} class="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50">
        <RotateCw size={16} class={updateState.status === 'checking' ? 'animate-spin' : ''} />
        Check
      </button>
    </div>

    <div class="grid gap-3 border-y border-border/40 py-3 md:grid-cols-2">
      {@render VersionIndicator('Current', updateState.availableUpdate?.currentVersion ?? '0.5.0-beta', 'current')}
      {@render VersionIndicator('Latest', updateState.availableUpdate?.version ?? (updateState.status === 'checking' ? 'Checking...' : updateState.status === 'error' ? 'Unavailable' : 'Check pending'), updateState.availableUpdate ? 'available' : updateState.status === 'error' ? 'error' : 'pending')}
    </div>

    {#if updateState.status === 'downloading' || updateState.status === 'installing'}
      <div>
        <div class="mb-1 flex items-center justify-between text-xs text-muted-foreground">
          <span>{updateState.status === 'installing' ? 'Installing update' : 'Downloading update'}</span>
          <span>{formatUpdateProgress(updateState)}</span>
        </div>
        <div class="h-2 overflow-hidden rounded-full bg-progress-track">
          <div class="h-full rounded-full bg-primary" style={`width: ${updateProgressPercent(updateState)}%;`}></div>
        </div>
      </div>
    {/if}

    {#if updateState.availableUpdate}
      <div class="flex justify-end">
        <button type="button" onclick={onInstallUpdate} disabled={updateIsBusy} class="flex h-9 items-center gap-2 rounded-md bg-primary px-3 text-sm font-medium text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50">
          <Download size={16} />
          Install Update
        </button>
      </div>
    {:else if updateState.errorMessage}
      <div class="border-l-2 border-destructive/60 bg-destructive/10 px-3 py-2 text-sm text-destructive">{updateState.errorMessage}</div>
    {/if}
  </div>
{/snippet}

{#snippet torrentingContent()}
  <div>
    {@render SwitchFieldRow(Gauge, 'Enable torrent downloads', 'Allow magnet and .torrent transfers.', torrentEnabledControl)}
    {@render FieldRow('Torrent Directory', 'Default save path for torrents.', torrentDirectoryControl)}
    {@render FieldRow('Seed Mode', 'Stop policy after completion.', seedModeControl)}
    {#if usesTorrentRatioLimit(formData.torrent.seedMode)}
      {@render FieldRow('Seed Ratio Limit', 'Target share ratio.', seedRatioControl)}
    {/if}
    {#if usesTorrentTimeLimit(formData.torrent.seedMode)}
      {@render FieldRow('Seed Time Limit', 'Minutes to seed.', seedTimeControl)}
    {/if}
    {@render FieldRow('Upload Limit', 'Seeding cap.', uploadLimitControl)}
    {@render SwitchFieldRow(Globe, 'Port forwarding', 'Use the configured listen port when available.', portForwardingControl)}
    {#if formData.torrent.portForwardingEnabled}
      {@render FieldRow('Forwarded Port', 'Router listen port.', forwardingPortControl)}
    {/if}
    {@render FieldRow('Peer Watchdog', 'Peer connection diagnostics.', peerWatchdogControl)}
    <div class="grid grid-cols-[minmax(160px,220px)_minmax(0,1fr)] items-center gap-4 border-t border-border/35 py-3">
      <div class="text-sm text-muted-foreground">
        {hasActiveTorrentJobs ? 'Active torrents are running. Clearing the cache may require a restart.' : 'Clear stale torrent engine session data.'}
      </div>
      <div class="flex justify-start">
        <button type="button" onclick={() => void clearTorrentSessionCache()} disabled={isClearingTorrentSessionCache} class="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50">
          <RotateCw size={16} class={isClearingTorrentSessionCache ? 'animate-spin' : ''} />
          Clear torrent session cache
        </button>
      </div>
    </div>
  </div>
{/snippet}

{#snippet appearanceContent()}
  <div>
    {@render FieldRow('Theme', 'Application color scheme.', themeControl)}
    {@render FieldRow('Accent Color', 'Primary highlight color.', accentControl, 'Primary highlight color.', true)}
    {@render FieldRow('Queue Row Size', 'Main list density.', rowSizeControl)}
    {@render SwitchFieldRow(Bell, 'Notifications', 'Show desktop notifications for completed or failed downloads.', notificationsControl)}
    {@render SwitchFieldRow(Clock3, 'Show details on click', 'Selecting a row opens the details pane.', showDetailsControl)}
  </div>
{/snippet}

{#snippet extensionContent()}
  <div>
    {@render SwitchFieldRow(PlugZap, 'Enable extension integration', 'Accept browser handoff requests.', extensionEnabledControl)}
    {@render FieldRow('Handoff Mode', 'How browser downloads are handled.', handoffModeControl)}
    {@render FieldRow('Listen Port', 'Extension bridge port.', listenPortControl)}
    {@render SwitchFieldRow(Globe, 'Context menu', 'Show Send to Simple Download Manager in the browser.', contextMenuControl)}
    {@render SwitchFieldRow(Download, 'Progress after handoff', 'Open a progress window after accepting a browser download.', progressAfterHandoffControl)}
    {@render SwitchFieldRow(CheckCircle2, 'Badge status', 'Show extension status in the browser toolbar.', badgeStatusControl)}
    {@render SwitchFieldRow(ShieldCheck, 'Authenticated handoff', 'Require signed browser handoff requests.', authenticatedHandoffControl)}
    <div class="grid grid-cols-[minmax(160px,220px)_minmax(0,1fr)] items-center gap-4 border-t border-border/35 py-3">
      <div>
        <div class="flex min-w-0 items-start gap-3">
          <span class="mt-0.5 text-primary"><Ban size={18} /></span>
          <div class="min-w-0">
            <div class="text-sm font-semibold text-foreground">Excluded Sites</div>
            <div class="mt-0.5 text-xs text-muted-foreground">{formatExcludedSitesSummary(excludedHosts)}</div>
          </div>
        </div>
      </div>
      <div class="flex justify-start">
        <button type="button" onclick={() => isExcludedSitesDialogOpen = true} class="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted">
          <Ban size={16} />
          Manage
        </button>
      </div>
    </div>
  </div>
{/snippet}

{#snippet nativeHostContent()}
  {@const registration = diagnostics?.hostRegistration}
  {@const RegistrationIcon = renderRegistrationIcon(registration?.status)}
  <div class="space-y-4">
    <div class="border-b border-border/40 pb-4">
      <div class="flex items-start justify-between gap-3">
        <div class="flex min-w-0 items-start gap-3">
          <RegistrationIcon size={24} class={registration?.status === 'configured' ? 'text-success' : registration?.status === 'broken' ? 'text-warning' : 'text-muted-foreground'} />
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <div class="font-semibold text-foreground">Native messaging host</div>
              <span class={`rounded-full px-2 py-0.5 text-xs font-semibold ${registrationBadgeClass(registration?.status)}`}>{registrationStatusLabel(registration?.status)}</span>
            </div>
            <div class="mt-1 text-sm leading-6 text-muted-foreground">{renderRegistrationMessage(registration?.status)}</div>
          </div>
        </div>
        <button type="button" onclick={onRefreshDiagnostics} class="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted">
          <RotateCw size={16} />
          Refresh
        </button>
      </div>
    </div>

    <div class="grid gap-3 border-b border-border/40 pb-4 md:grid-cols-3">
      {@render StatMetric('Queue', diagnostics ? String(diagnostics.queueSummary.total) : '--')}
      {@render StatMetric('Active', diagnostics ? String(diagnostics.queueSummary.active) : '--')}
      {@render StatMetric('Recent events', diagnostics ? String(diagnostics.recentEvents.length) : '--')}
    </div>

    <div class="border-b border-border/40 pb-4">
      <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
        <div class="text-sm font-semibold text-foreground">Tools</div>
        <div class="flex flex-wrap gap-2">
          {@render UtilityButton(ExternalLink, 'Install Docs', onOpenInstallDocs)}
          {@render UtilityButton(Wrench, 'Repair Host', onRunHostRegistrationFix)}
          {@render UtilityButton(Download, 'Test Handoff', onTestExtensionHandoff)}
          {@render UtilityButton(Globe, 'Copy Report', onCopyDiagnostics)}
          {@render UtilityButton(Upload, 'Export Report', onExportDiagnostics)}
        </div>
      </div>
      {#if registration?.entries?.length}
        <div class="border-y border-border/40">
          {#each registration.entries as entry}
            <div class="border-b border-border/35 py-2 last:border-b-0">
              <div class="flex items-center justify-between gap-3">
                <div class="text-sm font-semibold text-foreground">{entry.browser}</div>
                <span class={`rounded-full px-2 py-0.5 text-xs ${entry.hostBinaryExists && entry.manifestExists ? 'bg-success/10 text-success' : 'bg-warning/10 text-warning'}`}>
                  {entry.hostBinaryExists && entry.manifestExists ? 'Ready' : 'Check'}
                </span>
              </div>
              {@render DiagnosticRow('Registry', entry.registryPath, true)}
              {@render DiagnosticRow('Manifest', entry.manifestPath ?? 'Missing', !entry.manifestExists)}
              {@render DiagnosticRow('Host', entry.hostBinaryPath ?? 'Missing', !entry.hostBinaryExists)}
            </div>
          {/each}
        </div>
      {:else}
        <div class="border-y border-border/40 py-3 text-sm text-muted-foreground">No native host registration entries are loaded.</div>
      {/if}
    </div>

    <div>
      <div class="mb-3 text-sm font-semibold text-foreground">Recent Events</div>
      {#if diagnostics?.recentEvents?.length}
        <div class="max-h-56 overflow-auto rounded-md border border-border/55 bg-zinc-950 font-mono shadow-inner">
          {#each diagnostics.recentEvents as event}
            <div class="grid grid-cols-[132px_76px_minmax(0,1fr)] gap-3 border-b border-white/10 px-3 py-2 text-[11px] leading-5 last:border-b-0">
              <span class="text-zinc-500">{formatDiagnosticEventTime(event.timestamp)}</span>
              <span class={`uppercase tracking-[0.08em] ${diagnosticLevelConsoleClass(event.level)}`}>{event.level}</span>
              <span class="min-w-0 truncate text-zinc-100" title={event.message}>{event.message}</span>
            </div>
          {/each}
        </div>
      {:else}
        <div class="rounded-md border border-border/55 bg-zinc-950 px-3 py-3 font-mono text-xs text-zinc-500">No recent diagnostics events.</div>
      {/if}
    </div>
  </div>
{/snippet}

<form onsubmit={submit} class="settings-surface mx-auto w-full max-w-6xl p-4">
  <header class="sticky top-0 z-30 flex items-center justify-between border-b border-border bg-surface/95 pb-3 pt-4 backdrop-blur">
    <div>
      <h1 class="text-xl font-semibold tracking-normal text-foreground">Settings</h1>
      <p class="mt-0.5 text-xs text-muted-foreground">Configure downloads, appearance, notifications, and native host diagnostics.</p>
    </div>
    <div class="flex items-center gap-2">
      <button type="button" onclick={onCancel} class="h-9 rounded-md px-3 text-sm font-medium text-foreground transition hover:bg-muted">Cancel</button>
      <button type="submit" disabled={isSaving} class="flex h-9 items-center gap-2 rounded-md bg-primary px-3 text-sm font-medium text-primary-foreground transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50">
        <Save size={16} />
        {isSaving ? 'Saving...' : 'Save Changes'}
      </button>
    </div>
  </header>

  <div class="mt-3 min-w-0 space-y-3">
    <section id="settings-general" class="scroll-mt-4">
      {@render CategorySettingsCard('General', Settings2, generalContent)}
    </section>

    <section id="settings-updates" class="scroll-mt-4">
      {@render CategorySettingsCard('App Updates', Download, updateContent)}
    </section>

    <section id="settings-torrenting" class="scroll-mt-4">
      {@render CategorySettingsCard('Torrenting', Gauge, torrentingContent)}
    </section>

    <section id="settings-appearance" class="scroll-mt-4">
      {@render CategorySettingsCard('Appearance', Palette, appearanceContent)}
    </section>

    <section id="settings-extension" class="scroll-mt-4">
      {@render CategorySettingsCard('Web Extension', PlugZap, extensionContent)}
    </section>

    <section id="settings-native-host" class="scroll-mt-4">
      {@render CategorySettingsCard('Native Host', Wrench, nativeHostContent)}
    </section>
  </div>
</form>

{#if isExcludedSitesDialogOpen}
  {@render ExcludedSitesDialog()}
{/if}

{#snippet directoryControl()}
  <div class="flex min-w-0 gap-2">
    <input type="text" id="downloadDirectory" name="downloadDirectory" value={formData.downloadDirectory} readonly class="min-w-0 flex-1 rounded-md border border-input bg-background px-3 py-1.5 text-sm text-muted-foreground outline-none" />
    <button type="button" onclick={() => void browseDownloadDirectory()} class="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted">
      <FolderOpen size={16} />
      Browse
    </button>
  </div>
{/snippet}

{#snippet maxConcurrentControl()}
  <input type="number" id="maxConcurrentDownloads" name="maxConcurrentDownloads" value={formData.maxConcurrentDownloads} oninput={(event) => formData.maxConcurrentDownloads = normalizeInteger(event.currentTarget.value, formData.maxConcurrentDownloads)} min="1" max="10" class="h-9 w-28 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" />
{/snippet}

{#snippet autoRetryControl()}
  <input type="number" id="autoRetryAttempts" name="autoRetryAttempts" value={formData.autoRetryAttempts} oninput={(event) => formData.autoRetryAttempts = normalizeInteger(event.currentTarget.value, formData.autoRetryAttempts)} min="0" max="10" class="h-9 w-28 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" />
{/snippet}

{#snippet speedLimitControl()}
  <div class="flex items-center gap-2">
    <input type="number" id="speedLimitKibPerSecond" name="speedLimitKibPerSecond" value={formData.speedLimitKibPerSecond} oninput={(event) => formData.speedLimitKibPerSecond = normalizeInteger(event.currentTarget.value, formData.speedLimitKibPerSecond)} min="0" max="1048576" class="h-9 w-32 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" />
    <span class="text-sm text-muted-foreground">KB/s</span>
  </div>
{/snippet}

{#snippet performanceControl()}
  <select id="downloadPerformanceMode" name="downloadPerformanceMode" bind:value={formData.downloadPerformanceMode} class="h-9 w-44 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20">
    <option value="stable">Stable</option>
    <option value="balanced">Balanced</option>
    <option value="fast">Fast</option>
  </select>
{/snippet}

{#snippet torrentEnabledControl()}
  {@render ToggleSwitch('torrentEnabled', formData.torrent.enabled, (checked) => updateTorrentSettings({ enabled: checked }))}
{/snippet}

{#snippet torrentDirectoryControl()}
  <div class="flex min-w-0 gap-2">
    <input type="text" bind:value={formData.torrent.downloadDirectory} class="min-w-0 flex-1 rounded-md border border-input bg-background px-3 py-1.5 text-sm text-muted-foreground outline-none" />
    <button type="button" onclick={() => void browseTorrentDirectory()} class="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted">
      <FolderOpen size={16} />
      Browse
    </button>
  </div>
{/snippet}

{#snippet seedModeControl()}
  <select bind:value={formData.torrent.seedMode} onchange={() => updateTorrentSettings({ seedMode: formData.torrent.seedMode })} class="h-9 w-44 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20">
    <option value="forever">Forever</option>
    <option value="ratio">Ratio</option>
    <option value="time">Time</option>
    <option value="ratio_or_time">Ratio or time</option>
  </select>
{/snippet}

{#snippet seedRatioControl()}
  <input type="number" step="0.1" min="0.1" max="100" value={formData.torrent.seedRatioLimit} oninput={(event) => updateTorrentSettings({ seedRatioLimit: normalizeNumber(event.currentTarget.value, formData.torrent.seedRatioLimit) })} class="h-9 w-28 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" />
{/snippet}

{#snippet seedTimeControl()}
  <input type="number" min="1" max="525600" value={formData.torrent.seedTimeLimitMinutes} oninput={(event) => updateTorrentSettings({ seedTimeLimitMinutes: normalizeInteger(event.currentTarget.value, formData.torrent.seedTimeLimitMinutes) })} class="h-9 w-32 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" />
{/snippet}

{#snippet uploadLimitControl()}
  <div class="flex items-center gap-2">
    <input type="number" min="0" max="1048576" value={formData.torrent.uploadLimitKibPerSecond} oninput={(event) => updateTorrentSettings({ uploadLimitKibPerSecond: normalizeInteger(event.currentTarget.value, formData.torrent.uploadLimitKibPerSecond) })} class="h-9 w-32 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" />
    <span class="text-sm text-muted-foreground">KiB/s</span>
  </div>
{/snippet}

{#snippet portForwardingControl()}
  {@render ToggleSwitch('portForwardingEnabled', formData.torrent.portForwardingEnabled, (checked) => updateTorrentSettings({ portForwardingEnabled: checked }))}
{/snippet}

{#snippet forwardingPortControl()}
  <input type="number" min="1024" max="65534" value={formData.torrent.portForwardingPort} oninput={(event) => updateTorrentSettings({ portForwardingPort: normalizeTorrentPort(event.currentTarget.value) })} class="h-9 w-32 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" />
{/snippet}

{#snippet peerWatchdogControl()}
  <select bind:value={formData.torrent.peerConnectionWatchdogMode} onchange={() => updateTorrentSettings({ peerConnectionWatchdogMode: formData.torrent.peerConnectionWatchdogMode })} class="h-9 w-44 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20">
    <option value="diagnose">Diagnose</option>
    <option value="experimental">Experimental</option>
  </select>
{/snippet}

{#snippet themeControl()}
  <select bind:value={formData.theme} class="h-9 w-44 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20">
    <option value="system">System</option>
    <option value="light">Light</option>
    <option value="dark">Dark</option>
    <option value="oled_dark">OLED dark</option>
  </select>
{/snippet}

{#snippet accentControl()}
  <div class="grid gap-3">
    <div class="space-y-2">
      <div class="text-[11px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">Gradient options</div>
      <div class="grid grid-cols-[repeat(auto-fit,minmax(118px,1fr))] gap-2">
        {#each accentGradientPresets as preset}
          <button
            type="button"
            aria-label={`Use ${preset.name} gradient accent`}
            title={`${preset.name} gradient`}
            onclick={() => accentColorInput = preset.value}
            class={`group flex h-12 items-center gap-2 rounded-md border px-2 text-left transition hover:border-primary/70 ${normalizeAccentColor(accentColorInput) === preset.value ? 'border-primary bg-primary-soft' : 'border-border/55 bg-background'}`}
          >
            <span class="h-7 w-12 rounded border border-white/15 shadow-inner" style={`background: ${preset.gradient};`}></span>
            <span class="min-w-0 truncate text-xs font-semibold text-foreground">{preset.name}</span>
          </button>
        {/each}
      </div>
    </div>

    <div class="grid gap-2 border-t border-border/35 pt-3">
      <div class="flex flex-wrap items-center gap-2">
        <span class="mr-1 text-[11px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">Custom accent</span>
        <input type="color" bind:value={accentColorInput} class="h-8 w-10 rounded-md border border-input bg-background p-1" aria-label="Custom accent color" />
        <input bind:value={accentColorInput} placeholder={DEFAULT_ACCENT_COLOR} class="h-8 w-32 rounded-md border border-input bg-background px-2 font-mono text-xs text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" aria-label="Accent color hex value" />
      </div>

      <div class="flex min-w-0 flex-wrap items-center gap-1.5">
        <span class="mr-1 text-[11px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">Solid palette</span>
      {#each accentPresets as preset}
          <button
            type="button"
            aria-label={`Use accent ${preset}`}
            title={preset}
            onclick={() => accentColorInput = preset}
            class={`h-6 w-6 rounded-md border transition hover:scale-105 ${normalizeAccentColor(accentColorInput) === preset ? 'border-primary ring-2 ring-primary/25' : 'border-border/60'}`}
            style={`background: ${preset};`}
          ></button>
      {/each}
      </div>
    </div>
  </div>
{/snippet}

{#snippet rowSizeControl()}
  <select bind:value={formData.queueRowSize} class="h-9 w-44 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20">
    {#each queueRowSizes as option}
      <option value={option.value}>{option.label}</option>
    {/each}
  </select>
{/snippet}

{#snippet notificationsControl()}
  {@render ToggleSwitch('notificationsEnabled', formData.notificationsEnabled, (checked) => formData.notificationsEnabled = checked)}
{/snippet}

{#snippet showDetailsControl()}
  {@render ToggleSwitch('showDetailsOnClick', formData.showDetailsOnClick, (checked) => formData.showDetailsOnClick = checked)}
{/snippet}

{#snippet extensionEnabledControl()}
  {@render ToggleSwitch('extensionEnabled', formData.extensionIntegration.enabled, (checked) => updateExtensionIntegration({ enabled: checked }))}
{/snippet}

{#snippet handoffModeControl()}
  <select bind:value={formData.extensionIntegration.downloadHandoffMode} class="h-9 w-44 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20">
    <option value="ask">Ask</option>
    <option value="auto">Auto</option>
    <option value="off">Off</option>
  </select>
{/snippet}

{#snippet listenPortControl()}
  <input type="number" min="1" max="65535" bind:value={formData.extensionIntegration.listenPort} class="h-9 w-32 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" />
{/snippet}

{#snippet contextMenuControl()}
  {@render ToggleSwitch('contextMenuEnabled', formData.extensionIntegration.contextMenuEnabled, (checked) => updateExtensionIntegration({ contextMenuEnabled: checked }), !formData.extensionIntegration.enabled)}
{/snippet}

{#snippet progressAfterHandoffControl()}
  {@render ToggleSwitch('showProgressAfterHandoff', formData.extensionIntegration.showProgressAfterHandoff, (checked) => updateExtensionIntegration({ showProgressAfterHandoff: checked }), !formData.extensionIntegration.enabled)}
{/snippet}

{#snippet badgeStatusControl()}
  {@render ToggleSwitch('showBadgeStatus', formData.extensionIntegration.showBadgeStatus, (checked) => updateExtensionIntegration({ showBadgeStatus: checked }), !formData.extensionIntegration.enabled)}
{/snippet}

{#snippet authenticatedHandoffControl()}
  {@render ToggleSwitch('authenticatedHandoffEnabled', formData.extensionIntegration.authenticatedHandoffEnabled, (checked) => updateExtensionIntegration({ authenticatedHandoffEnabled: checked }), !formData.extensionIntegration.enabled)}
{/snippet}

{#snippet CategorySettingsCard(title: string, icon: IconComponent, content: Snippet)}
  {@const Icon = icon}
  <div class="rounded-md border border-border/60 bg-card">
    <header class="flex min-h-11 items-center justify-between gap-3 border-b border-border/45 bg-header px-4 py-2">
      <div class="flex items-center gap-2 font-semibold text-foreground">
        <span class="text-primary"><Icon size={20} /></span>
        {title}
      </div>
    </header>
    <div class="p-4">
      {@render content()}
    </div>
  </div>
{/snippet}

{#snippet FieldRow(label: string, description: string, control: Snippet, tooltip = description, wide = false)}
  <div class={`grid ${wide ? 'grid-cols-[minmax(160px,220px)_minmax(0,1fr)] items-start' : 'grid-cols-[minmax(160px,220px)_minmax(0,1fr)] items-center'} gap-4 border-t border-border/35 py-3 first:border-t-0 first:pt-0`}>
    <div>
      <div title={description} aria-label={`${label}: ${tooltip}`} class="cursor-help truncate text-sm font-semibold text-foreground">{label}</div>
      <span class="sr-only">{description}</span>
    </div>
    <div class="min-w-0">
      {@render control()}
    </div>
  </div>
{/snippet}

{#snippet ToggleSwitch(id: string, checked: boolean, onChange: (checked: boolean) => void, disabled = false)}
  <label for={id} class={`relative inline-flex items-center ${disabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'}`}>
    <input type="checkbox" {id} {checked} {disabled} class="peer sr-only" onchange={(event) => onChange(event.currentTarget.checked)} />
    <span class="h-6 w-11 rounded-full bg-muted transition peer-checked:bg-primary"></span>
    <span class="absolute left-0.5 top-0.5 h-5 w-5 rounded-full border border-border bg-white transition peer-checked:translate-x-5"></span>
  </label>
{/snippet}

{#snippet SwitchFieldRow(icon: IconComponent, title: string, description: string, control: Snippet)}
  {@const Icon = icon}
  <div class="grid min-h-12 grid-cols-[minmax(160px,220px)_minmax(0,1fr)] items-center gap-4 border-t border-border/35 py-3 first:border-t-0 first:pt-0">
    <div class="flex min-w-0 items-start gap-3">
      <span class="mt-0.5 shrink-0 text-primary"><Icon size={18} /></span>
      <div class="min-w-0">
        <div title={description} aria-label={`${title}: ${description}`} class="cursor-help truncate text-sm font-semibold text-foreground">{title}</div>
        <span class="sr-only">{description}</span>
      </div>
    </div>
    <div class="flex min-w-0 justify-start">
      {@render control()}
    </div>
  </div>
{/snippet}

{#snippet UtilityButton(icon: IconComponent, label: string, onClick: () => void, primary = false, disabled = false)}
  {@const Icon = icon}
  <button type="button" onclick={onClick} {disabled} class={`flex h-9 items-center gap-2 rounded-md px-3 text-sm font-medium transition ${primary ? 'bg-primary text-primary-foreground hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50' : 'border border-input bg-background text-foreground hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50'}`}>
    <Icon size={16} />
    {label}
  </button>
{/snippet}

{#snippet VersionIndicator(label: string, value: string, tone: AppUpdateVersionTone = 'current')}
  <div class="min-w-0 border-l border-border pl-3">
    <div class="text-[11px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">{label}</div>
    <div class={`mt-1 truncate text-sm font-semibold tabular-nums ${versionIndicatorToneClass(tone)}`} title={value}>{value}</div>
  </div>
{/snippet}

{#snippet StatMetric(label: string, value: string)}
  <div class="min-w-0 border-l border-border pl-3">
    <div class="text-xs text-muted-foreground">{label}</div>
    <div class="mt-1 text-lg font-semibold tabular-nums text-foreground">{value}</div>
  </div>
{/snippet}

{#snippet DiagnosticRow(label: string, value: string, muted = false)}
  <div class="grid grid-cols-[110px_minmax(0,1fr)] gap-3 py-1 text-sm">
    <span class="text-muted-foreground">{label}</span>
    <span class={`break-all font-mono text-xs ${muted ? 'text-muted-foreground' : 'text-foreground'}`}>{value}</span>
  </div>
{/snippet}

{#snippet ExcludedSitesDialog()}
  <div class="fixed inset-0 z-[90] flex items-center justify-center bg-black/60 px-4">
    <div role="dialog" aria-modal="true" aria-labelledby="excluded-sites-title" class="w-full max-w-2xl rounded-md border border-border bg-card shadow-2xl">
      <header class="flex items-center justify-between border-b border-border bg-header px-5 py-3">
        <div>
          <h2 id="excluded-sites-title" class="text-base font-semibold text-foreground">Excluded Sites</h2>
          <p class="mt-0.5 text-xs text-muted-foreground">Prevent matching hosts from being handed off by the browser extension.</p>
        </div>
        <button type="button" onclick={() => isExcludedSitesDialogOpen = false} aria-label="Close excluded sites" class="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition hover:bg-muted hover:text-foreground">
          <X size={18} />
        </button>
      </header>

      <div class="space-y-4 px-5 py-4">
        <div class="grid gap-3 md:grid-cols-[1fr_auto]">
          <input bind:value={excludedHostInput} placeholder="example.com or *.example.com" class="h-9 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" />
          <button type="button" onclick={addExcludedHost} disabled={!normalizeHostInput(excludedHostInput)} class="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50">
            <Plus size={14} />
            Add
          </button>
        </div>

        <div class="grid gap-3 md:grid-cols-[1fr_auto]">
          <textarea bind:value={excludedBulkInput} rows="3" placeholder="Paste hosts, URLs, or wildcard hosts separated by commas or new lines." class="resize-none rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"></textarea>
          <button type="button" onclick={addExcludedBulkHosts} disabled={parseExcludedHostInput(excludedBulkInput).length === 0} class="flex h-9 items-center gap-2 self-end rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50">
            <Plus size={14} />
            Add Bulk
          </button>
        </div>

        <div class="space-y-2">
          <label class="text-sm font-semibold text-foreground" for="excludedSitesSearch">Current Sites</label>
          <div class="relative">
            <Search size={15} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground" />
            <input id="excludedSitesSearch" type="text" bind:value={excludedSearchQuery} placeholder="Search hosts" class="h-9 w-full rounded-md border border-input bg-background pl-9 pr-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20" />
          </div>
          <div class="max-h-56 overflow-auto rounded-md border border-border bg-surface">
            {#if filteredExcludedHosts.length > 0}
              {#each filteredExcludedHosts as host}
                <div class="flex h-10 items-center justify-between gap-3 border-b border-border/35 px-3 last:border-b-0">
                  <div class="flex min-w-0 items-center gap-2">
                    <Ban size={14} class="shrink-0 text-muted-foreground" />
                    <span class="truncate text-sm font-medium text-foreground">{host}</span>
                  </div>
                  <button type="button" onclick={() => removeExcludedSite(host)} disabled={!formData.extensionIntegration.enabled} class="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition hover:bg-muted hover:text-destructive disabled:cursor-not-allowed disabled:opacity-50" title={`Remove ${host}`} aria-label={`Remove ${host}`}>
                    <Trash2 size={14} />
                  </button>
                </div>
              {/each}
            {:else}
              <div class="px-3 py-6 text-center text-sm text-muted-foreground">No excluded sites match.</div>
            {/if}
          </div>
        </div>
      </div>

      <footer class="flex justify-end border-t border-border px-5 py-3">
        <button type="button" onclick={() => isExcludedSitesDialogOpen = false} class="h-9 rounded-md bg-primary px-4 text-sm font-semibold text-primary-foreground transition hover:bg-primary/90">Done</button>
      </footer>
    </div>
  </div>
{/snippet}
