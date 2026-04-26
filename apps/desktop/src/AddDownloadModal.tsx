import React, { useEffect, useMemo, useRef, useState } from 'react';
import { addJob, addJobs, browseTorrentFile } from './backend';
import type { AddJobResult, AddJobsResult } from './backend';
import { progressPopupIntentForSubmission, type ProgressPopupIntent } from './batchProgress';
import {
  batchUrlTextAreaClassName,
  batchUrlTextAreaWrap,
  downloadSubmitLabel,
  ensureTrailingEditableLine,
  parseDownloadUrlLines,
  type DownloadMode,
} from './downloadInput';
import { Archive, Link2, ListPlus, Magnet, PackagePlus, X } from 'lucide-react';
import { getErrorMessage } from './errors';
import { validateOptionalSha256 } from './downloadIntegrity';

export type { DownloadMode } from './downloadInput';

export interface AddDownloadOutcome {
  mode: DownloadMode;
  intent: ProgressPopupIntent | null;
  primaryResult?: AddJobResult;
  result: AddJobResult | AddJobsResult;
}

interface AddDownloadModalProps {
  onClose: () => void;
  onAdded: (outcome: AddDownloadOutcome) => void;
}

export function AddDownloadModal({ onClose, onAdded }: AddDownloadModalProps) {
  const [mode, setMode] = useState<DownloadMode>('single');
  const [singleUrl, setSingleUrl] = useState('');
  const [torrentUrl, setTorrentUrl] = useState('');
  const [singleSha256, setSingleSha256] = useState('');
  const [multiUrls, setMultiUrls] = useState('');
  const [bulkUrls, setBulkUrls] = useState('');
  const [archiveName, setArchiveName] = useState('bulk-download.zip');
  const [combineBulk, setCombineBulk] = useState(true);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [isImportingTorrent, setIsImportingTorrent] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');
  const inputRef = useRef<HTMLInputElement | HTMLTextAreaElement>(null);

  const activeUrls = useMemo(() => {
    if (mode === 'single') return singleUrl.trim() ? [singleUrl.trim()] : [];
    if (mode === 'torrent') return torrentUrl.trim() ? [torrentUrl.trim()] : [];
    return parseDownloadUrlLines(mode === 'multi' ? multiUrls : bulkUrls);
  }, [bulkUrls, mode, multiUrls, singleUrl, torrentUrl]);

  useEffect(() => {
    inputRef.current?.focus();
  }, [mode]);

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (activeUrls.length === 0) return;

    setIsSubmitting(true);
    setErrorMessage('');

    try {
      if (mode === 'single' || mode === 'torrent') {
        const result = await addJob(activeUrls[0], {
          expectedSha256: mode === 'single' ? validateOptionalSha256(singleSha256) : null,
          transferKind: mode === 'torrent' ? 'torrent' : 'http',
        });
        onAdded({
          mode,
          intent: progressPopupIntentForSubmission(mode, result),
          primaryResult: result,
          result,
        });
      } else {
        const bulkArchiveName = mode === 'bulk' && combineBulk ? archiveName : undefined;
        const result = await addJobs(activeUrls, bulkArchiveName);
        onAdded({
          mode,
          intent: progressPopupIntentForSubmission(mode, result, bulkArchiveName),
          primaryResult: result.results.find((item) => item.status === 'queued') ?? result.results[0],
          result,
        });
      }

      onClose();
    } catch (error) {
      setErrorMessage(getErrorMessage(error, 'Failed to add downloads.'));
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleImportTorrent = async () => {
    setIsImportingTorrent(true);
    setErrorMessage('');

    try {
      const imported = await browseTorrentFile();
      if (imported) {
        setMode('torrent');
        setTorrentUrl(imported);
      }
    } catch (error) {
      setErrorMessage(getErrorMessage(error, 'Failed to import torrent file.'));
    } finally {
      setIsImportingTorrent(false);
    }
  };

  const handleBackdropMouseDown = (event: React.MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) {
      onClose();
    }
  };

  const submitLabel = downloadSubmitLabel(mode, activeUrls.length, combineBulk);
  const readyLabel = mode === 'torrent'
    ? `${activeUrls.length} ${activeUrls.length === 1 ? 'torrent' : 'torrents'} ready`
    : `${activeUrls.length} ${activeUrls.length === 1 ? 'link' : 'links'} ready`;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-background/60 p-4 backdrop-blur-[1px]"
      onMouseDown={handleBackdropMouseDown}
    >
      <div className="w-full max-w-xl overflow-hidden rounded-md border border-border bg-card shadow-2xl animate-in fade-in zoom-in-95 duration-200">
        <div className="flex items-center justify-between border-b border-border bg-header px-5 py-3">
          <div>
            <h2 className="text-base font-semibold text-foreground">New Download</h2>
            <p className="mt-0.5 text-xs text-muted-foreground">Add a file, torrent, link list, or bulk archive.</p>
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
          <div className="border-b border-border px-5 py-3">
            <div className="grid grid-cols-4 rounded-md border border-border bg-background p-1">
              <ModeButton icon={<Link2 size={15} />} label="File" active={mode === 'single'} onClick={() => setMode('single')} />
              <ModeButton icon={<Magnet size={15} />} label="Torrent" active={mode === 'torrent'} onClick={() => setMode('torrent')} />
              <ModeButton icon={<ListPlus size={15} />} label="Multi" active={mode === 'multi'} onClick={() => setMode('multi')} />
              <ModeButton icon={<PackagePlus size={15} />} label="Bulk" active={mode === 'bulk'} onClick={() => setMode('bulk')} />
            </div>
          </div>

          <div className="space-y-3 px-5 py-4">
            {mode === 'single' ? (
              <Field label="File URL" hint="HTTP(S) direct download.">
                <input
                  ref={inputRef as React.RefObject<HTMLInputElement>}
                  type="url"
                  value={singleUrl}
                  onChange={(event) => setSingleUrl(event.target.value)}
                  placeholder="https://example.com/file.zip"
                  required
                  className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
                />
              </Field>
            ) : null}

            {mode === 'single' ? (
              <Field label="SHA-256 Checksum" hint="Optional integrity check after download.">
                <input
                  type="text"
                  value={singleSha256}
                  onChange={(event) => setSingleSha256(event.target.value)}
                  placeholder="64-character hex digest"
                  spellCheck={false}
                  className="h-9 w-full rounded-md border border-input bg-background px-3 font-mono text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
                />
              </Field>
            ) : null}

            {mode === 'torrent' ? (
              <section className="space-y-3">
                <div className="flex items-center gap-2 text-sm font-semibold text-foreground">
                  <Magnet size={16} className="text-primary" />
                  <span>Add Torrent</span>
                </div>
                <Field label="Torrent URL" hint="Magnet or HTTP(S) .torrent link.">
                  <input
                    ref={inputRef as React.RefObject<HTMLInputElement>}
                    type="text"
                    value={torrentUrl}
                    onChange={(event) => setTorrentUrl(event.target.value)}
                    placeholder="magnet:?xt=urn:btih:... or https://example.com/file.torrent"
                    required
                    className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none transition focus:border-primary focus:ring-2 focus:ring-primary/20"
                  />
                </Field>
              </section>
            ) : null}

            {mode === 'multi' ? (
              <Field label="Download URLs" hint="Paste one HTTP(S) file link per line.">
                <textarea
                  ref={inputRef as React.RefObject<HTMLTextAreaElement>}
                  value={multiUrls}
                  onChange={(event) => setMultiUrls(ensureTrailingEditableLine(event.target.value))}
                  placeholder={'https://example.com/file-01.zip\nhttps://example.com/file-02.zip'}
                  rows={7}
                  wrap={batchUrlTextAreaWrap}
                  className={batchUrlTextAreaClassName}
                />
              </Field>
            ) : null}

            {mode === 'bulk' ? (
              <>
                <Field label="Bulk Links" hint="Paste one HTTP(S) file link per line.">
                  <textarea
                    ref={inputRef as React.RefObject<HTMLTextAreaElement>}
                    value={bulkUrls}
                    onChange={(event) => setBulkUrls(ensureTrailingEditableLine(event.target.value))}
                    placeholder={'https://example.com/assets/model.fbx\nhttps://example.com/assets/textures.zip\nhttps://example.com/assets/readme.pdf'}
                    rows={7}
                    wrap={batchUrlTextAreaWrap}
                    className={batchUrlTextAreaClassName}
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
              <span>{readyLabel}</span>
              <span>{mode === 'torrent' ? 'Torrent' : mode === 'bulk' && combineBulk ? archiveName : 'Queue only'}</span>
            </div>

            {errorMessage ? (
              <p className="rounded-md border border-destructive/35 bg-destructive/10 px-3 py-2 text-sm text-destructive">{errorMessage}</p>
            ) : null}
          </div>

          <div className="flex items-center justify-between gap-3 border-t border-border px-5 py-3">
            <div>
              {mode === 'torrent' ? (
                <button
                  type="button"
                  onClick={() => void handleImportTorrent()}
                  disabled={isImportingTorrent}
                  className="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-semibold text-foreground transition hover:bg-muted disabled:cursor-not-allowed disabled:opacity-50"
                  title="Import magnet or torrent file"
                >
                  <TorrentFileIcon />
                  <span>{isImportingTorrent ? 'Importing...' : 'Import'}</span>
                </button>
              ) : null}
            </div>

            <div className="flex justify-end gap-3">
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
      className={`flex h-8 items-center justify-center gap-1.5 rounded-[4px] text-xs font-semibold transition ${
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
        <label className="text-xs font-semibold text-foreground">{label}</label>
        <span className="text-xs text-muted-foreground">{hint}</span>
      </div>
      {children}
    </div>
  );
}

function TorrentFileIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      aria-hidden="true"
      className="shrink-0"
    >
      <path
        d="M4 1.75h5.25L12 4.5v9.75H4V1.75Z"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinejoin="round"
      />
      <path d="M9.25 1.75V4.5H12" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" />
      <path
        d="M6.15 7.1v2.05a1.85 1.85 0 0 0 3.7 0V7.1"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
      <path d="M6.15 7.1h1.2M8.65 7.1h1.2" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" />
    </svg>
  );
}

function normalizeArchiveName(value: string) {
  const sanitized = value.replace(/[<>:"/\\|?*\u0000-\u001F]/g, '').trimStart();
  if (!sanitized) return '';
  return sanitized.toLowerCase().endsWith('.zip') ? sanitized : `${sanitized}.zip`;
}
