import React from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { X } from 'lucide-react';
import { AppIcon } from './AppIcon';

export function PopupTitlebar({ title, onClose }: { title: string; onClose?: () => void }) {
  const appWindow = isTauriRuntime() ? getCurrentWindow() : null;

  const handlePointerDown = async (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || !appWindow) return;
    try {
      await appWindow.startDragging();
    } catch {
      // The drag region attribute remains the fallback.
    }
  };

  const handleClose = () => {
    if (onClose) {
      onClose();
      return;
    }
    void appWindow?.close();
  };

  return (
    <div className="flex h-12 shrink-0 select-none items-center justify-between border-b border-border bg-background">
      <div
        data-tauri-drag-region
        className="flex h-full min-w-0 flex-1 cursor-grab items-center gap-3 px-4 active:cursor-grabbing"
        onPointerDown={handlePointerDown}
      >
        <AppIcon size={22} className="pointer-events-none shrink-0 text-primary" />
        <span className="pointer-events-none min-w-0 truncate text-sm font-semibold text-foreground">{title}</span>
      </div>
      <button
        onClick={handleClose}
        className="flex h-full w-11 items-center justify-center text-muted-foreground transition hover:bg-destructive hover:text-destructive-foreground"
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
