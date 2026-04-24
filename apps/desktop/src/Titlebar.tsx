import React, { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { X, Minus, Square, Copy } from 'lucide-react';
import { AppIcon } from './AppIcon';

export function Titlebar() {
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
    <div
      data-tauri-drag-region 
      className="h-16 shrink-0 bg-background border-b border-border flex items-center justify-between select-none z-50 w-full"
    >
      {/* Left: Branding */}
      <div 
        data-tauri-drag-region
        className="flex items-center gap-5 pl-7 h-full flex-1 cursor-grab active:cursor-grabbing"
        onPointerDown={handleDragPointerDown}
        onDoubleClick={handleDragDoubleClick}
      >
        <AppIcon size={32} className="text-primary pointer-events-none" />
        <span className="text-lg font-semibold text-foreground pointer-events-none">
          Download Manager
        </span>
      </div>

      {/* Right: Window Controls */}
      <div className="flex h-full items-center">
        <button
          className="h-full w-16 flex items-center justify-center hover:bg-muted text-foreground transition-colors"
          onClick={() => void appWindow?.minimize()}
          title="Minimize"
        >
          <Minus size={16} />
        </button>
        <button
          className="h-full w-16 flex items-center justify-center hover:bg-muted text-foreground transition-colors"
          onClick={() => void appWindow?.toggleMaximize()}
          title={isMaximized ? "Restore Down" : "Maximize"}
        >
          {isMaximized ? <Copy size={14} className="rotate-180" /> : <Square size={14} />}
        </button>
        <button
          className="h-full w-16 flex items-center justify-center hover:bg-destructive hover:text-destructive-foreground text-foreground transition-colors"
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
