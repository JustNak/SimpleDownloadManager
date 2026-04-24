import React, { useState, useRef, useEffect } from 'react';
import { addJob } from './backend';
import { X } from 'lucide-react';

interface AddDownloadModalProps {
  onClose: () => void;
}

export function AddDownloadModal({ onClose }: AddDownloadModalProps) {
  const [url, setUrl] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!url.trim()) return;

    setIsSubmitting(true);
    setErrorMessage('');
    try {
      await addJob(url);
      onClose();
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Failed to add the download.');
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-sm p-4">
      <div 
        className="bg-card w-full max-w-md rounded-md shadow-2xl border border-border overflow-hidden animate-in fade-in zoom-in-95 duration-200"
      >
        <div className="flex items-center justify-between px-6 py-4 border-b border-border bg-header">
          <h2 className="text-lg font-semibold">New Download</h2>
          <button 
            onClick={onClose} 
            className="p-1.5 rounded-md text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
          >
            <X size={20} />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="p-6">
          <div className="mb-6">
            <label htmlFor="url" className="block text-sm font-medium mb-2 text-foreground">
              Download URL
            </label>
            <input
              ref={inputRef}
              type="url"
              id="url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://example.com/file.zip"
              required
              className="w-full bg-background border border-input rounded-md px-4 py-2.5 text-foreground focus:ring-2 focus:ring-primary/20 focus:border-primary outline-none transition-all"
            />
            {errorMessage ? (
              <p className="mt-2 text-sm text-destructive">{errorMessage}</p>
            ) : null}
          </div>

          <div className="flex justify-end gap-3">
            <button
              type="button"
              onClick={onClose}
              className="px-5 py-2.5 rounded-md font-medium hover:bg-muted text-foreground transition-colors"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={isSubmitting || !url.trim()}
              className="px-5 py-2.5 rounded-md font-medium bg-primary text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
            >
              {isSubmitting ? 'Adding...' : 'Start Download'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
