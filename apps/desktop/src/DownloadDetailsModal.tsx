import React from 'react';
import { JobState } from './types';
import type { DownloadJob } from './types';
import { X, Play, Pause, RotateCw, Trash2, HardDrive, File, Activity, Clock, Globe, FolderOpen, MousePointerClick, ExternalLink } from 'lucide-react';

interface DownloadDetailsModalProps {
  job: DownloadJob;
  onClose: () => void;
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
  onRemove: (id: string) => void;
  onOpen: (id: string) => void;
  onReveal: (id: string) => void;
}

function formatBytes(bytes: number, decimals = 2) {
  if (!+bytes) return '0 B';
  const k = 1024;
  const dm = decimals < 0 ? 0 : decimals;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dm))} ${sizes[i]}`;
}

function formatTime(seconds: number) {
  if (seconds === Infinity || isNaN(seconds)) return '--';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  if (m < 60) return `${m}m ${s}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

export function DownloadDetailsModal({ job, onClose, onPause, onResume, onCancel, onRetry, onRemove, onOpen, onReveal }: DownloadDetailsModalProps) {
  const isDownloading = job.state === JobState.Downloading;
  const isPaused = job.state === JobState.Paused;
  const isCompleted = job.state === JobState.Completed;
  const sourceLabel = job.source
    ? `${job.source.browser} ${job.source.entryPoint.replaceAll('_', ' ')}`
    : 'Manual URL';

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-sm animate-in fade-in duration-200">
      <div 
        className="bg-card w-full max-w-2xl max-h-[85vh] rounded-xl shadow-2xl border border-border flex flex-col overflow-hidden animate-in zoom-in-95 duration-200"
      >
        {/* Header */}
        <div className="h-14 border-b border-border flex items-center justify-between px-6 bg-muted/30 shrink-0">
          <div className="flex items-center gap-3 min-w-0">
            <h2 className="font-semibold text-lg truncate" title={job.filename}>
              {job.filename}
            </h2>
          </div>
          <button 
            onClick={onClose}
            className="p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground rounded-md transition-colors"
          >
            <X size={20} />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-6">
          
          {/* Main Progress Ring & Stats */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-6 mb-8">
            <div className="flex items-center justify-center bg-muted/20 rounded-xl p-6 border border-border/50">
              <div className="relative w-32 h-32 flex items-center justify-center">
                <svg className="w-full h-full transform -rotate-90">
                  <circle cx="64" cy="64" r="60" stroke="currentColor" strokeWidth="8" fill="transparent" className="text-muted" />
                  <circle 
                    cx="64" 
                    cy="64" 
                    r="60" 
                    stroke="currentColor" 
                    strokeWidth="8" 
                    fill="transparent" 
                    strokeDasharray={377} 
                    strokeDashoffset={377 - (377 * job.progress) / 100}
                    className={`transition-all duration-300 ${isDownloading ? 'text-primary' : isCompleted ? 'text-green-500' : 'text-muted-foreground'}`}
                  />
                </svg>
                <div className="absolute flex flex-col items-center">
                  <span className="text-2xl font-bold">{job.progress.toFixed(0)}%</span>
                  <span className="text-xs text-muted-foreground uppercase font-medium">{job.state}</span>
                </div>
              </div>
            </div>
            
            <div className="col-span-2 grid grid-cols-2 gap-4">
              <StatBox icon={<File size={16} />} label="Size" value={job.totalBytes > 0 ? formatBytes(job.totalBytes) : 'Unknown'} />
              <StatBox icon={<HardDrive size={16} />} label="Downloaded" value={formatBytes(job.downloadedBytes)} />
              <StatBox icon={<Activity size={16} />} label="Speed" value={isDownloading ? `${formatBytes(job.speed)}/s` : '-'} className="text-primary" />
              <StatBox icon={<Clock size={16} />} label="Time Left" value={isDownloading ? formatTime(job.eta) : '-'} />
              <div className="col-span-2 p-3 bg-muted/20 border border-border/50 rounded-xl flex items-center gap-3 min-w-0">
                <Globe size={16} className="text-muted-foreground shrink-0" />
                <span className="text-xs text-muted-foreground truncate select-all" title={job.url}>{job.url}</span>
              </div>
              <div className="col-span-2 p-3 bg-muted/20 border border-border/50 rounded-xl flex items-center gap-3 min-w-0">
                <FolderOpen size={16} className="text-muted-foreground shrink-0" />
                <span className="text-xs text-muted-foreground truncate select-all" title={job.targetPath}>{job.targetPath || 'No destination recorded yet.'}</span>
              </div>
              <div className="col-span-2 p-3 bg-muted/20 border border-border/50 rounded-xl flex items-center gap-3 min-w-0">
                <MousePointerClick size={16} className="text-muted-foreground shrink-0" />
                <span className="text-xs text-muted-foreground truncate" title={sourceLabel}>{sourceLabel}</span>
              </div>
            </div>
          </div>

          <div className="mb-6 p-4 bg-muted/30 border border-border/50 rounded-xl text-sm text-muted-foreground">
            This build uses a single verified download stream per job. Multi-connection chunking is not enabled yet.
          </div>

          {job.source?.pageTitle || job.source?.pageUrl || job.source?.referrer ? (
            <div className="mb-6 grid grid-cols-1 gap-3">
              {job.source.pageTitle ? (
                <InfoRow label="Page title" value={job.source.pageTitle} />
              ) : null}
              {job.source.pageUrl ? (
                <InfoRow label="Page URL" value={job.source.pageUrl} mono />
              ) : null}
              {job.source.referrer ? (
                <InfoRow label="Referrer" value={job.source.referrer} mono />
              ) : null}
            </div>
          ) : null}

          {/* Error Notice */}
          {job.error && (
            <div className="mb-6 p-4 bg-destructive/10 text-destructive border border-destructive/20 rounded-xl text-sm">
              <span className="font-semibold block mb-1">Error Occurred</span>
              {job.error}
            </div>
          )}
        </div>

        {/* Footer Actions */}
        <div className="p-4 border-t border-border bg-card flex justify-end gap-2 shrink-0">
          {job.state === JobState.Completed && (
            <ActionBtn onClick={() => onOpen(job.id)} icon={<ExternalLink size={16} />} label="Open File" variant="primary" />
          )}
          {job.targetPath && (
            <ActionBtn onClick={() => onReveal(job.id)} icon={<FolderOpen size={16} />} label="Show In Folder" />
          )}
          {(isDownloading || job.state === JobState.Starting) && (
            <ActionBtn onClick={() => onPause(job.id)} icon={<Pause size={16} />} label="Pause" />
          )}
          {isPaused && (
            <ActionBtn onClick={() => onResume(job.id)} icon={<Play size={16} />} label="Resume" variant="primary" />
          )}
          {[JobState.Downloading, JobState.Starting, JobState.Queued, JobState.Paused].includes(job.state) && (
            <ActionBtn onClick={() => onCancel(job.id)} icon={<X size={16} />} label="Cancel" />
          )}
          {[JobState.Failed, JobState.Canceled].includes(job.state) && (
            <ActionBtn onClick={() => onRetry(job.id)} icon={<RotateCw size={16} />} label="Retry" variant="primary" />
          )}
          <ActionBtn onClick={() => { onRemove(job.id); onClose(); }} icon={<Trash2 size={16} />} label="Remove" variant="danger" />
        </div>
      </div>
    </div>
  );
}

