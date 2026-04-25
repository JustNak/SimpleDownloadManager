import React, { useEffect, useMemo, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { X, Minus, Square, Copy } from 'lucide-react';
import { AppIcon } from './AppIcon';
import { shouldStartWindowDrag } from './windowDrag';

export function Titlebar({ children }: { children?: React.ReactNode }) {
  const [isMaximized, setIsMaximized] = useState(false);
  const appWindow = useMemo(() => (isTauriRuntime() ? getCurrentWindow() : null), []);

  const handleDragPointerDown = async (event: React.PointerEvent<HTMLDivElement>) => {
    if (!appWindow || !shouldStartWindowDrag(event)) {
      return;
    }

    try {
      await appWindow.startDragging();
    } catch {
      // Keep the declarative drag region as the fallback path.
    }
  };

  const handleDragDoubleClick = async (event: React.MouseEvent<HTMLDivElement>) => {
    if (!appWindow || !shouldStartWindowDrag(event)) return;
    await toggleMaximize();
  };

  const refreshMaximized = async () => {
    if (!appWindow) return;

    try {
      setIsMaximized(await appWindow.isMaximized());
    } catch {
      setIsMaximized(false);
    }
  };

  const minimize = async () => {
    if (!appWindow) return;

    try {
      await appWindow.minimize();
    } catch {
      // The web preview has no native window to minimize.
    }
  };

  const toggleMaximize = async () => {
    if (!appWindow) return;

    try {
      await appWindow.toggleMaximize();
      await refreshMaximized();
    } catch {
      await refreshMaximized();
    }
  };

  const close = async () => {
    if (!appWindow) return;

    try {
      await appWindow.close();
    } catch {
      // The web preview has no native window to close.
    }
  };

  useEffect(() => {
    if (!appWindow) return;
    const currentWindow = appWindow;

    let unlisten: (() => void) | undefined;

    async function checkMaximized() {
      try {
        setIsMaximized(await currentWindow.isMaximized());
      } catch {
        setIsMaximized(false);
      }
    }

    checkMaximized();

    const setupListener = async () => {
      unlisten = await currentWindow.onResized(async () => {
        const maximized = await currentWindow.isMaximized();
        setIsMaximized(maximized);
      });
    };

    setupListener();

    return () => {
      if (unlisten) unlisten();
    };
  }, [appWindow]);

  return (
    <div className="titlebar z-50 flex h-11 w-full shrink-0 select-none items-center justify-between border-b border-border bg-background">
      <div
        data-tauri-drag-region
        className="flex h-full w-[252px] shrink-0 cursor-grab items-center gap-3 border-r border-border pl-5 active:cursor-grabbing"
        onPointerDown={handleDragPointerDown}
        onDoubleClick={handleDragDoubleClick}
      >
        <AppIcon size={24} className="pointer-events-none text-primary" />
        <span className="pointer-events-none text-sm font-semibold text-foreground">
          Download Manager
        </span>
      </div>

      <div
        className="flex h-full min-w-0 flex-1 items-center px-4"
        onPointerDown={handleDragPointerDown}
        onDoubleClick={handleDragDoubleClick}
      >
        {children ? (
          <div className="min-w-0 flex-1">{children}</div>
        ) : (
          <div
            data-tauri-drag-region
            className="h-full flex-1 cursor-grab active:cursor-grabbing"
            onPointerDown={handleDragPointerDown}
            onDoubleClick={handleDragDoubleClick}
          />
        )}
      </div>

      <div className="flex h-full shrink-0 items-center">
        <button
          className="flex h-full w-10 items-center justify-center text-foreground transition-colors hover:bg-muted"
          onClick={() => void minimize()}
          title="Minimize"
          aria-label="Minimize"
        >
          <Minus size={16} />
        </button>
        <button
          className="flex h-full w-10 items-center justify-center text-foreground transition-colors hover:bg-muted"
          onClick={() => void toggleMaximize()}
          title={isMaximized ? "Restore Down" : "Maximize"}
          aria-label={isMaximized ? "Restore Down" : "Maximize"}
        >
          {isMaximized ? <Copy size={14} className="rotate-180" /> : <Square size={14} />}
        </button>
        <button
          className="flex h-full w-10 items-center justify-center text-foreground transition-colors hover:bg-destructive hover:text-destructive-foreground"
          onClick={() => void close()}
          title="Close"
          aria-label="Close"
        >
          <X size={16} />
        </button>
      </div>
    </div>
  );
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}
