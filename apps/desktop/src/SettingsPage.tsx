import React, { useEffect, useMemo, useRef, useState } from 'react';
import type { DiagnosticsSnapshot, Settings } from './types';
import type { AppUpdateState, AppUpdateVersionTone } from './appUpdates';
import { updateVersionIndicator } from './appUpdates';
import desktopPackage from '../package.json';
import { normalizeAccentColor } from './appearance';
import { shouldAdoptIncomingSettingsDraft } from './settingsDraftSync';
import {
  addExcludedHosts,
  filterExcludedHosts,
  formatExcludedSitesSummary,
  normalizeHostInput,
  parseExcludedHostInput,
  removeExcludedHost,
} from './settingsExcludedSites';
import {
  Activity,
  Ban,
  Check,
  Copy,
  Download,
  ExternalLink,
  FolderOpen,
  Gauge,
  Globe,
  MousePointerClick,
  Palette,
  PlugZap,
  Plus,
  RefreshCw,
  Save,
  Search,
  Settings2,
  ShieldAlert,
  ShieldCheck,
  ShieldX,
  TestTube2,
  Trash2,
  Wrench,
  X,
} from 'lucide-react';
import { defaultTorrentDownloadDirectory, normalizeTorrentSettings } from './torrentSettings';

const ACCENT_COLOR_PRESETS = [
  { name: 'Blue', value: '#3b82f6' },
  { name: 'Cyan', value: '#06b6d4' },
  { name: 'Green', value: '#22c55e' },
  { name: 'Amber', value: '#f59e0b' },
  { name: 'Rose', value: '#f43f5e' },
  { name: 'Violet', value: '#8b5cf6' },
];

const DESKTOP_APP_VERSION = desktopPackage.version;

