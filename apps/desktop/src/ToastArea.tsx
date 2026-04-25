import React, { useEffect } from 'react';
import type { ToastMessage } from './types';
import { X, CheckCircle, AlertCircle, Info, AlertTriangle } from 'lucide-react';

const TOAST_AUTO_CLOSE_MS = 3000;

interface ToastAreaProps {
  toasts: ToastMessage[];
  onDismiss: (id: string) => void;
}

export function ToastArea({ toasts, onDismiss }: ToastAreaProps) {
  if (toasts.length === 0) return null;

  return (
    <div className="fixed bottom-20 right-6 z-50 flex flex-col gap-3 w-full max-w-sm pointer-events-none">
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} onDismiss={() => onDismiss(toast.id)} />
      ))}
    </div>
  );
}

function ToastItem({ toast, onDismiss }: { toast: ToastMessage, onDismiss: () => void }) {
  useEffect(() => {
    if (toast.autoClose !== false) {
      const timer = setTimeout(onDismiss, TOAST_AUTO_CLOSE_MS);
      return () => clearTimeout(timer);
    }
  }, [toast, onDismiss]);

  const typeConfig = {
    success: { icon: <CheckCircle size={20} className="text-green-500" />, border: 'border-green-500/20' },
    error: { icon: <AlertCircle size={20} className="text-red-500" />, border: 'border-red-500/20' },
    warning: { icon: <AlertTriangle size={20} className="text-yellow-500" />, border: 'border-yellow-500/20' },
    info: { icon: <Info size={20} className="text-primary" />, border: 'border-primary/20' },
  };

  const config = typeConfig[toast.type as keyof typeof typeConfig] || typeConfig.info;

  return (
    <div className={`pointer-events-auto flex items-start gap-4 p-4 bg-card border ${config.border} rounded-md shadow-lg animate-in slide-in-from-bottom-5 fade-in duration-300`}>
      <div className="flex-shrink-0 mt-0.5">
        {config.icon}
      </div>
      <div className="flex-1 min-w-0">
        <h4 className="text-sm font-semibold text-foreground mb-1 leading-none">{toast.title}</h4>
        <p className="text-sm text-muted-foreground leading-snug break-words">{toast.message}</p>
      </div>
      <button 
        onClick={onDismiss} 
        className="flex-shrink-0 p-1 -mt-1 -mr-1 rounded-md text-muted-foreground hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-primary"
      >
        <X size={16} />
      </button>
    </div>
  );
}
