import type { QueueRowSize } from './types';

export type DetailsLevel = 'compact' | 'standard' | 'expanded';
export type QueueTableAlignment = 'start' | 'center' | 'end';

export const DETAILS_MIN_HEIGHT = 104;
export const DETAILS_CLOSE_THRESHOLD = 84;
export const DETAILS_DEFAULT_HEIGHT = 164;
export const DETAILS_EXPANDED_HEIGHT = 220;
export const DETAILS_MAX_HEIGHT = 300;
export const TABLE_MIN_HEIGHT = 180;
export const QUEUE_TABLE_GRID_CLASS = 'grid-cols-[minmax(420px,2.8fr)_150px_110px_100px_150px_72px]';
export const BULK_QUEUE_TABLE_GRID_CLASS = 'grid-cols-[minmax(420px,2.8fr)_150px_120px_150px_72px]';

export function queueRowSizeClass(size: QueueRowSize): string {
  switch (size) {
    case 'compact':
      return 'min-h-[28px] py-0 text-xs';
    case 'small':
      return 'min-h-[34px] py-0.5 text-xs';
    case 'large':
      return 'min-h-[54px] py-1.5 text-sm';
    case 'damn':
      return 'min-h-[68px] py-2.5 text-base';
    case 'medium':
    default:
      return 'min-h-[42px] py-1 text-sm';
  }
}

export function queueAlignmentClass(align: QueueTableAlignment): string {
  if (align === 'center') return 'justify-center text-center';
  if (align === 'end') return 'justify-end text-right';
  return 'justify-start text-left';
}

export function queueHeaderSelfClass(align: QueueTableAlignment): string {
  if (align === 'center') return 'justify-self-center';
  if (align === 'end') return 'justify-self-end';
  return 'justify-self-start';
}

export function queueHeaderCellClass(align: QueueTableAlignment = 'start'): string {
  return `flex min-w-0 items-center px-1.5 ${queueAlignmentClass(align)}`;
}

export function queueTableCellClass(align: QueueTableAlignment = 'start'): string {
  return `flex min-w-0 items-center px-1.5 tabular-nums text-muted-foreground ${queueAlignmentClass(align)} truncate`;
}

export function queueDateCellClass(): string {
  return queueTableCellClass('center');
}

export function queueMetricCellClass(): string {
  return queueTableCellClass('center');
}

export function getDetailsMaxHeight(containerHeight: number): number {
  if (!Number.isFinite(containerHeight) || containerHeight <= 0) return DETAILS_MAX_HEIGHT;
  return Math.max(DETAILS_MIN_HEIGHT, Math.min(DETAILS_MAX_HEIGHT, containerHeight - TABLE_MIN_HEIGHT));
}

export function detailsLevelForHeight(height: number): DetailsLevel {
  if (height < DETAILS_DEFAULT_HEIGHT) return 'compact';
  if (height < DETAILS_EXPANDED_HEIGHT) return 'standard';
  return 'expanded';
}

export function snapDetailsHeight(value: number, maxHeight: number): number {
  const snapPoints = [
    DETAILS_MIN_HEIGHT,
    Math.min(DETAILS_DEFAULT_HEIGHT, maxHeight),
    Math.min(DETAILS_EXPANDED_HEIGHT, maxHeight),
    maxHeight,
  ].filter((height, index, heights) => height >= DETAILS_MIN_HEIGHT && heights.indexOf(height) === index).sort((a, b) => a - b);
  return snapPoints.reduce((closest, height) => Math.abs(height - value) < Math.abs(closest - value) ? height : closest, snapPoints[0] ?? DETAILS_MIN_HEIGHT);
}

export function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}