interface SettingsPageProps {
  settings: Settings;
  diagnostics: DiagnosticsSnapshot | null;
  onSave: (settings: Settings) => void | Promise<void | boolean>;
  onBrowseDirectory: () => Promise<string | null | void>;
  hasActiveTorrentJobs: boolean;
  onClearTorrentSessionCache: () => void | Promise<void>;
  onCancel: () => void;
  onDirtyChange?: (isDirty: boolean) => void;
  onDraftChange?: (settings: Settings) => void;
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

export function SettingsPage({
  settings,
  diagnostics,
  onSave,
  onBrowseDirectory,
  hasActiveTorrentJobs,
  onClearTorrentSessionCache,
  onCancel,
  onDirtyChange,
  onDraftChange,
  onRefreshDiagnostics,
  onOpenInstallDocs,
  onRunHostRegistrationFix,
  onTestExtensionHandoff,
  onCopyDiagnostics,
  onExportDiagnostics,
  updateState,
  onCheckForUpdates,
  onInstallUpdate,
}: SettingsPageProps) {
  const [formData, setFormData] = useState<Settings>(settings);
  const formDataRef = useRef(formData);
  const previousSettingsRef = useRef(settings);
  const [excludedHostInput, setExcludedHostInput] = useState('');
  const [excludedBulkInput, setExcludedBulkInput] = useState('');
  const [excludedSearchQuery, setExcludedSearchQuery] = useState('');
  const [isExcludedSitesDialogOpen, setIsExcludedSitesDialogOpen] = useState(false);
  const [accentColorInput, setAccentColorInput] = useState(normalizeAccentColor(settings.accentColor));
  const [isClearingTorrentSessionCache, setIsClearingTorrentSessionCache] = useState(false);

  formDataRef.current = formData;

  useEffect(() => {
    const previousSettings = previousSettingsRef.current;
    previousSettingsRef.current = settings;

    if (!shouldAdoptIncomingSettingsDraft(formDataRef.current, previousSettings, settings)) {
      return;
    }

    formDataRef.current = settings;
    setFormData(settings);
    setExcludedHostInput('');
    setExcludedBulkInput('');
    setExcludedSearchQuery('');
    setIsExcludedSitesDialogOpen(false);
    setAccentColorInput(normalizeAccentColor(settings.accentColor));
  }, [settings]);

  const isDirty = useMemo(() => {
    return JSON.stringify(formData) !== JSON.stringify(settings);
  }, [formData, settings]);

  useEffect(() => {
    onDirtyChange?.(isDirty);
  }, [isDirty, onDirtyChange]);

  useEffect(() => {
    onDraftChange?.(formData);
  }, [formData, onDraftChange]);

  const handleChange = (event: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) => {
    const { name, value, type } = event.target;
    const nextValue = type === 'checkbox' ? (event.target as HTMLInputElement).checked : value;
    const numberValue = Number.parseInt(value, 10);

    setFormData((prev) => ({
      ...prev,
      [name]: type === 'number' ? (Number.isNaN(numberValue) ? 0 : numberValue) : nextValue,
    }));
  };

  const handleSubmit = (event: React.FormEvent) => {
    event.preventDefault();
    void onSave(formData);
  };

  const handleBrowseDirectory = async () => {
    const selectedDirectory = await onBrowseDirectory();
    if (!selectedDirectory) return;
    setFormData((prev) => {
      const previousDefaultTorrentDirectory = defaultTorrentDownloadDirectory(prev.downloadDirectory);
      const shouldUpdateTorrentDirectory = !prev.torrent.downloadDirectory
        || prev.torrent.downloadDirectory === previousDefaultTorrentDirectory;
      return {
        ...prev,
        downloadDirectory: selectedDirectory,
        torrent: shouldUpdateTorrentDirectory
          ? normalizeTorrentSettings(
            { ...prev.torrent, downloadDirectory: defaultTorrentDownloadDirectory(selectedDirectory) },
            selectedDirectory,
          )
          : prev.torrent,
      };
    });
  };

  const handleBrowseTorrentDirectory = async () => {
    const selectedDirectory = await onBrowseDirectory();
    if (!selectedDirectory) return;
    updateTorrentSettings({ downloadDirectory: selectedDirectory });
  };

  const handleClearTorrentSessionCache = async () => {
    setIsClearingTorrentSessionCache(true);
    try {
      await onClearTorrentSessionCache();
    } finally {
      setIsClearingTorrentSessionCache(false);
    }
  };

  const updateExtensionIntegration = (update: Partial<Settings['extensionIntegration']>) => {
    setFormData((prev) => ({
      ...prev,
      extensionIntegration: {
        ...prev.extensionIntegration,
        ...update,
      },
    }));
  };

  const updateTorrentSettings = (update: Partial<Settings['torrent']>) => {
    setFormData((prev) => ({
      ...prev,
      torrent: normalizeTorrentSettings({
        ...prev.torrent,
        ...update,
      }, prev.downloadDirectory),
    }));
  };

  const updateAccentColor = (accentColor: string) => {
    const normalizedAccentColor = normalizeAccentColor(accentColor);
    setAccentColorInput(normalizedAccentColor);
    setFormData((prev) => ({
      ...prev,
      accentColor: normalizedAccentColor,
    }));
  };

  const handleAccentTextChange = (value: string) => {
    setAccentColorInput(value);
    if (/^#[0-9a-f]{6}$/i.test(value.trim())) {
      updateAccentColor(value);
    }
  };

  const handleAddExcludedHost = () => {
    const nextHosts = addExcludedHosts(formData.extensionIntegration.excludedHosts, [excludedHostInput]).hosts;
    updateExtensionIntegration({ excludedHosts: nextHosts });
    setExcludedHostInput('');
  };

  const handleAddBulkExcludedHosts = () => {
    const candidates = parseExcludedHostInput(excludedBulkInput);
    if (candidates.length === 0) return;

    const nextHosts = addExcludedHosts(formData.extensionIntegration.excludedHosts, candidates).hosts;
    updateExtensionIntegration({ excludedHosts: nextHosts });
    setExcludedBulkInput('');
  };

  const handleRemoveExcludedHost = (host: string) => {
    updateExtensionIntegration({
      excludedHosts: removeExcludedHost(formData.extensionIntegration.excludedHosts, host),
    });
  };

  const updateVersion = updateVersionIndicator(updateState, DESKTOP_APP_VERSION);

  return (
    <>
    <form onSubmit={handleSubmit} className="settings-surface mx-auto grid w-full max-w-6xl grid-cols-[160px_minmax(0,1fr)] gap-3 p-4">
      <header className="col-span-2 sticky top-0 z-30 flex items-center justify-between border-b border-border bg-surface/95 pb-3 pt-4 backdrop-blur">
        <div>
          <h1 className="text-xl font-semibold tracking-normal text-foreground">Settings</h1>
          <p className="mt-0.5 text-xs text-muted-foreground">Configure downloads, appearance, notifications, and native host diagnostics.</p>
        </div>
        <div className="flex items-center gap-2">
          <button type="button" onClick={onCancel} className="h-9 rounded-md px-3 text-sm font-medium text-foreground transition hover:bg-muted">
            Cancel
          </button>
          <button type="submit" className="flex h-9 items-center gap-2 rounded-md bg-primary px-3 text-sm font-medium text-primary-foreground transition hover:bg-primary/90">
            <Save size={16} />
            Save Changes
          </button>
        </div>
      </header>

      <nav className="settings-nav sticky top-24 h-fit rounded-md border border-border bg-card p-1.5" aria-label="Settings sections">
        <SettingsNavLink href="#settings-general" label="General" />
        <SettingsNavLink href="#settings-updates" label="App Updates" />
        <SettingsNavLink href="#settings-torrenting" label="Torrenting" />
        <SettingsNavLink href="#settings-appearance" label="Appearance" />
        <SettingsNavLink href="#settings-extension" label="Web Extension" />
        <SettingsNavLink href="#settings-native-host" label="Native Host" />
      </nav>

      <div className="min-w-0 space-y-3">
        <section id="settings-general" className="scroll-mt-4">
        <SettingsPanel icon={<Settings2 size={20} />} title="General">
          <FieldRow label="Download Directory" description="Default save path." tooltip="Files are saved here by default.">
            <div className="flex min-w-0 gap-2">
              <input
                type="text"
                id="downloadDirectory"
                name="downloadDirectory"
                value={formData.downloadDirectory}
                readOnly
                className="min-w-0 flex-1 rounded-md border border-input bg-background px-3 py-1.5 text-sm text-muted-foreground outline-none"
              />
              <button
                type="button"
                onClick={() => void handleBrowseDirectory()}
                className="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted"
              >
                <FolderOpen size={16} />
                Browse
              </button>
            </div>
          </FieldRow>

          <FieldRow label="Max Concurrent Downloads" description="Active job limit." tooltip="Limits how many jobs may run at once.">
            <input
              type="number"
              id="maxConcurrentDownloads"
              name="maxConcurrentDownloads"
              value={formData.maxConcurrentDownloads}
              onChange={handleChange}
              min="1"
              max="10"
              className="h-9 w-28 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
            />
          </FieldRow>

          <FieldRow label="Auto Retry Attempts" description="Failure retries." tooltip="Retries transient network or server failures before marking a job failed.">
            <input
              type="number"
              id="autoRetryAttempts"
              name="autoRetryAttempts"
              value={formData.autoRetryAttempts}
              onChange={handleChange}
              min="0"
              max="10"
              className="h-9 w-28 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
            />
          </FieldRow>

          <FieldRow label="Per-Download Speed Limit" description="Transfer cap." tooltip="Caps each active transfer. Set 0 to keep downloads unlimited.">
            <div className="flex items-center gap-2">
              <input
                type="number"
                id="speedLimitKibPerSecond"
                name="speedLimitKibPerSecond"
                value={formData.speedLimitKibPerSecond}
                onChange={handleChange}
                min="0"
                max="1048576"
                className="h-9 w-32 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
              />
              <span className="text-sm text-muted-foreground">KB/s</span>
            </div>
          </FieldRow>

          <FieldRow label="Download Performance" description="Connection strategy." tooltip="Balanced uses safe segmented transfers for large range-capable files. Stable keeps one stream. Fast uses more segments when supported.">
            <select
              id="downloadPerformanceMode"
              name="downloadPerformanceMode"
              value={formData.downloadPerformanceMode}
              onChange={handleChange}
              className="h-9 w-44 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
            >
              <option value="stable">Stable</option>
              <option value="balanced">Balanced</option>
              <option value="fast">Fast</option>
            </select>
          </FieldRow>
        </SettingsPanel>
        </section>

        <section id="settings-updates" className="scroll-mt-4">
        <SettingsPanel icon={<Download size={20} />} title="App Updates">
          <div className="rounded-md border border-border bg-surface p-4">
            <div className="mb-4 flex items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="font-semibold text-foreground">Alpha channel updates</div>
                <div className="mt-1 text-sm leading-6 text-muted-foreground">
                  {renderUpdateStatus(updateState)}
                </div>
              </div>
              {updateState.availableUpdate ? (
                <span className="rounded-full bg-primary/10 px-2.5 py-1 text-xs font-semibold text-primary">
                  {updateState.availableUpdate.version}
                </span>
              ) : null}
            </div>

            <div className="mb-4 grid gap-2 sm:grid-cols-2">
              <VersionIndicator label="Current" value={updateVersion.currentVersion} />
              <VersionIndicator label="New" value={updateVersion.newVersion} tone={updateVersion.newVersionTone} />
            </div>

            {updateState.availableUpdate?.body ? (
              <div className="mb-4 rounded border border-border bg-background px-3 py-2 text-sm leading-6 text-muted-foreground">
                {updateState.availableUpdate.body}
              </div>
            ) : null}

            {updateState.status === 'downloading' ? (
              <div className="mb-4">
                <div className="mb-1 flex justify-between text-xs tabular-nums text-muted-foreground">
                  <span>Downloading update</span>
                  <span>{formatUpdateProgress(updateState)}</span>
                </div>
                <div className="h-1.5 overflow-hidden rounded-full bg-progress-track">
                  <div className="h-1.5 rounded-full bg-primary transition-all duration-300" style={{ width: `${updateProgressPercent(updateState)}%` }} />
                </div>
              </div>
            ) : null}

            {updateState.errorMessage ? (
              <div className="mb-4 rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                {updateState.errorMessage}
              </div>
            ) : null}

            <div className="flex flex-wrap gap-2">
              <UtilityButton
                icon={<RefreshCw size={16} />}
                label={updateState.status === 'checking' ? 'Checking...' : 'Check for Updates'}
                onClick={onCheckForUpdates}
                disabled={updateState.status === 'checking' || updateState.status === 'downloading' || updateState.status === 'installing'}
              />
              <UtilityButton
                icon={<Download size={16} />}
                label={updateState.status === 'installing' ? 'Installing...' : 'Install Update'}
                onClick={onInstallUpdate}
                disabled={!updateState.availableUpdate || updateState.status === 'checking' || updateState.status === 'downloading' || updateState.status === 'installing'}
                primary={Boolean(updateState.availableUpdate)}
              />
            </div>
          </div>
        </SettingsPanel>
        </section>

        <section id="settings-torrenting" className="scroll-mt-4">
        <SettingsPanel icon={<Gauge size={20} />} title="Torrenting">
          <FieldRow label="Torrent Downloads" description="Manual magnet and .torrent jobs." tooltip="Allows torrent jobs added from the desktop app or browser extension.">
            <ToggleSwitch
              id="torrentEnabled"
              checked={formData.torrent.enabled}
              onChange={(checked) => updateTorrentSettings({ enabled: checked })}
            />
          </FieldRow>

          <FieldRow label="Torrent Download Directory" description="Default torrent save path." tooltip="New torrent jobs use this folder instead of file-type category folders.">
            <div className="flex min-w-0 gap-2">
              <input
                type="text"
                id="torrentDownloadDirectory"
                value={formData.torrent.downloadDirectory}
                readOnly
                className="min-w-0 flex-1 rounded-md border border-input bg-background px-3 py-1.5 text-sm text-muted-foreground outline-none"
              />
              <button
                type="button"
                onClick={() => void handleBrowseTorrentDirectory()}
                className="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted"
              >
                <FolderOpen size={16} />
                Browse
              </button>
            </div>
          </FieldRow>

          <FieldRow
            label="Clear Cache Session"
            description={hasActiveTorrentJobs ? 'Pause active torrents first.' : 'Remove stale torrent engine resume cache.'}
            tooltip="Clears the torrent session cache without deleting downloaded payload files or persisted counters."
          >
            <button
              type="button"
              onClick={() => void handleClearTorrentSessionCache()}
              disabled={hasActiveTorrentJobs || isClearingTorrentSessionCache}
              className="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Trash2 size={16} />
              {isClearingTorrentSessionCache ? 'Clearing...' : 'Clear Cache Session'}
            </button>
          </FieldRow>

          <FieldRow label="Seeding Policy" description="Stop condition." tooltip="Controls when completed torrent downloads stop seeding.">
            <select
              id="torrentSeedMode"
              value={formData.torrent.seedMode}
              onChange={(event) => updateTorrentSettings({ seedMode: event.target.value as Settings['torrent']['seedMode'] })}
              disabled={!formData.torrent.enabled}
              className="h-9 w-44 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <option value="forever">Seed forever</option>
              <option value="ratio">Stop at ratio</option>
              <option value="time">Stop after time</option>
              <option value="ratio_or_time">Ratio or time</option>
            </select>
          </FieldRow>

          <FieldRow label="Ratio Limit" description="Upload/download target." tooltip="Used by ratio-based seeding policies.">
            <input
              type="number"
              id="torrentSeedRatioLimit"
              value={formData.torrent.seedRatioLimit}
              onChange={(event) => updateTorrentSettings({ seedRatioLimit: normalizeNumber(event.target.value, 0.1) })}
              min="0.1"
              max="100"
              step="0.1"
              disabled={!formData.torrent.enabled || !usesTorrentRatioLimit(formData.torrent.seedMode)}
              className="h-9 w-28 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
            />
          </FieldRow>

          <FieldRow label="Time Limit" description="Minutes after completion." tooltip="Used by time-based seeding policies.">
            <div className="flex items-center gap-2">
              <input
                type="number"
                id="torrentSeedTimeLimitMinutes"
                value={formData.torrent.seedTimeLimitMinutes}
                onChange={(event) => updateTorrentSettings({ seedTimeLimitMinutes: normalizeInteger(event.target.value, 1) })}
                min="1"
                max="525600"
                disabled={!formData.torrent.enabled || !usesTorrentTimeLimit(formData.torrent.seedMode)}
                className="h-9 w-28 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
              />
              <span className="text-sm text-muted-foreground">min</span>
            </div>
          </FieldRow>

          <FieldRow label="Upload Limit" description="0 keeps upload uncapped." tooltip="Applies to new torrent sessions and jobs.">
            <div className="flex items-center gap-2">
              <input
                type="number"
                id="torrentUploadLimitKibPerSecond"
                value={formData.torrent.uploadLimitKibPerSecond}
                onChange={(event) => updateTorrentSettings({ uploadLimitKibPerSecond: normalizeInteger(event.target.value, 0) })}
                min="0"
                max="1048576"
                disabled={!formData.torrent.enabled}
                className="h-9 w-32 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
              />
              <span className="text-sm text-muted-foreground">KB/s</span>
            </div>
          </FieldRow>

          <FieldRow label="Port Forwarding" description="UPnP inbound peer access." tooltip="Allows the torrent engine to ask your router to forward the selected listen port.">
            <ToggleSwitch
              id="torrentPortForwardingEnabled"
              checked={formData.torrent.portForwardingEnabled}
              onChange={(checked) => updateTorrentSettings({ portForwardingEnabled: checked })}
              disabled={!formData.torrent.enabled}
            />
          </FieldRow>

          <FieldRow label="Listen Port" description="Forwarded torrent port." tooltip="Used when torrent port forwarding is enabled.">
            <div className="flex items-center gap-2">
              <input
                type="number"
                id="torrentPortForwardingPort"
                value={formData.torrent.portForwardingPort}
                onChange={(event) => updateTorrentSettings({ portForwardingPort: normalizeTorrentPort(event.target.value) })}
                min="1024"
                max="65534"
                step="1"
                inputMode="numeric"
                disabled={!formData.torrent.enabled || !formData.torrent.portForwardingEnabled}
                className="h-9 w-32 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
              />
              <span className="text-sm text-muted-foreground">port</span>
            </div>
          </FieldRow>
        </SettingsPanel>
        </section>

        <section id="settings-appearance" className="scroll-mt-4">
        <SettingsPanel icon={<Palette size={20} />} title="Appearance & Behavior">
          <FieldRow label="Desktop Notifications" description="Native alerts." tooltip="Show a native notification when downloads finish or fail.">
            <label className="relative inline-flex cursor-pointer items-center">
              <input
                type="checkbox"
                id="notificationsEnabled"
                name="notificationsEnabled"
                checked={formData.notificationsEnabled}
                onChange={handleChange}
                className="peer sr-only"
              />
              <span className="h-6 w-11 rounded-full bg-muted transition peer-checked:bg-primary" />
              <span className="absolute left-0.5 top-0.5 h-5 w-5 rounded-full border border-border bg-white transition peer-checked:translate-x-5" />
            </label>
          </FieldRow>

          <FieldRow label="Click Opens Details" description="Show selected-download details on row click." tooltip="When enabled, clicking a download opens the bottom details pane. Turn it off to keep row clicks selection-only.">
            <ToggleSwitch
              id="showDetailsOnClick"
              checked={formData.showDetailsOnClick}
              onChange={(checked) => setFormData((prev) => ({ ...prev, showDetailsOnClick: checked }))}
            />
          </FieldRow>

          <FieldRow label="Start with Windows" description="Auto-launch app." tooltip="Register this application to launch when you sign in to Windows.">
            <ToggleSwitch
              id="startOnStartup"
              checked={formData.startOnStartup}
              onChange={(checked) => setFormData((prev) => ({ ...prev, startOnStartup: checked }))}
            />
          </FieldRow>

          <FieldRow label="Startup Launch" description="Window behavior." tooltip="Choose whether Windows startup opens the main window or keeps the app in the tray.">
            <select
              id="startupLaunchMode"
              name="startupLaunchMode"
              value={formData.startupLaunchMode}
              onChange={handleChange}
              disabled={!formData.startOnStartup}
              className="h-9 w-56 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <option value="open">Open window</option>
              <option value="tray">Tray only</option>
            </select>
          </FieldRow>

          <FieldRow label="App Theme" description="Shell theme." tooltip="Theme switching stays in Settings and applies to the entire shell.">
            <select
              id="theme"
              name="theme"
              value={formData.theme}
              onChange={handleChange}
              className="h-9 w-56 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
            >
              <option value="light">Light Mode</option>
              <option value="dark">Dark Mode</option>
              <option value="oled_dark">OLED Dark</option>
              <option value="system">System Default</option>
            </select>
          </FieldRow>

          <FieldRow label="Color Accent" description="Accent color." tooltip="Choose a preset or use the color picker for a custom accent.">
            <div className="flex flex-wrap items-center gap-3">
              <div className="flex flex-wrap gap-2" role="radiogroup" aria-label="Accent color presets">
                {ACCENT_COLOR_PRESETS.map((preset) => (
                  <button
                    key={preset.value}
                    type="button"
                    onClick={() => updateAccentColor(preset.value)}
                    className={`relative flex h-9 w-9 items-center justify-center rounded-md border transition ${
                      normalizeAccentColor(formData.accentColor) === preset.value
                        ? 'border-primary ring-2 ring-primary/30'
                        : 'border-border hover:border-input'
                    }`}
                    style={{ backgroundColor: preset.value }}
                    title={preset.name}
                    aria-label={`${preset.name} accent`}
                    aria-checked={normalizeAccentColor(formData.accentColor) === preset.value}
                    role="radio"
                  >
                    {normalizeAccentColor(formData.accentColor) === preset.value ? (
                      <Check size={16} className="text-white drop-shadow" />
                    ) : null}
                  </button>
                ))}
              </div>

              <label className="flex h-9 items-center gap-3 rounded-md border border-input bg-background px-3 text-sm text-foreground">
                <span className="text-muted-foreground">Custom</span>
                <input
                  type="color"
                  value={normalizeAccentColor(formData.accentColor)}
                  onChange={(event) => updateAccentColor(event.target.value)}
                  onInput={(event) => updateAccentColor(event.currentTarget.value)}
                  className="h-6 w-9 cursor-pointer rounded border-0 bg-transparent p-0"
                  aria-label="Custom accent color"
                />
                <input
                  type="text"
                  value={accentColorInput}
                  onChange={(event) => handleAccentTextChange(event.target.value)}
                  onBlur={() => setAccentColorInput(normalizeAccentColor(formData.accentColor))}
                  aria-label="Accent color hex"
                  spellCheck={false}
                  className="h-7 w-24 rounded border border-transparent bg-transparent px-1 font-mono text-xs text-muted-foreground outline-none transition focus:border-primary focus:text-foreground"
                />
              </label>
            </div>
          </FieldRow>
        </SettingsPanel>
        </section>

        <section id="settings-extension" className="scroll-mt-4">
        <SettingsPanel icon={<PlugZap size={20} />} title="Web Extension">
          <FieldRow label="Extension Integration" description="Browser handoff." tooltip="Allow browsers to hand downloads to this app.">
            <ToggleSwitch
              id="extensionIntegrationEnabled"
              checked={formData.extensionIntegration.enabled}
              onChange={(checked) => updateExtensionIntegration({ enabled: checked })}
            />
          </FieldRow>

          <FieldRow label="Browser Downloads" description="Capture behavior." tooltip="Controls what happens when a website starts a download.">
            <select
              id="downloadHandoffMode"
              value={formData.extensionIntegration.downloadHandoffMode}
              onChange={(event) => updateExtensionIntegration({ downloadHandoffMode: event.target.value as Settings['extensionIntegration']['downloadHandoffMode'] })}
              disabled={!formData.extensionIntegration.enabled}
              className="h-9 w-60 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <option value="off">Off</option>
              <option value="ask">Ask before sending</option>
              <option value="auto">Send automatically</option>
            </select>
          </FieldRow>

          <FieldRow label="Listen Port" description="Extension port." tooltip="Local port used by web-extension listener settings.">
            <div className="flex items-center gap-2">
              <input
                type="number"
                id="extensionListenPort"
                value={formData.extensionIntegration.listenPort}
                onChange={(event) => updateExtensionIntegration({ listenPort: normalizeListenPort(event.target.value) })}
                min="1"
                max="65535"
                step="1"
                inputMode="numeric"
                aria-label="Extension listen port"
                disabled={!formData.extensionIntegration.enabled}
                className="h-9 w-32 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
              />
              <span className="text-sm text-muted-foreground">port</span>
            </div>
          </FieldRow>

          <div className="flex items-start justify-between gap-4 rounded-md border border-border bg-surface px-4 py-3">
            <div className="flex min-w-0 items-start gap-3">
              {renderRegistrationIcon(diagnostics?.hostRegistration.status)}
              <div className="min-w-0">
                <div className="text-sm font-semibold text-foreground">Native host status</div>
                <div className="mt-1 text-sm leading-5 text-muted-foreground">
                  {renderRegistrationMessage(diagnostics?.hostRegistration.status)}
                </div>
              </div>
            </div>
            <span className={`shrink-0 rounded-full px-2.5 py-1 text-xs font-semibold ${registrationBadgeClass(diagnostics?.hostRegistration.status)}`}>
              {registrationStatusLabel(diagnostics?.hostRegistration.status)}
            </span>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <CompactSetting
              icon={<MousePointerClick size={17} />}
              title="Right-click handoff"
              description="Context command."
              tooltip="Show the browser context menu command."
              control={
                <ToggleSwitch
                  id="contextMenuEnabled"
                  checked={formData.extensionIntegration.contextMenuEnabled}
                  onChange={(checked) => updateExtensionIntegration({ contextMenuEnabled: checked })}
                  disabled={!formData.extensionIntegration.enabled}
                />
              }
            />
            <CompactSetting
              icon={<Download size={17} />}
              title="Progress window"
              description="Handoff progress."
              tooltip="Open progress after a browser handoff."
              control={
                <ToggleSwitch
                  id="showProgressAfterHandoff"
                  checked={formData.extensionIntegration.showProgressAfterHandoff}
                  onChange={(checked) => updateExtensionIntegration({ showProgressAfterHandoff: checked })}
                  disabled={!formData.extensionIntegration.enabled}
                />
              }
            />
            <CompactSetting
              icon={<Globe size={17} />}
              title="Badge status"
              description="Popup status."
              tooltip="Show connection and queue status in the popup."
              control={
                <ToggleSwitch
                  id="showBadgeStatus"
                  checked={formData.extensionIntegration.showBadgeStatus}
                  onChange={(checked) => updateExtensionIntegration({ showBadgeStatus: checked })}
                  disabled={!formData.extensionIntegration.enabled}
                />
              }
            />
            <CompactSetting
              icon={<TestTube2 size={17} />}
              title="Test handoff"
              description="Prompt test."
              tooltip="Open a browser-style confirmation prompt."
              control={
                <button
                  type="button"
                  onClick={onTestExtensionHandoff}
                  disabled={!formData.extensionIntegration.enabled}
                  className="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <TestTube2 size={15} />
                  Test
                </button>
              }
            />
          </div>

          <FieldRow label="Excluded Sites" description="Browser-only hosts and wildcard host patterns." tooltip="Downloads from these hostnames stay in the browser. Wildcards match host labels, such as *.example.com.">
            <button
              type="button"
              onClick={() => setIsExcludedSitesDialogOpen(true)}
              disabled={!formData.extensionIntegration.enabled}
              className="flex h-9 w-fit items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Ban size={15} />
              Configure Sites
            </button>
          </FieldRow>
          <FieldRow label="Protected Downloads" description="Use the browser session for exact protected download handoffs." tooltip="For downloads already intercepted by the extension, forward bounded memory-only browser auth headers to the desktop app. Header values are never persisted or included in diagnostics.">
            <ToggleSwitch
              id="authenticatedHandoffEnabled"
              checked={formData.extensionIntegration.authenticatedHandoffEnabled}
              onChange={(checked) => updateExtensionIntegration({ authenticatedHandoffEnabled: checked })}
              disabled={!formData.extensionIntegration.enabled}
            />
          </FieldRow>
        </SettingsPanel>
        </section>

        <section id="settings-native-host" className="scroll-mt-4">
        <SettingsPanel
          icon={<Activity size={20} />}
          title="Native Host Registration"
          action={
            <button
              type="button"
              onClick={onRefreshDiagnostics}
              className="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted"
            >
              <RefreshCw size={15} />
              Refresh
            </button>
          }
        >
          <div className="rounded-md border border-border bg-surface p-4">
            <div className="mb-4 flex items-start gap-3">
              {renderRegistrationIcon(diagnostics?.hostRegistration.status)}
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 font-semibold text-foreground">
                  <span>Browser native messaging host</span>
                  <button
                    type="button"
                    onClick={onOpenInstallDocs}
                    title="Open Docs"
                    aria-label="Open Docs"
                    className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition hover:bg-muted hover:text-primary"
                  >
                    <ExternalLink size={15} />
                  </button>
                </div>
                <div className="mt-1 text-sm leading-6 text-muted-foreground">
                  {renderRegistrationMessage(diagnostics?.hostRegistration.status)}
                </div>
              </div>
            </div>

            <div className="flex flex-wrap gap-2">
              <UtilityButton icon={<Copy size={16} />} label="Copy Diagnostics" onClick={onCopyDiagnostics} />
              <UtilityButton icon={<Download size={16} />} label="Export Report" onClick={onExportDiagnostics} />
              <UtilityButton icon={<Wrench size={16} />} label="Repair Host Registration" onClick={onRunHostRegistrationFix} primary />
            </div>
          </div>

          <div className="space-y-2">
            {diagnostics?.hostRegistration.entries.map((entry) => (
              <div key={entry.browser} className="rounded-md border border-border bg-background p-4">
                <div className="mb-3 flex items-center justify-between gap-3">
                  <div className="font-semibold text-foreground">{entry.browser}</div>
                  <span className={`rounded-full px-2.5 py-1 text-xs font-semibold ${entry.hostBinaryExists ? 'bg-success/10 text-success' : entry.manifestPath ? 'bg-warning/10 text-warning' : 'bg-muted text-muted-foreground'}`}>
                    {entry.hostBinaryExists ? 'Ready' : entry.manifestPath ? 'Broken' : 'Missing'}
                  </span>
                </div>
                <DiagnosticRow label="Registry" value={entry.registryPath} />
                <DiagnosticRow label="Manifest" value={entry.manifestPath ?? 'Not registered'} muted={!entry.manifestExists} />
                <DiagnosticRow label="Host Binary" value={entry.hostBinaryPath ?? 'Missing from manifest'} muted={!entry.hostBinaryExists} />
              </div>
            ))}
          </div>

          <div className="rounded-md border border-border bg-surface p-4">
            <div className="mb-3 flex items-center justify-between gap-3">
              <div className="font-semibold text-foreground">Recent diagnostic events</div>
              <span className="text-xs text-muted-foreground">{diagnostics?.recentEvents.length ?? 0} events</span>
            </div>
            <div className="max-h-48 space-y-2 overflow-auto">
              {diagnostics?.recentEvents.length ? (
                diagnostics.recentEvents.slice().reverse().map((event) => (
                  <div key={`${event.timestamp}-${event.category}-${event.message}`} className="rounded border border-border bg-background px-3 py-2 text-sm">
                    <div className="flex items-center justify-between gap-3">
                      <span className={`font-semibold ${diagnosticLevelClass(event.level)}`}>{event.level.toUpperCase()}</span>
                      <span className="text-xs tabular-nums text-muted-foreground">{formatDiagnosticEventTime(event.timestamp)}</span>
                    </div>
                    <div className="mt-1 truncate text-foreground" title={event.message}>{event.message}</div>
                    <div className="mt-1 text-xs text-muted-foreground">
                      {event.category}{event.jobId ? ` / ${event.jobId}` : ''}
                    </div>
                  </div>
                ))
              ) : (
                <div className="rounded border border-border bg-background px-3 py-6 text-center text-sm text-muted-foreground">
                  No diagnostic events recorded.
                </div>
              )}
            </div>
          </div>
        </SettingsPanel>
        </section>
      </div>
    </form>
    {isExcludedSitesDialogOpen ? (
      <ExcludedSitesDialog
        enabled={formData.extensionIntegration.enabled}
        hosts={formData.extensionIntegration.excludedHosts}
        singleInput={excludedHostInput}
        bulkInput={excludedBulkInput}
        searchQuery={excludedSearchQuery}
        onSingleInputChange={setExcludedHostInput}
        onBulkInputChange={setExcludedBulkInput}
        onSearchQueryChange={setExcludedSearchQuery}
        onAddSingle={handleAddExcludedHost}
        onAddBulk={handleAddBulkExcludedHosts}
        onRemove={handleRemoveExcludedHost}
        onClearAll={() => updateExtensionIntegration({ excludedHosts: [] })}
        onClose={() => setIsExcludedSitesDialogOpen(false)}
      />
    ) : null}
    </>
  );
}

function ExcludedSitesDialog({
  enabled,
  hosts,
  singleInput,
  bulkInput,
  searchQuery,
  onSingleInputChange,
  onBulkInputChange,
  onSearchQueryChange,
  onAddSingle,
  onAddBulk,
  onRemove,
  onClearAll,
  onClose,
}: {
  enabled: boolean;
  hosts: string[];
  singleInput: string;
  bulkInput: string;
  searchQuery: string;
  onSingleInputChange: (value: string) => void;
  onBulkInputChange: (value: string) => void;
  onSearchQueryChange: (value: string) => void;
  onAddSingle: () => void;
  onAddBulk: () => void;
  onRemove: (host: string) => void;
  onClearAll: () => void;
  onClose: () => void;
}) {
  const filteredHosts = filterExcludedHosts(hosts, searchQuery);
  const bulkCandidates = parseExcludedHostInput(bulkInput);
  const canAddSingle = enabled && Boolean(normalizeHostInput(singleInput));
  const canAddBulk = enabled && bulkCandidates.length > 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-6 py-8">
      <section
        role="dialog"
        aria-modal="true"
        aria-labelledby="excludedSitesTitle"
        className="flex max-h-full w-full max-w-2xl flex-col overflow-hidden rounded-md border border-border bg-card shadow-2xl"
      >
        <header className="flex h-12 items-center justify-between border-b border-border bg-header px-4">
          <div className="min-w-0">
            <h2 id="excludedSitesTitle" className="text-base font-semibold text-foreground">
              Excluded Sites
            </h2>
            <p className="text-xs text-muted-foreground">{formatExcludedSitesSummary(hosts)}</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition hover:bg-muted hover:text-foreground"
            aria-label="Close excluded sites"
          >
            <X size={16} />
          </button>
        </header>

        <div className="min-h-0 flex-1 space-y-4 overflow-auto p-4">
          <div className="grid grid-cols-3 gap-2">
            <MetricCard label="Total" value={hosts.length.toString()} />
            <MetricCard label="Visible" value={filteredHosts.length.toString()} />
            <MetricCard label="Pending" value={bulkCandidates.length.toString()} />
          </div>

          <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-2">
            <input
              type="text"
              value={singleInput}
              onChange={(event) => onSingleInputChange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  onAddSingle();
                }
              }}
              placeholder="*.example.com"
              disabled={!enabled}
              className="h-9 min-w-0 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
            />
            <button
              type="button"
              onClick={onAddSingle}
              disabled={!canAddSingle}
              className="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Plus size={15} />
              Add
            </button>
          </div>

          <div className="space-y-2">
            <div className="flex items-center justify-between gap-3">
              <label className="text-sm font-semibold text-foreground" htmlFor="excludedSitesBulk">
                Bulk Add
              </label>
              <span className="text-xs text-muted-foreground">One host, wildcard host, or URL per line</span>
            </div>
            <textarea
              id="excludedSitesBulk"
              value={bulkInput}
              onChange={(event) => onBulkInputChange(event.target.value)}
              disabled={!enabled}
              placeholder={'cdn.example.com\n*.example.org\nhttps://mirror.example.org/file.zip'}
              className="h-24 w-full resize-none rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground outline-none transition placeholder:text-muted-foreground focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
            />
            <div className="flex items-center justify-between gap-3">
              <span className="text-xs text-muted-foreground">{bulkCandidates.length} pending host patterns</span>
              <button
                type="button"
                onClick={onAddBulk}
                disabled={!canAddBulk}
                className="flex h-8 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
              >
                <Plus size={14} />
                Add Bulk
              </button>
            </div>
          </div>

          <div className="space-y-2">
            <label className="text-sm font-semibold text-foreground" htmlFor="excludedSitesSearch">
              Current Sites
            </label>
            <div className="relative">
              <Search size={15} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground" />
              <input
                id="excludedSitesSearch"
                type="text"
                value={searchQuery}
                onChange={(event) => onSearchQueryChange(event.target.value)}
                placeholder="Search hosts"
                className="h-9 w-full rounded-md border border-input bg-background pl-9 pr-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
              />
            </div>
            <div className="max-h-56 overflow-auto rounded-md border border-border bg-surface">
              {filteredHosts.length > 0 ? (
                filteredHosts.map((host) => (
                  <div key={host} className="flex h-10 items-center justify-between gap-3 border-b border-border/70 px-3 last:border-b-0">
                    <div className="flex min-w-0 items-center gap-2">
                      <Ban size={14} className="shrink-0 text-muted-foreground" />
                      <span className="truncate text-sm font-medium text-foreground">{host}</span>
                    </div>
                    <button
                      type="button"
                      onClick={() => onRemove(host)}
                      disabled={!enabled}
                      className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition hover:bg-muted hover:text-destructive disabled:cursor-not-allowed disabled:opacity-50"
                      title={`Remove ${host}`}
                      aria-label={`Remove ${host}`}
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                ))
              ) : (
                <div className="px-3 py-6 text-center text-sm text-muted-foreground">
                  {hosts.length === 0 ? 'No excluded sites.' : 'No matching sites.'}
                </div>
              )}
            </div>
          </div>
        </div>

        <footer className="flex items-center justify-between border-t border-border bg-card px-4 py-3">
          <button
            type="button"
            onClick={onClearAll}
            disabled={!enabled || hosts.length === 0}
            className="flex h-9 items-center gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 text-sm font-medium text-destructive transition hover:bg-destructive/15 disabled:cursor-not-allowed disabled:opacity-50"
          >
            <Trash2 size={15} />
            Clear All
          </button>
          <button type="button" onClick={onClose} className="h-9 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition hover:bg-primary/90">
            Done
          </button>
        </footer>
      </section>
    </div>
  );
}

function MetricCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border bg-surface px-3 py-2">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-1 text-lg font-semibold tabular-nums text-foreground">{value}</div>
    </div>
  );
}

function SettingsNavLink({ href, label }: { href: string; label: string }) {
  return (
    <a
      href={href}
      className="flex h-8 items-center rounded-md px-2.5 text-xs font-medium text-muted-foreground transition hover:bg-muted hover:text-foreground"
    >
      {label}
    </a>
  );
}

function SettingsPanel({
  icon,
  title,
  action,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-md border border-border bg-card">
      <header className="flex min-h-11 items-center justify-between gap-3 border-b border-border bg-header px-4 py-2">
        <div className="flex items-center gap-2 font-semibold text-foreground">
          <span className="text-primary">{icon}</span>
          {title}
        </div>
        {action}
      </header>
      <div className="space-y-3 p-4">{children}</div>
    </section>
  );
}

function FieldRow({
  label,
  description,
  tooltip,
  children,
}: {
  label: string;
  description: string;
  tooltip?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="grid grid-cols-[minmax(160px,220px)_minmax(0,1fr)] items-center gap-4">
      <div>
        <label className="text-sm font-semibold text-foreground">{label}</label>
        <p className="mt-0.5 text-xs leading-4 text-muted-foreground" title={tooltip ?? description}>
          {description}
        </p>
      </div>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function ToggleSwitch({
  id,
  checked,
  onChange,
  disabled = false,
}: {
  id: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <label htmlFor={id} className={`relative inline-flex items-center ${disabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'}`}>
      <input
        type="checkbox"
        id={id}
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
        disabled={disabled}
        className="peer sr-only"
      />
      <span className="h-6 w-11 rounded-full bg-muted transition peer-checked:bg-primary" />
      <span className="absolute left-0.5 top-0.5 h-5 w-5 rounded-full border border-border bg-white transition peer-checked:translate-x-5" />
    </label>
  );
}

function CompactSetting({
  icon,
  title,
  description,
  tooltip,
  control,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  tooltip?: string;
  control: React.ReactNode;
}) {
  return (
    <div className="flex min-h-16 items-center justify-between gap-3 rounded-md border border-border bg-surface px-3 py-2">
      <div className="flex min-w-0 items-start gap-3">
        <span className="mt-0.5 text-primary">{icon}</span>
        <div className="min-w-0">
          <div className="text-sm font-semibold text-foreground">{title}</div>
          <div className="mt-0.5 text-xs leading-4 text-muted-foreground" title={tooltip ?? description}>
            {description}
          </div>
        </div>
      </div>
      <div className="shrink-0">{control}</div>
    </div>
  );
}

function UtilityButton({
  icon,
  label,
  onClick,
  primary = false,
  disabled = false,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  primary?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`flex h-9 items-center gap-2 rounded-md px-3 text-sm font-medium transition ${
        primary
          ? 'bg-primary text-primary-foreground hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50'
          : 'border border-input bg-background text-foreground hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50'
      }`}
    >
      {icon}
      {label}
    </button>
  );
}

function renderUpdateStatus(updateState: AppUpdateState): string {
  if (updateState.status === 'checking') return 'Checking GitHub Releases for a newer alpha build.';
  if (updateState.status === 'available' && updateState.availableUpdate) {
    return `Version ${updateState.availableUpdate.version} is available.`;
  }
  if (updateState.status === 'not_available') return 'You are running the latest alpha build.';
  if (updateState.status === 'downloading') return 'Downloading the signed update package.';
  if (updateState.status === 'installing') return 'Installing the update. The app may close automatically.';
  if (updateState.status === 'error') return 'The last update action failed.';
  return 'Checks the signed alpha feed hosted on GitHub Releases.';
}

function VersionIndicator({
  label,
  value,
  tone = 'current',
}: {
  label: string;
  value: string;
  tone?: AppUpdateVersionTone;
}) {
  return (
    <div className="min-w-0 border-l border-border pl-3">
      <div className="text-[11px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">{label}</div>
      <div className={`mt-1 truncate text-sm font-semibold tabular-nums ${versionIndicatorToneClass(tone)}`} title={value}>
        {value}
      </div>
    </div>
  );
}

function versionIndicatorToneClass(tone: AppUpdateVersionTone): string {
  switch (tone) {
    case 'available':
      return 'text-primary';
    case 'error':
      return 'text-destructive';
    case 'pending':
      return 'text-muted-foreground';
    case 'current':
    default:
      return 'text-foreground';
  }
}

function updateProgressPercent(updateState: AppUpdateState): number {
  if (!updateState.totalBytes || updateState.totalBytes <= 0) return 0;
  return Math.max(0, Math.min(100, (updateState.downloadedBytes / updateState.totalBytes) * 100));
}

function formatUpdateProgress(updateState: AppUpdateState): string {
  if (!updateState.totalBytes) return `${formatCompactBytes(updateState.downloadedBytes)} downloaded`;
  return `${formatCompactBytes(updateState.downloadedBytes)} / ${formatCompactBytes(updateState.totalBytes)}`;
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

function DiagnosticRow({ label, value, muted = false }: { label: string; value: string; muted?: boolean }) {
  return (
    <div className="grid grid-cols-[110px_minmax(0,1fr)] gap-3 py-1 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <span className={`break-all font-mono text-xs ${muted ? 'text-muted-foreground' : 'text-foreground'}`}>{value}</span>
    </div>
  );
}

function renderRegistrationIcon(status?: DiagnosticsSnapshot['hostRegistration']['status']) {
  switch (status) {
    case 'configured':
      return <ShieldCheck size={24} className="text-success" />;
    case 'broken':
      return <ShieldAlert size={24} className="text-warning" />;
    default:
      return <ShieldX size={24} className="text-muted-foreground" />;
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

function diagnosticLevelClass(level: DiagnosticsSnapshot['recentEvents'][number]['level']) {
  switch (level) {
    case 'error':
      return 'text-destructive';
    case 'warning':
      return 'text-warning';
    default:
      return 'text-success';
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
