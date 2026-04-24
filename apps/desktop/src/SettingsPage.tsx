import React, { useEffect, useState } from 'react';
import type { DiagnosticsSnapshot, Settings } from './types';
import {
  Activity,
  Copy,
  Download,
  ExternalLink,
  FolderOpen,
  Palette,
  RefreshCw,
  Save,
  Settings2,
  ShieldAlert,
  ShieldCheck,
  ShieldX,
  Wrench,
} from 'lucide-react';

interface SettingsPageProps {
  settings: Settings;
  diagnostics: DiagnosticsSnapshot | null;
  onSave: (settings: Settings) => void;
  onBrowseDirectory: () => Promise<string | null | void>;
  onCancel: () => void;
  onRefreshDiagnostics: () => void;
  onOpenInstallDocs: () => void;
  onRunHostRegistrationFix: () => void;
  onCopyDiagnostics: () => void;
  onExportDiagnostics: () => void;
}

export function SettingsPage({
  settings,
  diagnostics,
  onSave,
  onBrowseDirectory,
  onCancel,
  onRefreshDiagnostics,
  onOpenInstallDocs,
  onRunHostRegistrationFix,
  onCopyDiagnostics,
  onExportDiagnostics,
}: SettingsPageProps) {
  const [formData, setFormData] = useState<Settings>(settings);

  useEffect(() => {
    setFormData(settings);
  }, [settings]);

  const handleChange = (event: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) => {
    const { name, value, type } = event.target;
    const nextValue = type === 'checkbox' ? (event.target as HTMLInputElement).checked : value;

    setFormData((prev) => ({
      ...prev,
      [name]: type === 'number' ? parseInt(value, 10) : nextValue,
    }));
  };

  const handleSubmit = (event: React.FormEvent) => {
    event.preventDefault();
    onSave(formData);
  };

  const handleBrowseDirectory = async () => {
    const selectedDirectory = await onBrowseDirectory();
    if (!selectedDirectory) return;
    setFormData((prev) => ({ ...prev, downloadDirectory: selectedDirectory }));
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
          <FieldRow label="Download Directory" description="Files are saved here by default.">
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

          <FieldRow label="Max Concurrent Downloads" description="Limits how many jobs may run at once.">
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
        </SettingsPanel>

        <SettingsPanel icon={<Palette size={20} />} title="Appearance & Behavior">
          <FieldRow label="Desktop Notifications" description="Show a native notification when downloads finish or fail.">
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

          <FieldRow label="App Theme" description="Theme switching stays in Settings and applies to the entire shell.">
            <select
              id="theme"
              name="theme"
              value={formData.theme}
              onChange={handleChange}
              className="h-10 w-56 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
            >
              <option value="light">Light Mode</option>
              <option value="dark">Dark Mode</option>
              <option value="system">System Default</option>
            </select>
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
  children,
}: {
  label: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <div className="grid grid-cols-[minmax(180px,260px)_minmax(0,1fr)] items-center gap-5">
      <div>
        <label className="text-sm font-semibold text-foreground">{label}</label>
        <p className="mt-1 text-sm leading-5 text-muted-foreground">{description}</p>
      </div>
      <div className="min-w-0">{children}</div>
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
