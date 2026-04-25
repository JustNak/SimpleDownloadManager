import React, { useMemo } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { X } from 'lucide-react';
import { AppIcon } from './AppIcon';
import { shouldStartWindowDrag } from './windowDrag';

export function PopupTitlebar({ title, onClose }: { title: string; onClose?: () => void }) {
  const appWindow = useMemo(() => (isTauriRuntime() ? getCurrentWindow() : null), []);

  const handleMouseDown = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!appWindow || !shouldStartWindowDrag(event)) return;
    void appWindow.startDragging().catch(() => {
      // The drag region attribute remains the fallback.
    });
  };

  const handleClose = () => {
    if (onClose) {
      onClose();
      return;
    }
    void appWindow?.close();
  };

  return (
    <div
      className="flex h-10 shrink-0 select-none items-center justify-between border-b border-border bg-background"
      onMouseDown={handleMouseDown}
    >
      <div
        data-tauri-drag-region
        className="flex h-full min-w-0 flex-1 cursor-grab items-center gap-2.5 px-3 active:cursor-grabbing"
      >
        <AppIcon size={20} className="pointer-events-none shrink-0 text-primary" />
        <span className="pointer-events-none min-w-0 truncate text-sm font-semibold text-foreground">{title}</span>
      </div>
      <button
        onClick={handleClose}
        className="flex h-full w-10 items-center justify-center text-muted-foreground transition hover:bg-destructive hover:text-destructive-foreground"
        title="Close"
        aria-label="Close"
      >
        <X size={16} />
      </button>
    </div>
  );
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}
