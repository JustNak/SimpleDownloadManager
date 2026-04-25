import React, { useEffect, useMemo, useState } from 'react';
import type { DiagnosticsSnapshot, Settings } from './types';
import {
  Activity,
  Ban,
  Check,
  Copy,
  Download,
  ExternalLink,
  FolderOpen,
  Globe,
  MousePointerClick,
  Palette,
  PlugZap,
  Plus,
  RefreshCw,
  Save,
  Settings2,
  ShieldAlert,
  ShieldCheck,
  ShieldX,
  TestTube2,
  Trash2,
  Wrench,
} from 'lucide-react';

const ACCENT_COLOR_PRESETS = [
  { name: 'Blue', value: '#3b82f6' },
  { name: 'Cyan', value: '#06b6d4' },
  { name: 'Green', value: '#22c55e' },
  { name: 'Amber', value: '#f59e0b' },
  { name: 'Rose', value: '#f43f5e' },
  { name: 'Violet', value: '#8b5cf6' },
];

interface SettingsPageProps {
  settings: Settings;
  diagnostics: DiagnosticsSnapshot | null;
  onSave: (settings: Settings) => void | Promise<void | boolean>;
  onBrowseDirectory: () => Promise<string | null | void>;
  onCancel: () => void;
  onDirtyChange?: (isDirty: boolean) => void;
  onDraftChange?: (settings: Settings) => void;
  onRefreshDiagnostics: () => void;
  onOpenInstallDocs: () => void;
  onRunHostRegistrationFix: () => void;
  onTestExtensionHandoff: () => void;
  onCopyDiagnostics: () => void;
  onExportDiagnostics: () => void;
}

