import React, { useEffect, useMemo, useRef, useState } from 'react';
import { addJob, addJobs } from './backend';
import type { AddJobResult } from './backend';
import { Archive, Link2, ListPlus, PackagePlus, X } from 'lucide-react';
import { getErrorMessage } from './errors';

type DownloadMode = 'single' | 'multi' | 'bulk';

interface AddDownloadModalProps {
  onClose: () => void;
  onAdded: (result: AddJobResult) => void;
}

export function AddDownloadModal({ onClose, onAdded }: AddDownloadModalProps) {
  const [mode, setMode] = useState<DownloadMode>('single');
  const [singleUrl, setSingleUrl] = useState('');
  const [multiUrls, setMultiUrls] = useState('');
  const [bulkUrls, setBulkUrls] = useState('');
  const [archiveName, setArchiveName] = useState('bulk-download.zip');
  const [combineBulk, setCombineBulk] = useState(true);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');
  const inputRef = useRef<HTMLInputElement | HTMLTextAreaElement>(null);

  const activeUrls = useMemo(() => {
    if (mode === 'single') return singleUrl.trim() ? [singleUrl.trim()] : [];
    return parseUrlLines(mode === 'multi' ? multiUrls : bulkUrls);
  }, [bulkUrls, mode, multiUrls, singleUrl]);

  useEffect(() => {
    inputRef.current?.focus();
  }, [mode]);

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (activeUrls.length === 0) return;

    setIsSubmitting(true);
    setErrorMessage('');

    try {
      if (mode === 'single') {
        const result = await addJob(activeUrls[0]);
        onAdded(result);
      } else {
        const result = await addJobs(activeUrls, mode === 'bulk' && combineBulk ? archiveName : undefined);
        if (result.results[0]) onAdded(result.results[0]);
      }

      onClose();
    } catch (error) {
      setErrorMessage(getErrorMessage(error, 'Failed to add downloads.'));
    } finally {
      setIsSubmitting(false);
    }
  };

  const submitLabel = mode === 'single'
    ? 'Start Download'
    : mode === 'multi'
      ? `Queue ${activeUrls.length || ''} Downloads`.trim()
      : combineBulk
        ? `Queue ${activeUrls.length || ''} and Combine`.trim()
        : `Queue ${activeUrls.length || ''} Downloads`.trim();

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/60 p-4 backdrop-blur-[1px]">
      <div className="w-full max-w-2xl overflow-hidden rounded-md border border-border bg-card shadow-2xl animate-in fade-in zoom-in-95 duration-200">
        <div className="flex items-center justify-between border-b border-border bg-header px-5 py-3">
          <div>
            <h2 className="text-base font-semibold text-foreground">New Download</h2>
            <p className="mt-0.5 text-xs text-muted-foreground">Add a single file, queue multiple links, or prepare a combined bulk archive.</p>
          </div>
          <button
            onClick={onClose}
            className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            aria-label="Close new download"
            title="Close"
          >
            <X size={18} />
          </button>
        </div>

        <form onSubmit={handleSubmit}>
          <div className="border-b border-border px-5 py-4">
            <div className="grid grid-cols-3 rounded-md border border-border bg-background p-1">
              <ModeButton icon={<Link2 size={16} />} label="Single Download" active={mode === 'single'} onClick={() => setMode('single')} />
              <ModeButton icon={<ListPlus size={16} />} label="Multi-Download" active={mode === 'multi'} onClick={() => setMode('multi')} />
              <ModeButton icon={<PackagePlus size={16} />} label="Bulk Download" active={mode === 'bulk'} onClick={() => setMode('bulk')} />
            </div>
          </div>

          <div className="space-y-4 px-5 py-5">
            {mode === 'single' ? (
              <Field label="Download URL" hint="Queue one direct HTTP or HTTPS file link.">
                <input
                  ref={inputRef as React.RefObject<HTMLInputElement>}
                  type="url"
                  value={singleUrl}
                  onChange={(event) => setSingleUrl(event.target.value)}
                  placeholder="https://example.com/file.zip"
                  required
                  className="h-10 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
                />
              </Field>
            ) : null}

            {mode === 'multi' ? (
              <Field label="Download URLs" hint="Paste one link per line. Each link becomes its own queue item.">
                <textarea
                  ref={inputRef as React.RefObject<HTMLTextAreaElement>}
                  value={multiUrls}
                  onChange={(event) => setMultiUrls(event.target.value)}
                  placeholder={'https://example.com/file-01.zip\nhttps://example.com/file-02.zip'}
                  rows={7}
                  className="w-full resize-none rounded-md border border-input bg-background px-3 py-2.5 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
                />
              </Field>
            ) : null}

            {mode === 'bulk' ? (
              <>
                <Field label="Bulk Links" hint="Paste one link per line. The batch can be grouped under a single archive output name.">
                  <textarea
                    ref={inputRef as React.RefObject<HTMLTextAreaElement>}
                    value={bulkUrls}
                    onChange={(event) => setBulkUrls(event.target.value)}
                    placeholder={'https://example.com/assets/model.fbx\nhttps://example.com/assets/textures.zip\nhttps://example.com/assets/readme.pdf'}
                    rows={7}
                    className="w-full resize-none rounded-md border border-input bg-background px-3 py-2.5 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
                  />
                </Field>

                <div className="grid gap-3 rounded-md border border-border bg-background p-3 md:grid-cols-[1fr_220px]">
                  <label className="flex items-start gap-3 text-sm">
                    <input
                      type="checkbox"
                      checked={combineBulk}
                      onChange={(event) => setCombineBulk(event.target.checked)}
                      className="mt-1 h-4 w-4 accent-primary"
                    />
                    <span>
                      <span className="flex items-center gap-2 font-medium text-foreground">
                        <Archive size={16} />
                        Combine into one archive
                      </span>
                      <span className="mt-1 block text-xs leading-5 text-muted-foreground">
                        Links are queued together with an archive name so the batch can be collected as one compressed output.
                      </span>
                    </span>
                  </label>

                  <input
                    value={archiveName}
                    onChange={(event) => setArchiveName(normalizeArchiveName(event.target.value))}
                    disabled={!combineBulk}
                    className="h-9 rounded-md border border-input bg-card px-3 text-sm text-foreground outline-none transition focus:border-primary disabled:cursor-not-allowed disabled:opacity-50"
                    aria-label="Archive file name"
                  />
                </div>
              </>
            ) : null}

            <div className="flex items-center justify-between rounded-md border border-border bg-background px-3 py-2 text-xs text-muted-foreground">
              <span>{activeUrls.length} {activeUrls.length === 1 ? 'link' : 'links'} ready</span>
              <span>{mode === 'bulk' && combineBulk ? archiveName : 'Queue only'}</span>
            </div>

            {errorMessage ? (
              <p className="rounded-md border border-destructive/35 bg-destructive/10 px-3 py-2 text-sm text-destructive">{errorMessage}</p>
            ) : null}
          </div>

          <div className="flex justify-end gap-3 border-t border-border px-5 py-4">
            <button
              type="button"
              onClick={onClose}
              className="h-9 rounded-md px-4 text-sm font-semibold text-foreground transition-colors hover:bg-muted"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={isSubmitting || activeUrls.length === 0}
              className="h-9 rounded-md bg-primary px-4 text-sm font-semibold text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {isSubmitting ? 'Adding...' : submitLabel}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

function ModeButton({
  icon,
  label,
  active,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex h-9 items-center justify-center gap-2 rounded-[4px] text-sm font-semibold transition ${
        active ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:bg-muted hover:text-foreground'
      }`}
    >
      {icon}
      <span className="truncate">{label}</span>
    </button>
  );
}

function Field({ label, hint, children }: { label: string; hint: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="mb-2 flex items-end justify-between gap-3">
        <label className="text-sm font-medium text-foreground">{label}</label>
        <span className="text-xs text-muted-foreground">{hint}</span>
      </div>
      {children}
    </div>
  );
}

function parseUrlLines(value: string) {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function normalizeArchiveName(value: string) {
  const sanitized = value.replace(/[<>:"/\\|?*\u0000-\u001F]/g, '').trimStart();
  if (!sanitized) return '';
  return sanitized.toLowerCase().endsWith('.zip') ? sanitized : `${sanitized}.zip`;
}
