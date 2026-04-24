import React, { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { X, Minus, Square, Copy } from 'lucide-react';
import { AppIcon } from './AppIcon';

export function Titlebar({ children }: { children?: React.ReactNode }) {
  const [isMaximized, setIsMaximized] = useState(false);
  const appWindow = isTauriRuntime() ? getCurrentWindow() : null;

  const handleDragPointerDown = async (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || !appWindow) {
      return;
    }

    try {
      await appWindow.startDragging();
    } catch {
      // Keep the declarative drag region as the fallback path.
    }
  };

  const handleDragDoubleClick = async () => {
    if (!appWindow) return;
    await appWindow.toggleMaximize();
  };

  useEffect(() => {
    if (!appWindow) return;
    const currentWindow = appWindow;

    let unlisten: (() => void) | undefined;
    
    async function checkMaximized() {
      setIsMaximized(await currentWindow.isMaximized());
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
    <div className="titlebar z-50 flex h-14 w-full shrink-0 select-none items-center justify-between border-b border-border bg-background">
      <div 
        data-tauri-drag-region
        className="flex h-full w-[252px] shrink-0 cursor-grab items-center gap-4 border-r border-border pl-6 active:cursor-grabbing"
        onPointerDown={handleDragPointerDown}
        onDoubleClick={handleDragDoubleClick}
      >
        <AppIcon size={28} className="pointer-events-none text-primary" />
        <span className="pointer-events-none text-base font-semibold text-foreground">
          Download Manager
        </span>
      </div>

      <div className="flex h-full min-w-0 flex-1 items-center px-5">
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
          className="flex h-full w-12 items-center justify-center text-foreground transition-colors hover:bg-muted"
          onClick={() => void appWindow?.minimize()}
          title="Minimize"
        >
          <Minus size={16} />
        </button>
        <button
          className="flex h-full w-12 items-center justify-center text-foreground transition-colors hover:bg-muted"
          onClick={() => void appWindow?.toggleMaximize()}
          title={isMaximized ? "Restore Down" : "Maximize"}
        >
          {isMaximized ? <Copy size={14} className="rotate-180" /> : <Square size={14} />}
        </button>
        <button
          className="flex h-full w-12 items-center justify-center text-foreground transition-colors hover:bg-destructive hover:text-destructive-foreground"
          onClick={() => void appWindow?.close()}
          title="Close"
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