export function SettingsPage({
  settings,
  diagnostics,
  onSave,
  onBrowseDirectory,
  onCancel,
  onDirtyChange,
  onDraftChange,
  onRefreshDiagnostics,
  onOpenInstallDocs,
  onRunHostRegistrationFix,
  onTestExtensionHandoff,
  onCopyDiagnostics,
  onExportDiagnostics,
}: SettingsPageProps) {
  const [formData, setFormData] = useState<Settings>(settings);
  const [excludedHostInput, setExcludedHostInput] = useState('');
  const [accentColorInput, setAccentColorInput] = useState(normalizeAccentColor(settings.accentColor));

  useEffect(() => {
    setFormData(settings);
    setExcludedHostInput('');
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
    setFormData((prev) => ({ ...prev, downloadDirectory: selectedDirectory }));
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
    const normalizedHost = normalizeHostInput(excludedHostInput);
    if (!normalizedHost) return;

    setFormData((prev) => {
      if (prev.extensionIntegration.excludedHosts.includes(normalizedHost)) {
        return prev;
      }

      return {
        ...prev,
        extensionIntegration: {
          ...prev.extensionIntegration,
          excludedHosts: [...prev.extensionIntegration.excludedHosts, normalizedHost],
        },
      };
    });
    setExcludedHostInput('');
  };

  const handleRemoveExcludedHost = (host: string) => {
    setFormData((prev) => ({
      ...prev,
      extensionIntegration: {
        ...prev.extensionIntegration,
        excludedHosts: prev.extensionIntegration.excludedHosts.filter((candidate) => candidate !== host),
      },
    }));
  };

  return (
    <form onSubmit={handleSubmit} className="settings-surface mx-auto flex w-full max-w-4xl flex-col gap-5 p-6">
      <header className="flex items-center justify-between border-b border-border pb-5">
        <div>
          <h1 className="text-2xl font-semibold tracking-normal text-foreground">Settings</h1>
          <p className="mt-1 text-sm text-muted-foreground">Configure downloads, appearance, notifications, and native host diagnostics.</p>
        </div>
        <div className="flex items-center gap-2">
          <button type="button" onClick={onCancel} className="h-10 rounded-md px-4 text-sm font-medium text-foreground transition hover:bg-muted">
            Cancel
          </button>
          <button type="submit" className="flex h-10 items-center gap-2 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground transition hover:bg-primary/90">
            <Save size={16} />
            Save Changes
          </button>
        </div>
      </header>

      <div className="space-y-5">
        <SettingsPanel icon={<Settings2 size={20} />} title="General">
          <FieldRow label="Download Directory" description="Default save path." tooltip="Files are saved here by default.">
            <div className="flex min-w-0 gap-2">
              <input
                type="text"
                id="downloadDirectory"
                name="downloadDirectory"
                value={formData.downloadDirectory}
                readOnly
                className="min-w-0 flex-1 rounded-md border border-input bg-background px-3 py-2 text-sm text-muted-foreground outline-none"
              />
              <button
                type="button"
                onClick={() => void handleBrowseDirectory()}
                className="flex h-10 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted"
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
              className="h-10 w-28 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
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
              className="h-10 w-28 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
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
                className="h-10 w-32 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
              />
              <span className="text-sm text-muted-foreground">KB/s</span>
            </div>
          </FieldRow>
        </SettingsPanel>

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

          <FieldRow label="App Theme" description="Shell theme." tooltip="Theme switching stays in Settings and applies to the entire shell.">
            <select
              id="theme"
              name="theme"
              value={formData.theme}
              onChange={handleChange}
              className="h-10 w-56 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
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

              <label className="flex h-10 items-center gap-3 rounded-md border border-input bg-background px-3 text-sm text-foreground">
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
              className="h-10 w-60 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
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
                className="h-10 w-32 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
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

          <FieldRow label="Excluded Sites" description="Browser-only hosts." tooltip="Downloads from these hostnames stay in the browser.">
            <div className="space-y-3">
              <div className="flex min-w-0 gap-2">
                <input
                  type="text"
                  value={excludedHostInput}
                  onChange={(event) => setExcludedHostInput(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') {
                      event.preventDefault();
                      handleAddExcludedHost();
                    }
                  }}
                  placeholder="example.com"
                  disabled={!formData.extensionIntegration.enabled}
                  className="min-w-0 flex-1 rounded-md border border-input bg-background px-3 py-2 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20 disabled:cursor-not-allowed disabled:opacity-50"
                />
                <button
                  type="button"
                  onClick={handleAddExcludedHost}
                  disabled={!formData.extensionIntegration.enabled || !normalizeHostInput(excludedHostInput)}
                  className="flex h-10 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <Plus size={16} />
                  Add
                </button>
              </div>

              {formData.extensionIntegration.excludedHosts.length > 0 ? (
                <div className="flex flex-wrap gap-2">
                  {formData.extensionIntegration.excludedHosts.map((host) => (
                    <span key={host} className="inline-flex h-8 items-center gap-2 rounded-md border border-border bg-surface px-2.5 text-sm text-foreground">
                      <Ban size={14} className="text-muted-foreground" />
                      {host}
                      <button
                        type="button"
                        onClick={() => handleRemoveExcludedHost(host)}
                        className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground transition hover:bg-muted hover:text-destructive"
                        title={`Remove ${host}`}
                        aria-label={`Remove ${host}`}
                      >
                        <Trash2 size={13} />
                      </button>
                    </span>
                  ))}
                </div>
              ) : (
                <div className="rounded-md border border-dashed border-border bg-surface px-3 py-2 text-sm text-muted-foreground">
                  No excluded sites.
                </div>
              )}
            </div>
          </FieldRow>
        </SettingsPanel>

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
        </SettingsPanel>
      </div>
    </form>
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
      <header className="flex min-h-14 items-center justify-between gap-3 border-b border-border bg-header px-5 py-3">
        <div className="flex items-center gap-3 font-semibold text-foreground">
          <span className="text-primary">{icon}</span>
          {title}
        </div>
        {action}
      </header>
      <div className="space-y-4 p-5">{children}</div>
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
    <div className="grid grid-cols-[minmax(180px,260px)_minmax(0,1fr)] items-center gap-5">
      <div>
        <label className="text-sm font-semibold text-foreground">{label}</label>
        <p className="mt-1 text-sm leading-5 text-muted-foreground" title={tooltip ?? description}>
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
    <div className="flex min-h-20 items-center justify-between gap-3 rounded-md border border-border bg-surface px-4 py-3">
      <div className="flex min-w-0 items-start gap-3">
        <span className="mt-0.5 text-primary">{icon}</span>
        <div className="min-w-0">
          <div className="text-sm font-semibold text-foreground">{title}</div>
          <div className="mt-1 text-sm leading-5 text-muted-foreground" title={tooltip ?? description}>
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
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  primary?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex h-10 items-center gap-2 rounded-md px-3 text-sm font-medium transition ${
        primary
          ? 'bg-primary text-primary-foreground hover:bg-primary/90'
          : 'border border-input bg-background text-foreground hover:bg-muted'
      }`}
    >
      {icon}
      {label}
    </button>
  );
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

function normalizeHostInput(value: string): string {
  return value
    .trim()
    .replace(/^https?:\/\//i, '')
    .replace(/\/.*$/, '')
    .toLowerCase();
}

function normalizeAccentColor(value: string | undefined): string {
  const color = value?.trim() ?? '';
  return /^#[0-9a-f]{6}$/i.test(color) ? color.toLowerCase() : '#3b82f6';
}

function normalizeListenPort(value: string): number {
  const port = Number.parseInt(value, 10);
  return Number.isFinite(port) && port >= 1 && port <= 65535 ? port : 1420;
}
