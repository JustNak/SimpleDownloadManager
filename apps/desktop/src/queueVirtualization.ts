import type { QueueRowSize } from './types';

export const VIRTUALIZED_QUEUE_THRESHOLD = 100;
export const QUEUE_VIRTUALIZATION_OVERSCAN = 8;

export interface VirtualQueueWindowInput {
  totalCount: number;
  rowSize: QueueRowSize;
  scrollTop: number;
  viewportHeight: number;
}

export interface VirtualQueueWindow {
  enabled: boolean;
  startIndex: number;
  endIndex: number;
  topPadding: number;
  bottomPadding: number;
  rowHeight: number;
}

export function queueRowHeightForSize(size: QueueRowSize): number {
  switch (size) {
    case 'compact':
      return 28;
    case 'small':
      return 34;
    case 'large':
      return 54;
    case 'damn':
      return 68;
    case 'medium':
    default:
      return 42;
  }
}

export function getVirtualQueueWindow({
  totalCount,
  rowSize,
  scrollTop,
  viewportHeight,
}: VirtualQueueWindowInput): VirtualQueueWindow {
  const rowHeight = queueRowHeightForSize(rowSize);
  if (totalCount <= VIRTUALIZED_QUEUE_THRESHOLD) {
    return {
      enabled: false,
      startIndex: 0,
      endIndex: Math.max(0, totalCount),
      topPadding: 0,
      bottomPadding: 0,
      rowHeight,
    };
  }

  const safeScrollTop = Math.max(0, scrollTop);
  const safeViewportHeight = Math.max(rowHeight, viewportHeight);
  const firstVisibleIndex = Math.min(
    Math.max(0, totalCount - 1),
    Math.floor(safeScrollTop / rowHeight),
  );
  const visibleCount = Math.ceil(safeViewportHeight / rowHeight);
  const startIndex = Math.max(0, firstVisibleIndex - QUEUE_VIRTUALIZATION_OVERSCAN);
  const endIndex = Math.min(
    totalCount,
    firstVisibleIndex + visibleCount + QUEUE_VIRTUALIZATION_OVERSCAN,
  );

  return {
    enabled: true,
    startIndex,
    endIndex,
    topPadding: startIndex * rowHeight,
    bottomPadding: Math.max(0, totalCount - endIndex) * rowHeight,
    rowHeight,
  };
}