function InfoRow({ label, value, mono = false }: { label: string, value: string, mono?: boolean }) {
  return (
    <div className="p-3 bg-muted/20 border border-border/50 rounded-xl flex flex-col gap-1.5 min-w-0">
      <span className="text-xs font-medium uppercase tracking-wider text-muted-foreground">{label}</span>
      <span className={`text-sm text-foreground truncate ${mono ? 'font-mono' : ''}`} title={value}>{value}</span>
    </div>
  );
}

function StatBox({ icon, label, value, className = '' }: { icon: React.ReactNode, label: string, value: string | React.ReactNode, className?: string }) {
  return (
    <div className="p-3 bg-muted/20 border border-border/50 rounded-xl flex flex-col gap-1.5">
      <div className="flex items-center gap-1.5 text-muted-foreground text-xs font-medium uppercase tracking-wider">
        {icon} {label}
      </div>
      <div className={`font-semibold text-base ${className}`}>{value}</div>
    </div>
  );
}

function ActionBtn({ onClick, icon, label, variant = 'default' }: { onClick: () => void, icon: React.ReactNode, label: string, variant?: 'default' | 'primary' | 'danger' }) {
  const baseClasses = "flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors shadow-sm";
  const variants = {
    default: "bg-muted text-foreground hover:bg-muted-foreground/20",
    primary: "bg-primary text-primary-foreground hover:bg-primary/90",
    danger: "bg-destructive text-destructive-foreground hover:bg-destructive/90"
  };

  return (
    <button onClick={onClick} className={`${baseClasses} ${variants[variant]}`}>
      {icon}
      {label}
    </button>
  );
}
